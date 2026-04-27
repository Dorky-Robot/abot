use clap::{Parser, Subcommand};

mod git;
mod paths;

#[derive(Parser)]
#[command(name = "abot", version, about = "Headless CLI for AI agent identities")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new agent identity
    Create { name: String },

    /// List all agents
    #[command(alias = "ls")]
    List,

    /// Show agent details and branches
    Show { name: String },

    /// Clone an agent into a new identity
    Clone { source: String, new_name: String },

    /// Create a worktree binding the agent to a room
    Employ { name: String, room: String },

    /// Remove a worktree, keeping the branch
    Dismiss { name: String, room: String },

    /// Merge a room's branch into the agent's main and delete the branch
    Integrate { name: String, room: String },

    /// Delete a room's branch without merging
    Discard { name: String, room: String },

    /// Run the agent: read stdin, dispatch to LLM, write stdout
    Run {
        name: String,
        /// Run inside the worktree of a specific room
        #[arg(long = "in")]
        room: Option<String>,
    },

    /// Show git log (optionally for a specific room)
    Log {
        name: String,
        #[arg(long)]
        room: Option<String>,
    },

    /// Show what changed in a room vs the agent's main
    Diff { name: String, room: String },

    /// Get or set config values
    Config {
        name: String,
        key: Option<String>,
        value: Option<String>,
    },

    /// Delete an agent entirely
    Rm { name: String },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Create { .. } => todo!("phase 1"),
        Command::List => todo!("phase 1"),
        Command::Show { .. } => todo!("phase 1"),
        Command::Clone { .. } => todo!("phase 1"),
        Command::Employ { .. } => todo!("phase 1"),
        Command::Dismiss { .. } => todo!("phase 1"),
        Command::Integrate { .. } => todo!("phase 1"),
        Command::Discard { .. } => todo!("phase 1"),
        Command::Run { .. } => todo!("phase 1"),
        Command::Log { .. } => todo!("phase 1"),
        Command::Diff { .. } => todo!("phase 1"),
        Command::Config { .. } => todo!("phase 1"),
        Command::Rm { .. } => todo!("phase 1"),
    }
}
