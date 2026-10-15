//! abot: the CLI for a git-based ecosystem log.
//!
//! An application on top of git and diwa. Everything it does is plain git
//! underneath and can be done by hand; it holds no locks and installs no
//! hooks. Its guard rails are templates, checks and refusals.

mod branch;
mod cmd;
mod ctx;
mod diwa;
mod git;
mod out;
mod repos;
mod secrets;
mod statefile;
mod trailers;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

const ABOUT: &str = "The CLI for the ecosystem log: orient, start, land, dead-end, park.";

const LONG_ABOUT: &str = "\
The CLI for the ecosystem log (~/Projects/ecosystem-log, or $ABOT_LOG).

state/ is what is true now, commits are what happened and why, and
<project>/<task> branches are what is being tried. abot is the easy path
through that: everything it does is plain git, it holds no locks, and its
guard rails are checks and refusals, never hooks.

Lifecycle:  orient -> start -> (work, commit) -> land | dead-end | park

Environment: ABOT_LOG (the log), ABOT_PROJECTS (where code repos live,
default ~/Projects), ABOT_DECISIONS (felix-decisions.json), ABOT_DIWA
(diwa binary; empty to skip diwa).";

#[derive(Parser)]
#[command(name = "abot", version, about = ABOUT, long_about = LONG_ABOUT)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Everything the log knows about a project, from main, in a screen or two
    #[command(long_about = "Fetch origin and read origin/main (never the local checkout; local main, said loudly, only if origin is unreachable), then print: the ecosystem/ summary, the \
project's state file, how it got here, decisions about it, dead ends, in-flight branches, \
and the nearest diwa insights for the log and the code repo. Output is capped; empty \
sections say (nothing). With --cross, several projects with the map rows between them.")]
    Orient {
        /// Project slug (a file in state/projects/)
        project: Option<String>,
        /// More projects, for cross-project work
        #[arg(long, num_args = 1..)]
        cross: Vec<String>,
    },
    /// Start <project>/<task>: worktrees in the log and code repo, a first commit with the why, push
    #[command(long_about = "Create the worktree ../ecosystem-log-<task> on branch <project>/<task> \
from origin/main, and ../<repo>-<task> in the project's code repo if one is found. The first log \
commit is empty, says why, and carries `Outcome: in-flight`; the log branch is pushed so \
everyone can see the path is taken. Refuses a bare <project>, a project with no state file \
(--new scaffolds one), and a branch that already exists on origin.",
        override_usage = "abot start [OPTIONS] <project>/<task> \"<why>\"")]
    Start {
        /// The branch: <project>/<task>, e.g. everyday-vet/group-thread
        #[arg(value_name = "<project>/<task>")]
        branch: String,
        /// Why this path is worth trying, in quotes (becomes the first commit)
        #[arg(value_name = "\"<why>\"")]
        why: Option<String>,
        /// Scaffold state/projects/<project>.md for a new project
        #[arg(long)]
        new: bool,
        /// The code repo, when it can't be found from the state file or by name
        #[arg(long)]
        repo: Option<PathBuf>,
        /// The Claude Code session working this task, recorded as a `Session:` trailer
        /// (default: $CLAUDE_CODE_BRIDGE_SESSION_ID, which Remote Control sessions have)
        #[arg(long)]
        session: Option<String>,
    },
    /// Land the task you are standing in: rebase, fast-forward main, push, clean up
    #[command(long_about = "Run inside a log worktree. Refuses if nothing under state/, \
ecosystem/, how/ or people/ changed (unless --no-state-change \"<why>\"), and if the diff \
looks like it holds a secret. Warns on state files that lose their headings or pass 60 \
lines. Adds an empty `Land <project>/<task>` commit with `Outcome: landed` unless the last \
commit already says landed, rebases on main, fast-forwards main, pushes, removes the \
worktree and deletes the branch. The code repo's branch, if any, lands first the same way. \
Never force-pushes, never rewrites main.")]
    Land {
        /// Land although no state file changed, and say why
        #[arg(long, value_name = "WHY")]
        no_state_change: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
    },
    /// Record the task you are standing in as a dead end, and keep the lesson
    #[command(name = "dead-end", long_about = "Run inside a log worktree. Commits the lesson as \
