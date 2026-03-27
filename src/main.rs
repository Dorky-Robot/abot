use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

pub mod engine;
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

    /// External hostname (for WebAuthn origin, e.g. abot.dorkyrobot.com)
    #[arg(long)]
    host: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Subcommand)]
enum Command {
    /// Start the server
    Start,
    /// Run the HTTP/WS server (alias for start)
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
    host: Option<String>,
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
    if matches.value_source("host") != Some(clap::parser::ValueSource::CommandLine) {
        if let Some(host) = config.host {
            cli.host = Some(host);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cli.data_dir, std::fs::Permissions::from_mode(0o700))?;
    }

    let command = cli.command.clone().unwrap_or(Command::Start);

    match command {
        Command::Start | Command::Serve => {
            let addr = format!("{}:{}", cli.bind, cli.port);
            server::run(&addr, &cli.data_dir, cli.host.as_deref()).await?;
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

async fn cmd_update(cli: &Cli) -> anyhow::Result<()> {
    let addr = format!("{}:{}", cli.bind, cli.port);

    tracing::info!("abot rolling update");

    // Check for running server
    let server_pid_path = cli.data_dir.join("server.pid");
    if let Some(old_pid) = pid::read_live_pid(&server_pid_path) {
        if !pid::is_abot_process(old_pid) {
            tracing::warn!(
                "PID {} is not an abot process (possible PID reuse), skipping signal",
                old_pid
            );
        } else {
            tracing::info!("sending SIGTERM to old server (pid {})", old_pid);

            unsafe {
                libc::kill(old_pid, libc::SIGTERM);
            }

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

    tracing::info!("starting new server");
    server::run(&addr, &cli.data_dir, cli.host.as_deref()).await
}
