# abot v0 (archived)

This commit retires abot v0. The name goes to the CLI for the ecosystem log
(Dorky-Robot/ecosystem-log), which lands in the commits after this one.

## What v0 was

A headless Rust CLI in which **an agent identity was a git repository**
(`~/.abot/agents/<name>.abot`). History was the agent's memory, and the
lifecycle verbs were thin mappings onto git:

- `employ <name> <room>`: a worktree on a `kubo/<room>` branch
- `integrate <name> <room>`: merge that branch back into the agent's main
- `discard <name> <room>`: drop the branch
- `run <name>`: stdin to a local Ollama model, stream to stdout

plus create, list, show, clone, dismiss, log, diff, config and rm. Before
the headless rewrite (v1.0.0, April 2026) it was a spatial terminal with an
Axum server, a Flutter client and Docker sandboxes ("kubos").

## Why it is archived

Felix, 2026-10-15: "why dont we just take abot arhieve it and then take its
place with this and call this abot. i feel like that was a failed attempt
heavy handed thing and this is actually a more elegant solution that we can
use to what we need now."

- **The unit was wrong.** One repo per agent. What the org needed was one
  shared log with a branch per task, so that any agent on any machine can
  pick up where the last one stopped. Memory belongs to the work, not to
  the worker.
- **It was heavy.** Per-agent repos under `~/.abot`, a manifest and config
  per agent, an Ollama dispatcher, a server and client before that. None of
  it had users.

## What survived

The shape of the verbs. A task is a branch with a worktree; work that pays
off is merged; work that doesn't is dropped, and git keeps the record. The
new abot keeps that shape (`start`, `land`, `dead-end`, `park`) and applies
it to the ecosystem log, where a dead end lands as a lesson instead of
being thrown away.

## Where the old code is

Nothing is deleted from history. The tag `v0-archived` points at the last v0
commit, and the releases v0.1.0, v0.2.0 and v1.0.0 still have their binaries:

    git checkout v0-archived

diwa holds about 60 insights on those 70 commits:
`diwa search Dorky-Robot/abot "<query>"`.
