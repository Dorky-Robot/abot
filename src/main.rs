use clap::{Parser, Subcommand};
use std::io::Read;

mod agent;
mod clone;
mod config;
mod employ;
mod git;
mod integrate;
mod manifest;
mod paths;
mod run;
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
        Command::Clone { source, new_name } => {
            clone::clone(&root, &source, &new_name)?;
            eprintln!("cloned {source} -> {new_name}");
        }
        Command::Employ { name, room } => {
            employ::employ(&root, &name, &room)?;
            eprintln!("employed {name} in {room}");
        }
        Command::Dismiss { name, room } => {
            employ::dismiss(&root, &name, &room)?;
            eprintln!("dismissed {name} from {room}");
        }
        Command::Integrate { name, room } => {
            integrate::integrate(&root, &name, &room)?;
            eprintln!("integrated {room} into {name}");
        }
        Command::Discard { name, room } => {
            integrate::discard(&root, &name, &room)?;
            eprintln!("discarded {room} from {name}");
        }
        Command::Log { name, room } => {
            let out = agent::log(&root, &name, room.as_deref())?;
            print!("{out}");
        }
        Command::Diff { name, room } => {
            let out = agent::diff(&root, &name, &room)?;
            print!("{out}");
        }
        Command::Run { name, room } => {
            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input)?;
            let client = run::OllamaClient::default();
            let mut stdout = std::io::stdout();
            run::run(&root, &name, room.as_deref(), &input, &client, &mut stdout)?;
        }
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