the branch's last commit (\"Dead end: <project>/<task>. <failed>\", body Tried/Failed/Instead, \
`Outcome: dead-end`), merges it into main with -s ours --no-ff so main's files don't change \
but the history keeps it, adds a Traps line to the project's state file on main, pushes, and \
cleans up. A code branch is pushed and kept.")]
    DeadEnd {
        /// What we tried
        #[arg(long)]
        tried: String,
        /// Why it failed (its first line goes in the subject)
        #[arg(long)]
        failed: String,
        /// What to do instead (becomes the Traps line)
        #[arg(long)]
        instead: String,
        #[arg(long)]
        repo: Option<PathBuf>,
    },
    /// Park the task you are standing in: push the branch, note it under Parked on main
    #[command(long_about = "Run inside a log worktree. Adds an empty commit with `Outcome: \
parked`, pushes the branch, adds a line under Parked in the project's state file on main, \
pushes main, and removes the worktree. The branch is kept. Park briefly: diwa only indexes \
what main reaches.")]
    Park {
        /// Why it waits
        why: String,
        #[arg(long)]
        repo: Option<PathBuf>,
    },
    /// Step into a commit: its message, its Refs: into other repos, then diwa show
    Dig {
        /// A commit in the log, or in any code repo under ~/Projects
        sha: String,
    },
    /// Record one of Felix's decisions in felix-decisions.json (the mirror lands it)
    Decide {
        /// The decision, in Felix's words where possible
        text: String,
        #[arg(long)]
        why: String,
        /// Projects it is about, comma-separated
        #[arg(long, value_delimiter = ',', required = true)]
        about: Vec<String>,
        #[arg(long, default_value = "Felix")]
        by: String,
        #[arg(long)]
        json: bool,
    },
    /// Every project: live, next, waits on Felix, in flight, dead ends, uses/serves
    Map {
        #[arg(long)]
        json: bool,
    },
    /// Lint the log: shapes, lengths, map, stale branches, secrets, orphan branches
    Check {
        #[arg(long)]
        json: bool,
    },
    /// The project's discovery tree: main and <project>/* with their outcomes
    Tree {
        project: String,
        /// How many commits to show
        #[arg(short = 'n', long, default_value_t = 60)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Mirror felix-decisions.json into decisions/ on main, one commit per decision
    Mirror,
    /// Which Claude Code session worked which task, and how the task ended
    #[command(long_about = "Read origin/main and every task branch on origin for `Session:` \
trailers (abot start records the session; land, dead-end and park carry it onto the commit \
that ends the task). One row per session, from the newest commit that names it: session, \
outcome (in-flight, landed, dead-end, parked), branch, sha, date. The session sweeper reads \
--json to know whose work is finished.")]
    Sessions {
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    // Die quietly when the reader goes away (`abot … | head`), as a Unix
    // tool should, instead of panicking on a write to a closed pipe.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    let ctx = ctx::Ctx::from_env();
    let r = match cli.cmd {
        Cmd::Orient { project, cross } => {
            let mut all: Vec<String> = project.into_iter().collect();
            all.extend(cross);
            cmd::orient::run(&ctx, all).map(|_| true)
        }
        Cmd::Start { branch, why, new, repo, session } => {
            cmd::lifecycle::start(&ctx, &branch, why.as_deref().unwrap_or(""), new, repo.as_deref(), session.as_deref())
                .map(|_| true)
        }
        Cmd::Land { no_state_change, repo } => {
            cmd::lifecycle::land(&ctx, no_state_change.as_deref(), repo.as_deref()).map(|_| true)
        }
        Cmd::DeadEnd { tried, failed, instead, repo } => {
            cmd::lifecycle::dead_end(&ctx, &tried, &failed, &instead, repo.as_deref()).map(|_| true)
        }
        Cmd::Park { why, repo } => cmd::lifecycle::park(&ctx, &why, repo.as_deref()).map(|_| true),
        Cmd::Dig { sha } => cmd::dig::run(&ctx, &sha).map(|_| true),
        Cmd::Decide { text, why, about, by, json } => {
            cmd::decisions::decide(&ctx, &text, &why, &about, &by, json).map(|_| true)
        }
        Cmd::Map { json } => cmd::map::map(&ctx, json).map(|_| true),
        Cmd::Check { json } => cmd::map::check(&ctx, json),
        Cmd::Tree { project, limit, json } => cmd::map::tree(&ctx, &project, limit, json).map(|_| true),
        Cmd::Mirror => cmd::decisions::mirror(&ctx).map(|_| true),
        Cmd::Sessions { json } => cmd::sessions::run(&ctx, json).map(|_| true),
    };
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("abot: {e:#}");
            ExitCode::FAILURE
        }
    }
}
