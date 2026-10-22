# abot

The CLI for a git-based ecosystem log: the shared memory of a group of
agents and the person they work for.

The log is a plain git repo (`~/Projects/ecosystem-log`):

- `state/` is **what is true now**, overwritten as it changes;
- commits are **what happened and why**, never rewritten;
- `<project>/<task>` branches are **what is being tried**, and every one
  ends as landed, a dead end (which still lands, as its lesson), or parked.

abot is the easy path through that. Everything it does is plain git
underneath and can be done by hand. It holds no locks, installs no hooks
and wraps nothing; its guard rails are templates, checks and refusals.
diwa (semantic search over commits) is called as a user would call it.

## Install

```sh
brew install dorky-robot/tap/abot
```

## The lifecycle

```sh
abot orient everyday-vet                 # what the log knows, capped at ~150 lines
abot start everyday-vet/group-thread "why this is worth trying"
#   worktrees: ../ecosystem-log-group-thread and ../<code-repo>-group-thread
#   first commit: "Start everyday-vet/group-thread: …"  Outcome: in-flight
#   the log branch is pushed, so everyone can see the path is taken
cd ../ecosystem-log-group-thread
#   work; edit state/projects/everyday-vet.md when the truth changes; commit
abot land                                # rebase, ff main, push, clean up (both repos)
abot dead-end --tried "…" --failed "…" --instead "…"
abot park "waits on Felix's answer about X"
```

