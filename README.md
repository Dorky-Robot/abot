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
| `orient <p> [--cross p2 …]` | pull main; ecosystem summary, state file, how it got here, decisions, dead ends, in-flight branches, diwa | a bad slug |
| `start <p>/<t> "<why>"` | log + code worktrees, first empty commit with the why (`Outcome: in-flight`), push | a bare `<p>`; no state file (unless `--new`, which scaffolds one); a branch already on origin |
| `land` | `Outcome: landed` commit if needed, rebase on main, ff main, push, remove worktree, delete branch; code repo first | no change under state/ecosystem/how/people (unless `--no-state-change "<why>"`); secrets in the diff; uncommitted work |
| `dead-end --tried --failed --instead` | lesson commit (`Dead end: <p>/<t>. …`, `Outcome: dead-end`), `merge -s ours --no-ff`, a Traps line on main, push, clean up; code branch pushed and kept | any part missing |
| `park "<why>"` | `Outcome: parked` commit, push the branch, a Parked line on main, remove the worktree, keep the branch | |
| `dig <sha>` | the commit, its `Refs:` into code repos, `diwa show` | |
| `decide "<text>" --why … --about p1,p2` | appends to `~/Projects/ceo/felix-decisions.json` | the machine that doesn't hold it |
| `map [--json]` | every project: live, next, waits on Felix, in flight, dead ends, last change, uses/serves | |
| `check [--json]` | state-file shape and length, map.md current, stale branches (>7 days), branches with no project, secrets | exits 1 on any problem |
| `tree <p> [--json]` | `git log --graph` of main and `<p>/*` with outcomes | |
| `mirror` | decisions JSON → `decisions/<id>.md`, one commit each, idempotent, push | the main checkout off main |

Never force-pushes; never rewrites main. Run `abot <verb> --help` for details.

## Where things are

| | default | override |
|---|---|---|
| the log | `~/Projects/ecosystem-log` | `ABOT_LOG` |
| code repos | main checkouts in `~/Projects/*/` and `~/Projects/*/*/` | `ABOT_PROJECTS`; `--repo <path>` |
| decisions | `~/Projects/ceo/felix-decisions.json` | `ABOT_DECISIONS` |
| diwa | `diwa` on PATH | `ABOT_DIWA` (empty skips it) |

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
