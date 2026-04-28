use clap::{Parser, Subcommand};

mod agent;
mod config;
mod git;
mod manifest;
mod paths;
mod settings;

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
    let root = paths::default_root()?;

    match cli.command {
        Command::Create { name } => {
            agent::create(&root, &name)?;
            eprintln!("created agent: {name}");
        }
        Command::List => {
            for n in agent::list(&root)? {
                println!("{n}");
            }
        }
        Command::Show { name } => {
            let info = agent::show(&root, &name)?;
            print_show(&info);
        }
        Command::Rm { name } => {
            agent::rm(&root, &name)?;
            eprintln!("removed agent: {name}");
        }
        Command::Config { name, key, value } => match (key, value) {
            (None, _) => {
                let cfg = agent::config_read(&root, &name)?;
                println!("{}", serde_json::to_string_pretty(&cfg)?);
            }
            (Some(k), None) => {
                let v = agent::config_get(&root, &name, &k)?;
                println!("{v}");
            }
            (Some(k), Some(v)) => {
                agent::config_set(&root, &name, &k, &v)?;
            }
        },
        Command::Clone { .. }
        | Command::Employ { .. }
        | Command::Dismiss { .. }
        | Command::Integrate { .. }
        | Command::Discard { .. }
        | Command::Run { .. }
        | Command::Log { .. }
        | Command::Diff { .. } => todo!("phase 1: not yet implemented"),
    }

    Ok(())
}

fn print_show(info: &agent::AgentInfo) {
    println!("name:    {}", info.manifest.name);
    println!("model:   {}", info.config.model);
    println!("shell:   {}", info.config.shell);
    println!("created: {}", info.manifest.created.to_rfc3339());
    println!("updated: {}", info.manifest.updated.to_rfc3339());
    if !info.config.instructions.is_empty() {
        println!("instructions:");
        for line in info.config.instructions.lines() {
            println!("  {line}");
        }
    }
    println!("branches:");
    for b in &info.branches {
        println!("  {b}");
    }
    println!("worktrees:");
    for wt in &info.worktrees {
        let branch = wt.branch.as_deref().unwrap_or("?");
        println!("  {branch}\t{}", wt.path.display());
    }
}
