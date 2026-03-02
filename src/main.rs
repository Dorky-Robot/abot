use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
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
    #[arg(short, long, default_value = "0.0.0.0")]
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
    /// Manage setup tokens for device enrollment
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },
}

#[derive(Clone, Subcommand)]
enum TokenAction {
    /// Create a setup token (shown once)
    Create {
        /// Name for the token
        name: String,
    },
    /// List setup tokens
    List,
    /// Revoke a token (and linked credential)
    Revoke {
        /// Token ID to revoke
        id: String,
    },
}

fn default_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".abot")
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct FileConfig {
    port: Option<u16>,
    bind: Option<String>,
}

fn load_config(data_dir: &std::path::Path) -> FileConfig {
    let path = data_dir.join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
            tracing::warn!("invalid config.toml: {}", e);
            FileConfig::default()
        }),
        Err(_) => FileConfig::default(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("abot=info".parse().unwrap()))
        .init();

    let matches = Cli::command().get_matches();
    let mut cli = Cli::from_arg_matches(&matches)?;

    // Ensure data directory exists with owner-only permissions
    std::fs::create_dir_all(&cli.data_dir)?;

    // Load config file and apply where CLI didn't explicitly override
    let config = load_config(&cli.data_dir);
    if matches.value_source("port") != Some(clap::parser::ValueSource::CommandLine) {
        if let Some(port) = config.port {
            cli.port = port;
        }
    }
    if matches.value_source("bind") != Some(clap::parser::ValueSource::CommandLine) {
        if let Some(bind) = config.bind {
            cli.bind = bind;
        }
    }
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
        Command::Token { action } => cmd_token(&cli.data_dir, action)?,
    }

    Ok(())
}

const TOKEN_TTL_SECS: i64 = 24 * 60 * 60;

fn cmd_token(data_dir: &std::path::Path, action: TokenAction) -> anyhow::Result<()> {
    let db_path = data_dir.join("abot.db");
    let db = auth::state::init_db(&db_path)?;

    match action {
        TokenAction::Create { name } => {
            let token = auth::tokens::generate_token();
            let hash = auth::tokens::hash_token(&token)?;
            let id = uuid::Uuid::new_v4().to_string();
            let expires_at = chrono::Utc::now().timestamp() + TOKEN_TTL_SECS;

            auth::state::add_setup_token(&db, &id, &name, &hash, expires_at)?;

            eprintln!("Token created:");
            eprintln!("  ID:      {}", id);
            eprintln!("  Name:    {}", name);
            eprintln!("  Expires: {}", format_expiry(expires_at));
            println!("{}", token);
            eprintln!("\nSave this token — it will not be shown again.");
        }
        TokenAction::List => {
            let tokens = auth::state::get_setup_tokens(&db)?;
            if tokens.is_empty() {
                println!("No tokens.");
                return Ok(());
            }

            println!("{:<38} {:<20} EXPIRES", "ID", "NAME");
            for token in &tokens {
                let status = format_expiry(token.expires_at);
                let cred = auth::state::get_credential_for_token(&db, &token.id)?;
                let cred_info = cred.map(|c| c.name.unwrap_or_else(|| "unnamed".into()));
                let display = match cred_info {
                    Some(name) => format!("{} (enrolled: {})", status, name),
                    None => status,
                };
                println!("{:<38} {:<20} {}", token.id, token.name, display);
            }
        }
        TokenAction::Revoke { id } => {
            let existing = auth::state::get_setup_tokens(&db)?
                .into_iter()
                .any(|t| t.id == id);
            if !existing {
                anyhow::bail!("no token with ID {}", id);
            }
            if let Some(cred) = auth::state::get_credential_for_token(&db, &id)? {
                auth::state::delete_auth_grants_for_credential(&db, &cred.id)?;
                auth::state::delete_credential(&db, &cred.id)?;
            }
            auth::state::delete_setup_token(&db, &id)?;
            println!("Token {} revoked.", id);
        }
    }

    Ok(())
}

fn format_expiry(expires_at: i64) -> String {
    let remaining = expires_at - chrono::Utc::now().timestamp();
    if remaining <= 0 {
        return "expired".to_string();
    }
    let hours = remaining / 3600;
    let mins = (remaining % 3600) / 60;
    if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

/// Spawn the daemon as a detached process with setsid().
fn spawn_daemon(data_dir: &std::path::Path) -> anyhow::Result<()> {
    tracing::info!("spawning daemon as separate process");

    let exe = std::env::current_exe()?;
    let log_path = data_dir.join("daemon.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let stderr_file = log_file.try_clone()?;

    unsafe {
        ProcessCommand::new(&exe)
            .arg("--data-dir")
            .arg(data_dir)
            .arg("daemon")
            .stdout(log_file)
            .stderr(stderr_file)
            .stdin(std::process::Stdio::null())
            .pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            })
            .spawn()?;
    }

    Ok(())
}

async fn cmd_start(cli: &Cli) -> anyhow::Result<()> {
    let data_dir = cli.data_dir.clone();
    let addr = format!("{}:{}", cli.bind, cli.port);

    tracing::info!("abot starting (daemon + server)");

    // Check if daemon is already running via PID file
    if !pid::daemon_is_running(&data_dir) {
        spawn_daemon(&data_dir)?;
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

    // Spawn daemon supervisor — restarts daemon if it dies
    let supervisor_data_dir = data_dir.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            if !pid::daemon_is_running(&supervisor_data_dir) {
                tracing::warn!("daemon process died, restarting...");
                if let Err(e) = spawn_daemon(&supervisor_data_dir) {
                    tracing::error!("failed to restart daemon: {}", e);
                }
            }
        }
    });

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
