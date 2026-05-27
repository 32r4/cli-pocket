//! `cli-pocket-daemon` binary entry point.
//!
//! Thin clap-based CLI wrapper over [`cli_pocket_daemon_core`]. Subcommands
//! either operate on persistent state (identity / client DB) or drive the
//! long-running daemon lifecycle.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cli_pocket_daemon_core::client_db::ClientDb;
use cli_pocket_daemon_core::config::{build_pairing_offer_url, DaemonConfig, PairingOffer};
use cli_pocket_daemon_core::identity_store::{load_or_create, DaemonIdentity};
use cli_pocket_daemon_core::Daemon;
use cli_pocket_proto::ClientId;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "cli-pocket-daemon", version, about = "cli-pocket daemon")]
struct Cli {
    /// Path to TOML config; if omitted, defaults are used.
    #[arg(long, env = "CLI_POCKET_CONFIG")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the daemon listener.
    Start,
    /// Print the host's pairing public key.
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
        let cfg = DaemonConfig::default();
        println!("{}", toml::to_string_pretty(&cfg)?);
        return Ok(());
    }

    let cfg = match &cli.config {
        Some(p) => {
            DaemonConfig::load_from(p).with_context(|| format!("load config {}", p.display()))?
        }
        None => DaemonConfig::default(),
    };

    match cli.cmd {
        Cmd::Start => run_start(cfg).await?,
        Cmd::PairKey => {
            let id = load_or_create(&cfg.security.identity_path)
                .context("load or create daemon identity")?;
            println!("host_id   = {}", id.host_id.0);
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
            println!("new host_id = {}", new.host_id.0);
        }
        Cmd::PrintSampleConfig => unreachable!("handled above"),
    }

    Ok(())
}

fn build_pair_url(cfg: &DaemonConfig, identity: &DaemonIdentity) -> Result<String> {
    let relay = cfg
        .relay
        .as_ref()
        .context("pair-url requires [relay] configuration")?;
    let relay_url = relay.url.trim();
    anyhow::ensure!(!relay_url.is_empty(), "pair-url requires relay.url");

    let relay_psk_hex = relay.psk_hex.trim();
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
            host_id: identity.host_id,
            server_public_hex: hex::encode(identity.keypair.public),
            relay_url: relay_url.to_owned(),
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
    use super::{build_pair_url, Cli};
    use clap::Parser;
    use cli_pocket_crypto::KeyPair;
    use cli_pocket_daemon_core::config::{AppConfig, RelayConfig, SecurityConfig};
    use cli_pocket_daemon_core::identity_store::DaemonIdentity;
    use cli_pocket_daemon_core::DaemonConfig;
    use cli_pocket_proto::HostId;
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
            relay: Some(RelayConfig {
                url: "wss://relay.example/ws/client?host=test".to_owned(),
                psk_hex: "22".repeat(32),
                host_token: None,
            }),
            security: test_security_config(),
            ..DaemonConfig::default()
        };

        let url = build_pair_url(&config, &identity).expect("build pair url");

        assert!(url.starts_with("https://cli-pocket.example/#pair="));
    }

    #[test]
    fn pair_url_requires_relay_configuration() {
        let err = build_pair_url(
            &DaemonConfig {
                security: test_security_config(),
                ..DaemonConfig::default()
            },
            &test_identity(),
        )
        .expect_err("missing relay should fail");

        assert!(err.to_string().contains("[relay] configuration"));
    }

    fn test_identity() -> DaemonIdentity {
        let keypair = KeyPair::generate().expect("generate keypair");

        DaemonIdentity {
            host_id: HostId(Uuid::now_v7()),
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
