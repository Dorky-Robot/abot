use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

mod daemon;
mod error;
mod server;

pub mod auth;
pub mod stream;

#[derive(Parser)]
#[command(name = "abot", about = "AI-native spatial interface")]
struct Cli {
    /// Data directory
    #[arg(long, default_value_os_t = default_data_dir())]
    data_dir: PathBuf,

    /// Port to listen on
    #[arg(short, long, default_value = "6969")]
    port: u16,

    /// Bind address
    #[arg(short, long, default_value = "127.0.0.1")]
    bind: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Subcommand)]
enum Command {
    /// Start both daemon and server
    Start,
    /// Run the daemon (PTY session owner)
    Daemon,
    /// Run the HTTP/WS server
    Serve,
    /// Rolling update: drain and restart server
    Update,
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".abot")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("abot=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    // Ensure data directory exists
    std::fs::create_dir_all(&cli.data_dir)?;

    let command = cli.command.clone().unwrap_or(Command::Start);

    match command {
        Command::Start => cmd_start(&cli).await?,
        Command::Daemon => daemon::run(&cli.data_dir).await?,
        Command::Serve => {
            let addr = format!("{}:{}", cli.bind, cli.port);
            server::run(&addr, &cli.data_dir).await?;
        }
        Command::Update => cmd_update(&cli).await?,
    }

    Ok(())
}

async fn cmd_start(cli: &Cli) -> anyhow::Result<()> {
    let data_dir = cli.data_dir.clone();
    let addr = format!("{}:{}", cli.bind, cli.port);

    tracing::info!("abot starting (daemon + server)");

    // Spawn daemon in background task
    let daemon_data_dir = data_dir.clone();
    let daemon_handle = tokio::spawn(async move {
        if let Err(e) = daemon::run(&daemon_data_dir).await {
            tracing::error!("daemon error: {}", e);
        }
    });

    // Wait for daemon socket to appear
    let sock_path = data_dir.join("daemon.sock");
    for _ in 0..50 {
        if sock_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    if !sock_path.exists() {
        anyhow::bail!("daemon socket did not appear at {:?}", sock_path);
    }

    tracing::info!("daemon ready, starting server");

    // Run server in foreground
    let server_result = server::run(&addr, &data_dir).await;

    // If server exits, also stop daemon
    daemon_handle.abort();

    server_result
}

async fn cmd_update(_cli: &Cli) -> anyhow::Result<()> {
    tracing::info!("update not yet implemented");
    Ok(())
}
