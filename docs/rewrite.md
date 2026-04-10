# abot rewrite — headless agent primitive

**Status:** planned rewrite. The current codebase (spatial terminal +
Axum server + Flutter client + daemon) is a spike. No external users.
This doc describes what abot becomes.

## What abot is

A headless CLI that manages AI agent identities. An agent is a git
repository with a name, config, and working directory. The CLI handles
creation, cloning, employing agents into rooms (via git worktrees),
and integrating their experience back (via git merge).

abot knows nothing about Docker, UIs, sessions, or HTTP. It is a git
workflow tool specialized for agent lifecycle management.

## Why git

An agent's value is its accumulated experience — the files it has
created, the configs it has tuned, the context it has built up over
time. Git gives us:

- **Identity**: the repo IS the agent. Its history is its memory.
- **Branching**: work in multiple rooms simultaneously without
  conflicts. Each room gets a `kubo/<room-name>` branch.
- **Cloning**: snapshot an agent for a task. The clone diverges; the
  original is untouched.
- **Merging**: bring experience back. `git merge kubo/<room>` folds
  what the agent learned in that room into its main identity.
- **Audit trail**: `git log` shows everything the agent has ever done.
- **Diffing**: `git diff` shows exactly what changed in a room.

No custom database. No proprietary format. Standard git, standard
tools.

## Data model

```
~/.abot/
  agents/
    alice.abot/                  ← canonical git repo (alice's identity)
      .git/                      ← owns all history, all branches
      .gitignore                 ← excludes credentials, scrollback
      manifest.json              ← { name, version, created, updated }
      config.json                ← { shell, env, instructions }
      home/                      ← the agent's working directory
    bob.abot/
      ...
  kubos/                         ← worktrees for employed agents
    daily-room/
      alice/                     ← git worktree on branch kubo/daily-room
        .git                     ← file (not dir), points to alice.abot/.git
        manifest.json
        config.json
        home/                    ← bind-mounted into kubo container
```

### manifest.json

```json
{
  "name": "alice",
  "version": "0.1.0",
  "created": "2026-04-10T12:00:00Z",
  "updated": "2026-04-10T14:30:00Z"
}
```

### config.json

```json
{
  "shell": "/bin/zsh",
  "env": {
    "EDITOR": "vim"
  },
  "instructions": "You are alice, a general-purpose assistant."
}
```

The `instructions` field is the agent's personality/system prompt.
Callers (humOS, shell scripts) read this when launching the agent in a
kubo.

## CLI

```
abot create <name>                     Create a new agent identity
abot list                              List all agents
abot show <name>                       Show agent details + branches
abot clone <source> <new-name>         Clone an agent (new git repo)
abot employ <name> <room>              Create worktree on kubo/<room> branch
abot dismiss <name> <room>             Remove worktree, keep branch
abot integrate <name> <room>           Merge kubo/<room> into main, delete branch
abot discard <name> <room>             Delete branch without merging
abot log <name> [--room <room>]        Show git log (optionally for a specific room)
abot diff <name> <room>                Show what changed in a room vs main
abot config <name> [key] [value]       Get/set config values
abot rm <name>                         Delete an agent entirely
```

### Lifecycle

```
create ──→ employ ──→ [agent works in kubo] ──→ dismiss ──→ integrate
                                                    │            │
                                                    │            ↓
                                                    │     experience merged
                                                    │     into main identity
                                                    │
                                                    └──→ discard
                                                          (throw away work)
```

For task-specific clones:

```
clone ──→ employ clone ──→ [clone works] ──→ integrate clone back into original
                                                 │
                                                 └──→ or discard clone entirely
```

## Git operations under the hood

### `abot create alice`

```bash
mkdir -p ~/.abot/agents/alice.abot
cd ~/.abot/agents/alice.abot
git init
echo '{"name":"alice","version":"0.1.0",...}' > manifest.json
echo '{"shell":"/bin/zsh","env":{},...}' > config.json
mkdir home
cat > .gitignore <<'EOF'
credentials.json
scrollback
EOF
git add -A
git commit -m "initialize agent: alice"
```

### `abot clone alice alice-draft`

