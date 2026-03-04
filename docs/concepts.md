# Concepts

## Sessions

A **session** is the core abstraction in abot. Each session is a server-side resource — a PTY process running inside a Docker container. Only terminal I/O crosses the wire between client and server.

Sessions persist across server restarts because the **daemon** owns them independently. You can restart, update, or crash the server and every session keeps running.

Every session runs inside a **kubo** — a Docker container that hosts one or more abots via `docker exec`. See [Kubos](#kubos) below.

!!! tip "First launch"
    On first launch, you see a terminal and can start typing. Kubos, abots, and git concepts are invisible until you need them — they surface progressively as you grow into the tool.

### Session Lifecycle

| Action | What Happens |
|--------|-------------|
| **Create** | Exec into a kubo container, creates `~/.abot/abots/{name}.abot/home/`, inits git repo |
| **Attach** | Client joins session, receives ring buffer snapshot (output history) |
| **Input** | Keystrokes routed to the session's PTY stdin |
| **Output** | PTY stdout broadcast to all attached clients |
| **Resize** | Terminal dimensions updated (TIOCSWINSZ) |
| **Detach** | Client leaves session — session keeps running |
| **Close** | Ends the exec session; bundle directory stays for reopening later |
| **Delete** | Ends the exec session AND deletes the bundle directory |
| **Save** | Writes metadata files + auto-commits git repo (`autosave {timestamp}`) |
| **Save As** | Copies the entire bundle directory to a new path |
| **Rename** | Updates the session name in-place |
| **Open** | Reads an existing `.abot` bundle and creates a session from it |

### Session Generation Counter

Each session carries a monotonic `generation` counter (global `AtomicU64`). When a session name is reused (delete then recreate), the new session gets a higher generation. Background tasks (output relays) compare generations to detect that they belong to a stale session and stop relaying.

### Multi-Client Attach

Multiple clients can attach to the same session simultaneously. All clients see the same output in real time — useful for pair programming or monitoring. Input from any client goes to the same PTY.

### Auto-Save

Dirty sessions (those with unsaved environment changes) are automatically saved every **5 minutes**. The autosave writes metadata and auto-commits the abot's git repo. The home directory is bind-mounted live, so filesystem changes are always persisted.

### Session Environment

Each session gets a base environment:

| Variable | Value |
|----------|-------|
| `TERM` | `xterm-256color` |
| `COLORTERM` | `truecolor` |
| `LANG` | `en_US.UTF-8` |

Additional environment variables can be injected per-session (via bundle config) or globally (via the Anthropic API key feature, which pushes `ANTHROPIC_API_KEY` and `CLAUDE_API_KEY` to all running sessions).

## Kubos

A **kubo** is a runtime room — a long-lived Docker container that hosts abots. Abots share the container's tools, packages, and resources. Every session in abot runs inside a kubo.

### How Kubos Work

```
┌─────────────────────────────────────────────────┐
│  Kubo Container (abot-kubo-default)             │
│  CMD: sleep infinity                            │
│  Memory: 2 GB · CPU: 100% · PIDs: 512          │
│                                                 │
│  /home/abots/                                   │
│    alice/home/ ← docker exec session (PTY)      │
│    bob/home/   ← docker exec session (PTY)      │
│    carol/home/ ← docker exec session (PTY)      │
└─────────────────────────────────────────────────┘
```

1. The kubo container runs `sleep infinity` — it's a long-lived host process
2. Each abot is a **git worktree** from the canonical abot repo, checked out on a `kubo/<kubo-name>` branch
3. The entire kubo dir is bind-mounted as `/home/abots/` — each abot's `home/` appears at `/home/abots/{name}/home/`
4. Sessions use `docker exec` to create PTY sessions inside the already-running container
5. Credentials live at the kubo level (`credentials.json`), not per-abot — they're injected into the container environment

A kubo is **not** a git repo itself — it's infrastructure (Docker container + manifest + credentials). The version-controlled work belongs to each abot's canonical repo.

### Container Specs

| Setting | Value |
|---------|-------|
| **Image** | `abot-kubo` (custom Dockerfile per kubo), fallback `alpine:3` |
| **User** | `1000:1000` (non-root) |
| **Memory** | 2 GB |
| **CPU** | 100% of one core (shared across all abots) |
| **PIDs** | Max 512 processes |
| **Security** | All capabilities dropped, no-new-privileges |
| **Mount** | Kubo directory → `/home/abots/` (read-write bind) |

### Session Reference Counting

The kubo tracks active sessions with `session_opened()` / `session_closed()` calls:

- `session_opened()` — increments the active session count, clears idle timer
- `session_closed()` — decrements the count; when it hits zero, starts the idle timer

### Idle Timeout

When a kubo has **zero active sessions** for **5 minutes**, the container is automatically stopped. This prevents idle containers from consuming resources indefinitely. The container is restarted on the next session creation.

### Custom Dockerfiles

Each kubo can have a custom `Dockerfile` in its directory. When present, abot builds a custom image (`abot-kubo-{name}`) from it. This lets you install project-specific tools, languages, or dependencies that all abots in the kubo share.

### Kubo Directory Structure

```
~/.abot/kubos/default.kubo/
  manifest.json                  ← name, version, abots list
  credentials.json               ← kubo-level API keys (injected into container)
  Dockerfile                     ← optional custom image
  alice/                         ← git worktree of alice.abot on branch kubo/default
    .git                         ← file pointing to alice.abot/.git
    manifest.json
    home/                        ← bind-mounted as /home/abots/alice/home
  bob/                           ← git worktree of bob.abot on branch kubo/default
    .git
    home/
```

### Kubo Manifest

```json
{
  "version": 1,
  "name": "default",
  "created_at": "2026-03-01T10:00:00Z",
  "updated_at": "2026-03-01T10:00:00Z",
  "abots": ["alice", "bob"]
}
```

The manifest tracks which abots are currently employed in the kubo.

## Abots as Git Repos

Every `.abot` bundle is a **git repository**. This is automatic — abot initializes git on bundle creation and auto-commits on save.

### Auto-Initialization

When a bundle is created (or opened for the first time after migrating from v1), abot runs:

1. Writes a `.gitignore` to exclude sensitive and volatile files
2. `git init` (uses the system's default branch name via `init.defaultBranch`)
3. `git add -A && git commit -m "Initial abot snapshot"`
4. Updates `manifest.json` version to `2`

### Auto-Commit on Save

Every save operation (including autosave) runs:

```
git add -A
git commit -m "autosave 2026-03-03 14:22:00 UTC"
```

If nothing changed, no commit is made.

When an abot is employed in a kubo (as a worktree), autosave commits land on the `kubo/<kubo-name>` branch in the canonical repo — no separate push step needed.

### .gitignore

The default `.gitignore` excludes sensitive and cache files:

```
credentials.json
scrollback
scrollback.tmp
home/.cache/
home/.local/share/
home/.claude/
home/.bash_history
home/.zsh_history
home/.node_repl_history
home/.python_history
```

### IPC Operations

| Operation | IPC Message | Description |
|-----------|-------------|-------------|
| **Clone** | `clone-abot` | Copy an abot bundle (creates a new git repo) |
| **Status** | `abot-git` (op: `status`) | Run `git status --porcelain` on the abot repo |
| **Log** | `abot-git` (op: `log`) | Run `git log --oneline -20` |
| **Diff** | `abot-git` (op: `diff`) | Run `git diff` |

### Version Migration

- **v1** bundles (pre-git) are auto-migrated to v2 on open: git is initialized, manifest version updated
- The `migrate_data_dir` function renames `~/.abot/bundles/` to `~/.abot/abots/` and initializes git in every existing abot

## Git Worktree Model

An abot is like a worker you **employ** into a kubo. The canonical abot (`~/.abot/abots/alice.abot/`) IS the git repo — its identity, history, and growth all live in one place. When employed into a kubo, the abot gets a **git worktree** on a kubo-specific branch.

### How It Works

1. The canonical abot has a default branch (source of truth) and `kubo/<name>` branches for each kubo it's employed in
2. `git worktree add` creates a working copy inside the kubo directory on the kubo-specific branch
3. All terminal I/O writes directly to the worktree — it IS the working copy
4. Autosave commits land on the `kubo/<name>` branch automatically (the worktree shares the canonical repo's `.git` object store)

```
~/.abot/abots/alice.abot/              ← canonical repo (default branch)
  .git/                                 ← owns ALL history and objects
  manifest.json
  home/

~/.abot/kubos/everyday_vet.kubo/       ← NOT a git repo — infrastructure only
  manifest.json                         ← {abots: ["alice", "bob"]}
  credentials.json                      ← kubo-level API keys
  alice/                                ← worktree on branch kubo/everyday_vet
    .git                                ← file (not dir), points to alice.abot/.git
    manifest.json
    home/                               ← bind-mounted into container
```

### Employing an Abot

When adding `alice.abot` to kubo `everyday_vet`:

1. Create branch `kubo/everyday_vet` in the canonical `alice.abot` repo
2. `git worktree add ~/.abot/kubos/everyday_vet.kubo/alice kubo/everyday_vet`
3. The kubo manifest is updated to include `"alice"` in its abots list

The same abot can be employed in multiple kubos — each gets its own branch (`kubo/everyday_vet`, `kubo/ml_lab`, etc.) and worktree. No race conditions.

### Credentials Belong to the Kubo

API keys and tokens live at the kubo level (`everyday_vet.kubo/credentials.json`) and are injected into the container environment. The abot's `.gitignore` excludes `credentials.json`, so credentials never enter version control or travel with shared abots.

Standalone abots (not yet employed in any kubo) can still have their own `credentials.json` for local use. When employed, the kubo's credentials take precedence.

### Autosave

Autosave just commits in the worktree — no push needed. Commits land directly on the `kubo/<name>` branch in the canonical repo because the worktree shares the `.git` object store.

### Update Indicator

To check if the canonical abot has upstream changes: `git log kubo/<name>..<default-branch>` in the canonical repo. If there are commits, show a UI indicator. The user can pull updates on demand. Future: AI-assisted conflict resolution.

### Sharing and Reintegration

- **Sharing a kubo** — copy the kubo directory. Abot files are all present in the worktrees. The receiver imports each abot as a new canonical repo.
- **Reintegration** — merging a `kubo/<name>` branch back into the default branch is a deliberate user action, like a worker bringing experience home. Standard `git merge`.

### Implementation Notes

- `.git` in a worktree is a **file** (not a directory) containing `gitdir: /path/to/alice.abot/.git/worktrees/<name>/`. Code should use `path.join(".git").exists()`, not `.is_dir()`.
- A repo can only have one worktree per branch — enforced by git and by the `kubo/<name>` naming convention.
- Worktrees require the canonical `.git` dir to be accessible on disk.

## Facets

A **facet** is a translucent floating panel — the visual primitive of abot's UI. Facets are drawn on a `<canvas>` with edge glow and depth gradients.

!!! info "Server Knows Nothing About Facets"
    All positioning, focus tracking, z-ordering, and layout happens client-side in the Flutter Web app. The server only knows about sessions. Facets are purely a UI concept.

### Focus-Based Routing

The client tracks which facet has focus and tags outgoing keyboard input with the corresponding session ID. This means:

- Multiple sessions can be visible at once
- Input always goes to the focused facet
- Unfocused facets show live output but don't receive keystrokes

### Layout

- **Focused facet** fills the main area, rendered at full size
- **Unfocused facets** are CSS-transformed into a sidebar strip
- **Drag and resize** to arrange facets spatially on the canvas
- **FLIP animations** for smooth layout transitions

## .abot Bundles

Each `.abot` bundle **is** the container's sandbox. The bundle's `home/` subdirectory is bind-mounted into the Docker container — there's no snapshot/restore cycle.

```
~/.abot/abots/main.abot/        ← canonical git repo (v2)
  .git/                          ← auto-initialized, owns all history
  .gitignore                     ← excludes credentials, scrollback, caches
  manifest.json                  ← name, version 2, timestamps, image
  credentials.json               ← API keys (for standalone use; excluded from git)
  config.json                    ← shell, memory_mb (2048), cpu_percent, env vars
  scrollback                     ← terminal scrollback (persisted across close/reopen)
  home/                          ← bind-mounted as /home/abots/{name}/home in kubo container
```

When employed in a kubo, the abot appears as a git worktree inside the kubo directory. Credentials come from the kubo, not the abot.

### Why Bind-Mount?

Because the home directory is bind-mounted live:

- Every file you create in the terminal is **immediately visible on the host**
- Every file you drop into the host directory is **immediately visible in the container**
- "Save" only writes metadata — the filesystem is always current
- No export/import step, no snapshot delay

### Bundle Files

#### manifest.json

```json
{
  "version": 2,
  "name": "my-project",
  "created_at": "2026-03-01T10:30:00Z",
  "updated_at": "2026-03-01T14:22:00Z",
  "image": "abot-session"
}
```

- `version` — `2` for git-backed bundles (v1 auto-migrated on open)
- `created_at` — preserved across saves (original creation time)
- `updated_at` — updated on every save
- `image` — Docker image to use (defaults to `abot-session`, falls back to `alpine:3`)

#### config.json

```json
{
  "shell": "/bin/bash",
  "memory_mb": 2048,
  "cpu_percent": 50,
  "env": {
    "EDITOR": "vim",
    "MY_VAR": "value"
  }
}
```

- `shell` — shell to spawn in the container (default: `/bin/bash`)
- `memory_mb` — container memory limit (default: 2048)
- `cpu_percent` — CPU limit as percentage of one core (default: 50)
- `env` — custom environment variables (non-credential)

#### credentials.json

```json
{
  "api_key": "sk-ant-...",
  "claude_token": "oauth-token-..."
}
```

- `api_key` — set as both `ANTHROPIC_API_KEY` and `CLAUDE_API_KEY` in the container
- `claude_token` — set as `CLAUDE_CODE_OAUTH_TOKEN` in the container

!!! warning
    `credentials.json` contains sensitive data and is excluded from git via `.gitignore`. The `~/.abot/` directory is created with `0700` permissions (owner-only access).

!!! info "Credentials and kubos"
    When an abot is employed in a kubo, **credentials come from the kubo** (`kubo-name.kubo/credentials.json`), not from the abot. This ensures credentials never travel when abots are shared. Standalone abots can still have their own `credentials.json` for local use.

### Bundle Operations

| Operation | CLI/API | What Happens |
|-----------|---------|-------------|
| **Save** | `POST /sessions/{name}/save` | Writes metadata to existing bundle path, auto-commits git |
| **Save As** | `POST /sessions/{name}/save-as` | Copies entire bundle to new path |
| **Open** | `POST /sessions/open` | Reads bundle, creates session with its config |
| **Close** | `POST /sessions/{name}/close` | Kills container, keeps bundle on disk |
| **Delete** | `DELETE /sessions/{name}` | Kills container, deletes bundle directory |

## Authentication

abot uses **WebAuthn passkeys** — no passwords ever.

### Access Methods

| Method | Detection | Auth Flow |
|--------|-----------|-----------|
| **Localhost** | Socket is loopback + Host header matches | Auto-bypass — no login needed |
| **Remote** | Everything else | Setup token → passkey registration → passkey login |

### Why Localhost Is Trusted

If an attacker has localhost access to your machine, they already have full system access. Adding an auth layer on localhost provides no meaningful security — it just adds friction.

### First-Time Setup (Remote)

1. Generate a setup token from the CLI: `abot token create "My Phone"`
2. On the remote device, enter the setup token
3. Register a WebAuthn passkey (Touch ID, Face ID, security key)
4. Future visits: authenticate with the passkey

### Session Persistence

- **Cookie:** `abot_session` (HttpOnly, SameSite=Lax, Secure on non-localhost)
- **Expiry:** 30 days from creation
- **Auto-refresh:** If idle for 24+ hours, the expiry extends to 30 more days
- **Revocation:** Deleting a credential immediately closes all WebSocket connections for that credential

## Docker Backend

All sessions run inside kubo containers:

| Setting | Value |
|---------|-------|
| **Image** | `abot-kubo` or custom per-kubo Dockerfile (falls back to `alpine:3`) |
| **User** | `1000:1000` (non-root) |
| **Memory** | 2 GB |
| **CPU** | 100% of one core (shared across all abots) |
| **PIDs** | Max 512 processes |
| **Security** | All capabilities dropped, no-new-privileges |
| **Mount** | Kubo directory → `/home/abots/` (read-write bind) |
| **CMD** | `sleep infinity` (long-lived container) |

Sessions are created via `docker exec` into the kubo container, each with its own working directory under `/home/abots/{abot}/home`.

!!! note
    The abot server itself runs directly on the host as a single binary — no Docker needed to start it. Docker is only required when you create a session. If Docker isn't running, session creation returns a clear error message.