| verb | does | refuses |
|---|---|---|
| `orient <p> [--cross p2 …]` | fetch, then read `origin/main`: ecosystem summary, state file, how it got here, decisions, dead ends, in-flight branches, diwa | a bad slug |
| `start <p>/<t> "<why>"` | log + code worktrees, first empty commit with the why (`Session: session_…` from `$CLAUDE_CODE_BRIDGE_SESSION_ID` or `--session`; `Parent: …`, who started this session, from `$CLAUDE_PARENT_SESSION_ID` or `--parent`; `Outcome: in-flight`), push | a bare `<p>`; no state file (unless `--new`, which scaffolds one); a branch already on origin |
| `land [--ready <p>/<id>.<n>]` | marks the ticket steps the task finished (from `Ready:` lines, `--ready`, or a branch named for a one-step-left ticket); `Outcome: landed` commit if needed (with `Code: <org/repo> <from>..<to>`, the exact range it put on the code trunk), rebase on main, ff main, push, remove worktree, delete branch; code repo first | no change under state/ecosystem/how/people (unless `--no-state-change "<why>"`); secrets in the diff; uncommitted work |
| `dead-end --tried --failed --instead` | lesson commit (`Dead end: <p>/<t>. …`, `Code:` the branch's own commits, `Outcome: dead-end`), `merge -s ours --no-ff`, a Traps line on main, push, clean up; code branch pushed and kept | any part missing |
| `park "<why>"` | `Outcome: parked` commit (with `Code:` the kept branch), push the branch, a Parked line on main, remove the worktree, keep the branch | |
| `dig <sha>` | the commit, its `Refs:` into code repos, its `Code:` range as commits with diwa's insights on each, `diwa show` | |
| `decide "<text>" --why … --about p1,p2` | appends to `~/Projects/ceo/felix-decisions.json` | the machine that doesn't hold it |
| `map [--json]` | every project: live, next, waits on Felix, in flight, dead ends, last change, uses/serves | |
| `check [--json]` | state-file shape and length, tickets written out (the same rules as `ticket add`), map.md current, stale branches (>7 days), branches with no project, secrets | exits 1 on any problem |
| `tree <p> [--json]` | `git log --graph` of main and `<p>/*` with outcomes | |
| `sessions [--json]` | one row per `Session:` trailer on origin (main and task branches): session, parent (from its Start commit's `Parent:`), outcome, branch, sha, date; land, dead-end and park carry the Start commit's `Session:` onto the commit that ends the task, so the session sweeper knows whose work is finished | |
| `mirror` | decisions JSON → `decisions/<id>.md`, one commit each, idempotent, push | the main checkout off main |
| `ticket add <p> "<title>" --why … --done-when … --step …` | hands the draft to the ticket writer (below); files it as one commit on origin's main, or prints the writer's questions and exits 2 | a project with no state file; a taken id; a ticket that fails the rules |
| `ticket answer <session> "…"` | resumes the writer's session with the answers; files or asks again | no such conversation in this clone |
| `ticket step <p>/<id> "…"` · `<p>/<id>.<n> "…"` · `<p>/<id>.<n> --done` | add a step, reword one (its number stays), mark one done | a step that reads like a label |
| `ticket retire <p>/<id> "<why>" [--title …]` | moves it under Retired; `--title` records an old one so its id stays taken | |
| `ticket list [<p>] [--json]` | open tickets; `--json` (what overview reads) carries each one's steps, `written_out` and `problems` | |

## Tickets

Felix, 10-15: "these tickets are gibberish... how do we make them actually
understandable". A ticket is work a project has ready, in
`state/tickets/<project>.md`, written by `abot ticket` and never by hand. It
says what the work is and why, what done looks like to someone with no
context, its steps as plain sentences, and who it waits on, if anyone.

`ticket add` runs **the ticket writer**: a cheap `claude -p` on the agent's
own box (Sonnet at low effort, `ABOT_TICKET_MODEL` to change; the model is
always named, since unnamed it is the costly default), with no tools, no
MCP, no settings files and no hooks, given the draft, the project's state
file and the rules. It is a co-writer, not a reviewer: it keeps the agent's
meaning and writes it plainly, or asks up to three questions; `ticket answer`
resumes the same session (`--resume`), so it remembers. The questions and
answers go in the filing commit's body. `ticket::lint` checks the same rules
in abot itself, on every filing and in `abot check`, so a thin ticket can't
land with or without the model. A first live ticket took two rounds and
$0.05.

Every ticket change is one commit made with git's plumbing against
`origin/main` in a private index and pushed straight to origin (retried on
top of a newer main), so filing never touches a checkout. `abot land` marks
the steps a task finished: `Ready: <p>/<id>.<n>` in its log or code commits,
`--ready <p>/<id>.<n>`, or, on a branch named `<p>/<id>` whose ticket has one
open step, that step. A ticket whose steps are all done moves to Done.

Reads (`orient`, `map`, `tree`, `dig`, `check`) fetch and read `origin/main`, the copy of record, never the local checkout, so a stale, dirty or detached clone can't mislead them; if origin can't be reached they say "origin unreachable, reading local main as of <date>" and go on. `start` branches from `origin/main`.

Never force-pushes; never rewrites main. Run `abot <verb> --help` for details.

## Where things are

| | default | override |
|---|---|---|
| the log | `~/Projects/ecosystem-log` | `ABOT_LOG` |
| code repos | main checkouts in `~/Projects/*/` and `~/Projects/*/*/` | `ABOT_PROJECTS`; `--repo <path>` |
| decisions | `~/Projects/ceo/felix-decisions.json` | `ABOT_DECISIONS` |
| diwa | `diwa` on PATH | `ABOT_DIWA` (empty skips it) |
| the ticket writer | `claude` on PATH, model `sonnet` | `ABOT_CLAUDE`, `ABOT_TICKET_MODEL` |
| writer conversations waiting on answers | the log clone's git dir, `abot-tickets/` | |

A project's code repo is the one named by a `Repo:` line in its state file
(`Repo: Dorky-Robot/overview`, or a path), else the repo whose origin has
the project's name.

## History

abot v0 (2026) was a different tool: an agent identity as a git repo, with
employ/integrate/discard as worktree/merge/drop and an Ollama `run`. It was
archived as a dead end, because one repo per agent was the wrong unit and
the stack around it was heavy; the shape of its verbs survives here. Its
code is at tag `v0-archived`, and its releases (up to v1.0.0) are still on
the releases page.

## License

MIT OR Apache-2.0.
