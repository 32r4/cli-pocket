//! `cli-pocket-daemon` binary entry point.
//!
//! Thin clap-based CLI wrapper over [`cli_pocket_daemon_core`]. Subcommands
//! either operate on persistent state (identity / client DB) or drive the
//! long-running daemon lifecycle.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cli_pocket_daemon_core::client_db::ClientDb;
use cli_pocket_daemon_core::config::{
    build_pairing_offer_url, default_config_path, relay_client_ws_url_for_server, DaemonConfig,
    PairingOffer, SecurityConfig,
};
use cli_pocket_daemon_core::identity_store::{load_or_create, DaemonIdentity};
use cli_pocket_daemon_core::Daemon;
use cli_pocket_proto::ClientId;
use uuid::Uuid;

const BUILD_CONFIG_TEMPLATE: &str = include_str!("../daemon.build.toml");

#[derive(Parser)]
#[command(name = "cli-pocket-daemon", version, about = "cli-pocket daemon")]
struct Cli {
    /// Path to TOML config; if omitted, uses ~/.cli-pocket/daemon.toml.
    #[arg(long, env = "CLI_POCKET_CONFIG")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the daemon listener.
    Start,
    /// Print the server's pairing public key.
    PairKey,
    /// Print the canonical relay pairing URL.
    PairUrl,
    /// List paired clients.
    ListClients,
    /// Revoke a client by ID.
    Revoke {
        /// UUID of the client to revoke.
        client_id: String,
    },
    /// Regenerate the daemon identity (DANGEROUS — invalidates all pairings).
    RegenerateIdentity {
        #[arg(long)]
        yes_i_understand_this_breaks_all_clients: bool,
    },
    /// Print a sample config TOML to stdout.
    PrintSampleConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    // `print-sample-config` must work without loading a config file.
    if matches!(cli.cmd, Cmd::PrintSampleConfig) {
        print!("{BUILD_CONFIG_TEMPLATE}");
        return Ok(());
    }

    let cfg = load_config(cli.config.as_ref())?;

    match cli.cmd {
        Cmd::Start => run_start(cfg).await?,
        Cmd::PairKey => {
            let id = load_or_create(&cfg.security.identity_path)
                .context("load or create daemon identity")?;
            println!("server_id = {}", id.server_id.0);
            println!("public_pk = {}", hex::encode(id.keypair.public));
        }
        Cmd::PairUrl => {
            let id = load_or_create(&cfg.security.identity_path)
                .context("load or create daemon identity")?;
            let url = build_pair_url(&cfg, &id)?;
            println!("{url}");
        }
        Cmd::ListClients => {
            let db = ClientDb::open(&cfg.security.clients_path, &cfg.security.revoked_path)
                .await
                .context("open client db")?;
            for c in db.list().await {
                println!("{}  {}", c.client_id.0, hex::encode(&c.public_key[..6]));
            }
        }
        Cmd::Revoke { client_id } => {
            let uuid = Uuid::parse_str(&client_id)
                .with_context(|| format!("parse client_id {client_id:?}"))?;
            let cid = ClientId(uuid);
            let db = ClientDb::open(&cfg.security.clients_path, &cfg.security.revoked_path)
                .await
                .context("open client db")?;
            db.revoke(cid).await.context("revoke client")?;
            println!("revoked {}", cid.0);
        }
        Cmd::RegenerateIdentity {
            yes_i_understand_this_breaks_all_clients,
        } => {
            anyhow::ensure!(
                yes_i_understand_this_breaks_all_clients,
                "refusing to regenerate without --yes-i-understand-this-breaks-all-clients"
            );
            let p = &cfg.security.identity_path;
            if p.exists() {
                let bak = p.with_extension("json.bak");
                std::fs::rename(p, &bak)
                    .with_context(|| format!("back up identity to {}", bak.display()))?;
                eprintln!("old identity moved to {}", bak.display());
            }
            let new = load_or_create(p).context("generate new identity")?;
            println!("new server_id = {}", new.server_id.0);
        }
        Cmd::PrintSampleConfig => unreachable!("handled above"),
    }

    Ok(())
}

