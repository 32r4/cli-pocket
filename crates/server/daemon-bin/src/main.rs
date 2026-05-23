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
        Some(p) => DaemonConfig::load_from(p).with_context(|| format!("load config {p:?}"))?,
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
        Cmd::ListClients => {
            let db = ClientDb::open(&cfg.security.clients_path, &cfg.security.revoked_path)
                .await
                .context("open client db")?;
            for c in db.list().await {
                println!(
                    "{}  {}  {}",
                    c.client_id.0,
                    c.label,
                    hex::encode(&c.public_key[..6])
                );
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
                std::fs::rename(p, &bak).with_context(|| format!("back up identity to {bak:?}"))?;
                eprintln!("old identity moved to {bak:?}");
            }
            let new = load_or_create(p).context("generate new identity")?;
            println!("new host_id = {}", new.host_id.0);
        }
        Cmd::PrintSampleConfig => unreachable!("handled above"),
    }

    Ok(())
}

async fn run_start(cfg: DaemonConfig) -> Result<()> {
    let mut daemon = Daemon::boot(cfg).await.context("boot daemon")?;
    daemon.start().await.context("start daemon")?;
    tokio::signal::ctrl_c().await.context("await ctrl-c")?;
    tracing::info!("ctrl-c received, shutting down");
    daemon.shutdown().await;
    Ok(())
}