```bash
# Full clone — new repo, not a worktree. Independent identity.
git clone ~/.abot/agents/alice.abot ~/.abot/agents/alice-draft.abot
cd ~/.abot/agents/alice-draft.abot
# Update manifest name
jq '.name = "alice-draft"' manifest.json > tmp && mv tmp manifest.json
git add manifest.json
git commit -m "cloned from alice"
```

### `abot employ alice daily-room`

```bash
cd ~/.abot/agents/alice.abot
git branch kubo/daily-room 2>/dev/null || true
git worktree add ~/.abot/kubos/daily-room/alice kubo/daily-room
```

The worktree at `~/.abot/kubos/daily-room/alice/home/` is what gets
bind-mounted into the kubo container. All commits land on the
`kubo/daily-room` branch in alice's canonical repo.

### `abot dismiss alice daily-room`

```bash
git worktree remove ~/.abot/kubos/daily-room/alice
# Branch kubo/daily-room still exists — history preserved.
```

### `abot integrate alice daily-room`

```bash
cd ~/.abot/agents/alice.abot
git merge kubo/daily-room -m "integrate experience from daily-room"
git branch -d kubo/daily-room
```

### `abot discard alice daily-room`

```bash
cd ~/.abot/agents/alice.abot
# Remove worktree if still exists
git worktree remove ~/.abot/kubos/daily-room/alice 2>/dev/null || true
git branch -D kubo/daily-room
```

## Composition with kubo

abot and kubo are independent tools. The caller composes them:

```bash
# Create room and agent
kubo new draft-room ~/project
abot create writer
abot employ writer draft-room

# Mount agent's home into the room
kubo add draft-room ~/.abot/kubos/draft-room/writer/home

# Agent works
kubo exec draft-room -- bash -c 'cd /work/home && claude "write a draft"'

# Bring experience home, tear down
abot integrate writer draft-room
kubo rm draft-room
```

humOS wraps this into higher-level commands. A shell script can do the
same. abot doesn't care who calls it.

## Composition with tao

Agents can gate the human for approval mid-workflow:

```bash
# Inside a kubo, agent drafts something then asks the human
cat draft.txt | tao approve reviewer felix
# Pipeline blocks until felix replies
# Agent continues with felix's feedback
```

tao doesn't know about abot. abot doesn't know about tao. They compose
via pipes.

## What abot does NOT do

- **Run containers.** That's kubo.
- **Manage sessions.** That's kubo or the caller.
- **Serve HTTP/WebSocket.** No server. No daemon. Just a CLI.
- **Render UI.** No Flutter, no canvas, no terminal UI.
- **Authenticate users.** No passkeys, no tokens.
- **Poll for events.** No watchers, no filesystem monitors.
- **Orchestrate workflows.** That's humOS or the caller.

## Implementation

Rust. Single crate. Dependencies: `clap` (CLI), `serde`/`serde_json`
(manifest/config), `chrono` (timestamps). Git operations via shelling
out to `git` (not libgit2) — keeps the binary small, uses the user's
git config, and avoids linking complexity.

Estimated size: ~1500-2500 lines of Rust.

```
src/
  main.rs          CLI entry point (clap)
  agent.rs         create, list, show, rm, config
  clone.rs         clone (full repo copy)
  employ.rs        employ, dismiss (worktree add/remove)
  integrate.rs     integrate, discard (merge/delete branch)
  manifest.rs      manifest.json read/write
  config.rs        config.json read/write
  git.rs           git shell-out helpers
  paths.rs         ~/.abot/ path resolution
```

## Migration from current abot

The current abot codebase is a spike. The rewrite is a clean start:

1. New branch (`headless`) or fresh repo — TBD.
2. No code carried over. The only things preserved are the ideas:
   git-backed identity, the employ/dismiss/integrate lifecycle, and
   the `~/.abot/` directory layout (simplified).
3. The Flutter client, Axum server, daemon, Docker orchestration,
   WebAuthn auth, ring buffers, session management — all removed.
   Container management is kubo's job. UI is a future concern that
   lives in a separate project.
4. Existing `~/.abot/` directories from the spike are compatible —
   the agent repos and worktree layout are the same. The rewrite just
   drops everything that isn't git operations on those repos.
