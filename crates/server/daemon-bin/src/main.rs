//! `cli-pocket-daemon` binary entry point.
//!
//! Thin clap-based CLI wrapper over [`cli_pocket_daemon_core`]. Subcommands
//! either operate on persistent state (identity / client DB) or drive the
//! long-running daemon lifecycle.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cli_pocket_daemon_core::client_db::ClientDb;
use cli_pocket_daemon_core::config::DaemonConfig;
use cli_pocket_daemon_core::identity_store::load_or_create;
use cli_pocket_daemon_core::service::{build_config_template, load_or_create_config};
use cli_pocket_daemon_core::Daemon;
use cli_pocket_proto::ClientId;
use rustls::crypto::aws_lc_rs;
use uuid::Uuid;

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
    /// Print the canonical relay pairing URL as a terminal QR code.
    PairQr,
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
    install_rustls_crypto_provider();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    // `print-sample-config` must work without loading a config file.
    if matches!(cli.cmd, Cmd::PrintSampleConfig) {
        print!("{}", build_config_template());
        return Ok(());
    }

    let cfg = load_or_create_config(cli.config.clone())?;

    match cli.cmd {
        Cmd::Start => run_start(cfg).await?,
        Cmd::PairKey => {
            let id = load_or_create(&cfg.security.identity_path)
                .context("load or create daemon identity")?;
            println!("server_id = {}", id.server_id.0);
            println!("public_pk = {}", hex::encode(id.keypair.public));
        }
        Cmd::PairUrl => {
            let daemon = Daemon::boot(cfg.clone()).await.context("boot daemon")?;
            let url = daemon.pair_url().context("build pair url")?;
            println!("{url}");
        }
        Cmd::PairQr => {
            let daemon = Daemon::boot(cfg.clone()).await.context("boot daemon")?;
            let qr = daemon.pair_qr_code().context("build pair QR code")?;
            println!("{}", qr.terminal);
            println!("{}", qr.url);
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

fn install_rustls_crypto_provider() {
    let _ = aws_lc_rs::default_provider().install_default();
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
    use super::Cli;
    use clap::Parser;
    use cli_pocket_daemon_core::config::{
        AppConfig, RelayConfig, RelayRetryConfig, SecurityConfig,
    };
    use cli_pocket_daemon_core::service::{
        dev_config_template, load_or_create_config, load_or_create_config_with_template, pair_url,
    };
    use cli_pocket_daemon_core::DaemonConfig;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn pair_url_subcommand_parses() {
        let cli = Cli::try_parse_from(["cli-pocket-daemon", "pair-url"])
            .expect("pair-url subcommand should parse");

        assert!(matches!(cli.cmd, super::Cmd::PairUrl));
    }

    #[test]
    fn pair_qr_subcommand_parses() {
        let cli = Cli::try_parse_from(["cli-pocket-daemon", "pair-qr"])
            .expect("pair-qr subcommand should parse");

        assert!(matches!(cli.cmd, super::Cmd::PairQr));
    }

    #[tokio::test]
    async fn pair_url_builds_canonical_offer_url() {
        let dir = TempDir::new().expect("temp dir");
        let config = DaemonConfig {
            app: AppConfig {
                base_url: "https://cli-pocket.example/".to_owned(),
            },
            relay: RelayConfig {
                base_url: "wss://relay.example".to_owned(),
                psk_hex: "22".repeat(32),
                server_auth_token: None,
                retry: RelayRetryConfig::default(),
            },
            security: test_security_config(dir.path()),
            ..DaemonConfig::default()
        };

        let url = pair_url(config).await.expect("build pair url");

        assert!(url.starts_with("https://cli-pocket.example/#pair="));
    }

    #[tokio::test]
    async fn pair_url_requires_relay_psk() {
        let dir = TempDir::new().expect("temp dir");
        let err = pair_url(DaemonConfig {
            security: test_security_config(dir.path()),
            relay: RelayConfig::default(),
            ..DaemonConfig::default()
        })
        .await
        .expect_err("missing relay psk should fail");

        assert!(err.to_string().contains("relay.psk_hex"));
    }

    #[test]
    fn load_or_create_config_writes_default_file() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("daemon.toml");
        let config = load_or_create_config(Some(path.clone())).expect("load config");

        assert!(path.exists());
        assert_eq!(
            config.security.identity_path,
            dir.path().join("identity.json")
        );
        assert_eq!(config.relay.psk_hex.len(), 64);
    }

    #[test]
    fn load_or_create_config_with_template_uses_dev_defaults() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("daemon.toml");
        let config = load_or_create_config_with_template(path.clone(), dev_config_template())
            .expect("load config");

        assert!(path.exists());
        assert_eq!(config.listen.port, 17842);
        assert_eq!(config.relay.base_url, "wss://relay.cli-pocket.32r4.asia");
        assert_eq!(
            config.relay.psk_hex,
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff"
        );
        assert_eq!(config.app.base_url, "http://127.0.0.1:5173");
        assert_eq!(
            config.security.identity_path,
            dir.path().join("identity.json")
        );
    }

    fn test_security_config(base: &Path) -> SecurityConfig {
        SecurityConfig {
            identity_path: base.join("identity.json"),
            clients_path: base.join("clients.json"),
            revoked_path: base.join("revoked.json"),
        }
    }
}
