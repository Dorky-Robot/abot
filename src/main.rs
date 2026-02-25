use clap::{Parser, Subcommand};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use tracing_subscriber::EnvFilter;

mod daemon;
mod error;
pub mod pid;
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

    // Ensure data directory exists with owner-only permissions
    std::fs::create_dir_all(&cli.data_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cli.data_dir, std::fs::Permissions::from_mode(0o700))?;
    }

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

    // Check if daemon is already running via PID file
    if !pid::daemon_is_running(&data_dir) {
        tracing::info!("spawning daemon as separate process");

        let exe = std::env::current_exe()?;
        let log_path = data_dir.join("daemon.log");
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let stderr_file = log_file.try_clone()?;

        // Use pre_exec with setsid() to fully detach the daemon so it
        // doesn't become a zombie when the parent (server) exits.
        unsafe {
            ProcessCommand::new(&exe)
                .arg("--data-dir")
                .arg(&data_dir)
                .arg("daemon")
                .stdout(log_file)
                .stderr(stderr_file)
                .stdin(std::process::Stdio::null())
                .pre_exec(|| {
                    libc::setsid();
                    Ok(())
                })
                .spawn()?;
        }
    } else {
        tracing::info!("daemon already running, reusing");
    }

    // Wait for daemon socket to appear (5s timeout: 50 x 100ms)
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

    // Run server in foreground — daemon continues independently
    server::run(&addr, &data_dir).await
}

async fn cmd_update(cli: &Cli) -> anyhow::Result<()> {
    let data_dir = cli.data_dir.clone();
    let addr = format!("{}:{}", cli.bind, cli.port);

    tracing::info!("abot rolling update");

    // Step 1: Check daemon is running
    if !pid::daemon_is_running(&data_dir) {
        tracing::info!("daemon not running, falling back to full start");
        return cmd_start(cli).await;
    }

    // Step 2: Check for running server
    let server_pid_path = data_dir.join("server.pid");
    if let Some(old_pid) = pid::read_live_pid(&server_pid_path) {
        // Verify this PID is actually an abot process before signaling
        if !pid::is_abot_process(old_pid) {
            tracing::warn!(
                "PID {} is not an abot process (possible PID reuse), skipping signal",
                old_pid
            );
        } else {
            tracing::info!("sending SIGTERM to old server (pid {})", old_pid);

            // Step 3: Send SIGTERM
            unsafe {
                libc::kill(old_pid, libc::SIGTERM);
            }

            // Step 4: Wait for old server to exit (100ms intervals, 10s timeout)
            let mut exited = false;
            for _ in 0..100 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if !pid::process_alive(old_pid) {
                    exited = true;
                    break;
                }
            }

            if !exited {
                tracing::warn!("old server didn't exit gracefully, sending SIGKILL");
                unsafe {
                    libc::kill(old_pid, libc::SIGKILL);
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            tracing::info!("old server stopped");
        }
    } else {
        tracing::info!("no running server found, starting fresh");
    }

    // Step 5: Start new server
    tracing::info!("starting new server");
    server::run(&addr, &data_dir).await
}
