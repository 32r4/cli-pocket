use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cli_pocket_relay_core::{RelayConfig, RelayServer};

#[derive(Parser)]
#[command(name = "cli-pocket-relay", version, about = "cli-pocket relay")]
struct Cli {
    /// Path to relay config TOML file.
    #[arg(long, env = "CLI_POCKET_RELAY_CONFIG")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the relay (default).
    Serve,
    /// Print a sample config TOML.
    PrintSampleConfig,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let cfg = match &cli.config {
        Some(p) => RelayConfig::load_from(p).with_context(|| format!("load {p:?}"))?,
        None => RelayConfig::default(),
    };

    match cli.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => {
            let server = RelayServer::new(cfg);
            tokio::select! {
                r = server.serve() => r?,
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("ctrl-c received; exiting");
                }
            }
        }
        Cmd::PrintSampleConfig => {
            println!("{}", toml::to_string_pretty(&RelayConfig::default())?);
        }
    }

    Ok(())
}