fn load_config(config_path: Option<&PathBuf>) -> Result<DaemonConfig> {
    let path = config_path.cloned().unwrap_or_else(default_config_path);

    if path.exists() {
        return DaemonConfig::load_from(&path)
            .with_context(|| format!("load config {}", path.display()));
    }

    let mut cfg = DaemonConfig {
        security: SecurityConfig::for_config_path(&path),
        ..DaemonConfig::default()
    };
    cfg.relay.psk_hex = generate_relay_psk_hex().context("generate relay.psk_hex")?;
    cfg.save_to(&path)
        .with_context(|| format!("create default config {}", path.display()))?;
    Ok(cfg)
}

fn generate_relay_psk_hex() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).context("read OS random bytes")?;
    Ok(hex::encode(bytes))
}

fn build_pair_url(cfg: &DaemonConfig, identity: &DaemonIdentity) -> Result<String> {
    let relay_url = relay_client_ws_url_for_server(&cfg.relay.base_url, identity.server_id)
        .context("pair-url requires relay.base_url to be a valid ws:// or wss:// URL")?;

    let relay_psk_hex = cfg.relay.psk_hex.trim();
    anyhow::ensure!(!relay_psk_hex.is_empty(), "pair-url requires relay.psk_hex");

    let relay_psk =
        hex::decode(relay_psk_hex).context("pair-url relay.psk_hex must be valid hex")?;
    anyhow::ensure!(
        relay_psk.len() == 32,
        "pair-url requires relay.psk_hex to decode to 32 bytes"
    );

    build_pairing_offer_url(
        &cfg.app.base_url,
        &PairingOffer {
            label: None,
            server_id: identity.server_id,
            server_public_hex: hex::encode(identity.keypair.public),
            relay_url,
            relay_psk_hex: relay_psk_hex.to_owned(),
        },
    )
    .map_err(Into::into)
}

async fn run_start(cfg: DaemonConfig) -> Result<()> {
    let mut daemon = Daemon::boot(cfg).await.context("boot daemon")?;
    daemon.start().await.context("start daemon")?;
    tokio::signal::ctrl_c().await.context("await ctrl-c")?;
    tracing::info!("ctrl-c received, shutting down");
    daemon.shutdown().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_pair_url, generate_relay_psk_hex, Cli};
    use clap::Parser;
    use cli_pocket_crypto::KeyPair;
    use cli_pocket_daemon_core::config::{AppConfig, RelayConfig, SecurityConfig};
    use cli_pocket_daemon_core::identity_store::DaemonIdentity;
    use cli_pocket_daemon_core::DaemonConfig;
    use cli_pocket_proto::ServerId;
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn pair_url_subcommand_parses() {
        let cli = Cli::try_parse_from(["cli-pocket-daemon", "pair-url"])
            .expect("pair-url subcommand should parse");

        assert!(matches!(cli.cmd, super::Cmd::PairUrl));
    }

    #[test]
    fn pair_url_builds_canonical_offer_url() {
        let identity = test_identity();
        let config = DaemonConfig {
            app: AppConfig {
                base_url: "https://cli-pocket.example/".to_owned(),
            },
            relay: RelayConfig {
                base_url: "wss://relay.example".to_owned(),
                psk_hex: "22".repeat(32),
                server_auth_token: None,
            },
            security: test_security_config(),
            ..DaemonConfig::default()
        };

        let url = build_pair_url(&config, &identity).expect("build pair url");

        assert!(url.starts_with("https://cli-pocket.example/#pair="));
    }

    #[test]
    fn pair_url_requires_relay_psk() {
        let err = build_pair_url(
            &DaemonConfig {
                security: test_security_config(),
                relay: RelayConfig::default(),
                ..DaemonConfig::default()
            },
            &test_identity(),
        )
        .expect_err("missing relay psk should fail");

        assert!(err.to_string().contains("relay.psk_hex"));
    }

    #[test]
    fn generated_relay_psk_has_32_random_bytes() {
        let psk_hex = generate_relay_psk_hex().expect("generate relay psk");
        assert_eq!(psk_hex.len(), 64);
        assert!(psk_hex.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    fn test_identity() -> DaemonIdentity {
        let keypair = KeyPair::generate().expect("generate keypair");

        DaemonIdentity {
            server_id: ServerId(Uuid::now_v7()),
            keypair,
        }
    }

    fn test_security_config() -> SecurityConfig {
        SecurityConfig {
            identity_path: PathBuf::from("identity.json"),
            clients_path: PathBuf::from("clients.json"),
            revoked_path: PathBuf::from("revoked.json"),
        }
    }
}
