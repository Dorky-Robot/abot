# abot — *intelligence within reach*

A spatial interface between human and computer intelligence, rendered on canvas, served by a Rust binary.

## Architecture

- **Rust binary** (`src/`): daemon (PTY session owner) + server (HTTP/WS), single binary with subcommands
- **Flutter client** (`flutter_client/`): Flutter Web (WASM) canvas rendering, facet-based spatial UI
- **Assets** embedded in binary via rust-embed for single-binary distribution

### Daemon/Server Split

- `abot start` — launches both (daemon first, server 500ms later)
- `abot daemon` — PTY session owner, Unix socket IPC (NDJSON)
- `abot serve` — HTTP/WS server, connects to daemon
- `abot update` — rolling update: drain server, swap binary, restart

### Module Layout

```
src/daemon/     Sessions, ring buffer, NDJSON IPC, Docker + kubo backends, bundle/git ops
src/server/     HTTP routes, asset serving, daemon client
src/auth/       WebAuthn, sessions, setup tokens, lockout, middleware
src/stream/     WebSocket handler, client tracking, message protocol
```

## Terminology

- **Facet** — a translucent floating panel (the visual primitive). Drawn on canvas with edge glow, depth gradients. The server knows nothing about facets — all positioning/focus/z-order is client-side.
- **Session** — a server-side resource (PTY process). Only session I/O crosses the wire.
- **Kubo** — a shared runtime room. One Docker container hosting multiple abots via `docker exec`. Sessions inside a kubo share the container's tools and resources.
- **Abot** — a `.abot` bundle directory, now a git repo (v2). Each abot's `home/` is bind-mounted into a container.

## Key Patterns

- **Zero-dependency install** — the binary runs on the host with no prerequisites. Docker is only needed at session creation, not at startup. A setup wizard guides non-technical users through provisioning.
- Passkey auth (WebAuthn) — no passwords
- Session persistence across restarts (daemon survives server restarts)
- Rolling updates with client reconnection
- Touch-first design
- Localhost auto-auth bypass
- Focus-based routing: client tracks which facet has focus, tags outgoing input with session ID
- **Kubos** — shared runtime rooms: one container, multiple abots via `docker exec`, idle timeout (5 min)
- **Abots as git repos** — each `.abot` bundle auto-initialized as git repo (v2), auto-committed on save

## Session Sandbox Model

Every session runs inside a **kubo** — a shared Docker container hosting one or more abots.

### Abot (git-backed bundle)

```
~/.abot/abots/main.abot/        ← canonical git repo (v2)
  .git/                          ← auto-initialized, owns all history
  .gitignore                     ← excludes credentials, scrollback, caches
  manifest.json                  ← name, version 2, timestamps
  credentials.json               ← API keys (standalone use; excluded from git)
  config.json                    ← shell, env vars
  home/                          ← bind-mounted into kubo container
```

### Kubo (Docker container)

```
~/.abot/kubos/default.kubo/     ← NOT a git repo — infrastructure only
  manifest.json                  ← name, version, abots list
  credentials.json               ← kubo-level API keys
  Dockerfile                     ← optional custom image
  alice/                         ← git worktree of alice.abot on branch kubo/default
    .git                         ← file (not dir), points to alice.abot/.git
    home/                        ← bind-mounted as /home/abots/alice/home
  bob/                           ← git worktree of bob.abot on branch kubo/default
    .git
    home/
```

- Container specs: 2 GB memory, 100% CPU, 512 PIDs, `sleep infinity`, `/home/abots/` mount
- Sessions use `docker exec` into the running kubo container
- Idle timeout: 5 min with no active sessions → container stops

### Git Worktree Model

An abot is like a worker you **employ** into a kubo. The abot IS the git repo —
its identity, history, and growth all live in one place. When employed into a
kubo, the abot gets a **worktree** on a kubo-specific branch, like the worker
going to that office. The kubo itself is not a git repo — it's infrastructure
(container config + credentials + manifest).

#### Branch-per-kubo pattern

Each canonical abot (`~/.abot/abots/alice.abot/`) has:
- A default branch (whatever `init.defaultBranch` is configured to) — the source-of-truth identity
- `kubo/<kubo-name>` — one branch per kubo the abot is employed in

```
alice.abot/                          ← canonical git repo
  branches:
    main (or system default)         ← source of truth
    kubo/everyday_vet                ← work context for everyday_vet kubo
    kubo/ml_lab                      ← work context for ml_lab kubo
```

All branches, worktrees, and history are visible with standard git commands:
- `git branch --list 'kubo/*'` — see everywhere the abot is employed
- `git worktree list` — see all active work locations
- `git log kubo/everyday_vet..HEAD` — check for upstream updates (from default branch)

#### Credentials belong to the kubo, not the abot

API keys and tokens live at the kubo level (`everyday_vet.kubo/credentials.json`)
and are injected into the container environment. `credentials.json` is excluded
by the abot's `.gitignore`, so it never enters version control. When an abot is
shared or moved between kubos, no credentials travel with it.

Standalone abots (not employed in any kubo) can still have their own
`credentials.json` for local use. When employed, the kubo's credentials take
precedence.

#### Employing an abot into a kubo

When adding `alice.abot` to kubo `everyday_vet`:

```bash
cd ~/.abot/abots/alice.abot
git branch kubo/everyday_vet
git worktree add ~/.abot/kubos/everyday_vet.kubo/alice kubo/everyday_vet
```

On disk:
```
~/.abot/abots/alice.abot/              ← canonical repo (default branch)
  .git/                                 ← owns ALL history and objects
  manifest.json
  home/

~/.abot/kubos/everyday_vet.kubo/       ← NOT a git repo — infrastructure only
  manifest.json                         ← {abots: ["alice", "bob"]}
  credentials.json                      ← kubo-level API keys
  Dockerfile                            ← optional custom image
  alice/                                ← worktree on branch kubo/everyday_vet
    .git                                ← file (not dir), points to alice.abot/.git
    manifest.json
    home/                               ← bind-mounted into container
  bob/                                  ← worktree on branch kubo/everyday_vet
    home/
```

The worktree IS the working copy — all terminal I/O writes directly there.
The bind mount is unchanged: the entire kubo dir mounts as `/home/abots/`, so
`alice/home/` appears at `/home/abots/alice/home/` in the container.

#### Autosave flow

Autosave just commits in the worktree — no push needed. The commits land
directly on the `kubo/everyday_vet` branch in the canonical abot repo because
the worktree shares the `.git` object store.

```bash
cd ~/.abot/kubos/everyday_vet.kubo/alice
git add -A && git commit -m "autosave 2026-03-04 12:00:00 UTC"
# ↑ commits to kubo/everyday_vet branch in alice.abot
```

#### Variant lifecycle

A variant is a `kubo/<name>` branch in an abot's git repo — one per kubo the
abot has been employed in. Variants are either employed (worktree exists) or
past work (branch exists, no worktree). The UI shows only variants that still
need a decision.

| Action | What happens | Result |
|--------|-------------|--------|
| **Employ** | `git branch kubo/<name>` + `git worktree add` | Variant created, abot working in kubo |
| **Dismiss** | Remove from kubo manifest (worktree kept, branch kept) | Variant becomes "past work" |
| **Integrate** | Remove worktree if any, `git merge <branch>` into default, `git branch -d` | Variant absorbed into core abot, branch gone |
| **Discard** | Remove worktree if any, `git branch -D` | Variant deleted, branch gone |

After integrate or discard, the branch is gone — no sub-item in the UI.

#### Implementation notes

- `.git` in a worktree is a **file** (not a directory) containing
  `gitdir: /path/to/alice.abot/.git/worktrees/<name>/`. Existing
  `path.join(".git").exists()` checks still work, but `.is_dir()` will not.
- A repo can only have one worktree per branch — enforced by git and by our
  `kubo/<name>` branch naming (each kubo gets a unique branch).
- Worktrees require the canonical `.git` dir to be accessible on disk. Remote
  or network-shared abots would need a different strategy (future).

### Operations

- **Create session** → auto-creates `~/.abot/abots/{name}.abot/home/`, inits git
- **Terminal I/O** → writes directly to bind-mounted `home/` (live)
- **Save** → writes metadata + auto-commits git repo (`autosave {timestamp}`)
- **Save As** → copies entire bundle directory to new path
- **Delete** → kills container + deletes bundle directory
- **Close** → kills container, bundle directory stays for reopening

## UX Principles

### Teach through interaction

Abot, kubo, facet — these are new words. Users learn them by using them, not by
reading docs. The UI should make the meaning obvious through context and
interaction patterns.

### Progressive disclosure

First launch: the user sees a terminal and can start typing. No kubo management,
no abot configuration, no git concepts. Power features (multiple kubos,
employing abots, credentials, updates) surface when the user reaches for them.

### Empty kubo = onboarding surface

When a kubo has no abots, the main stage shows an **explanatory landing page**
(not a blank screen) with:
- What a kubo is (in plain language)
- An "Add abot" button (create new or employ existing)
- An "Open abot" button (from a `.abot` bundle on disk)

This replaces the generic `session-N` auto-creation flow. The user names their
abot intentionally.

### Session names avoid git collision

Auto-generated session names must NOT use `main` — it collides with git's
default branch. Use a different naming convention (e.g. `abot-1`, or prompt
the user for a name).

### Abot visual identity

Each abot should have a consistent visual identity (future: avatar, color, icon)
that persists across kubos. When alice appears in `everyday_vet` and `ml_lab`,
the user should immediately recognize her as the same worker.

### Credentials per kubo are intentional

Different kubos can be backends for different clients/sites. A user running a
one-person consultancy needs separate API keys per kubo. This is a feature, not
friction. The UI should make credential setup per-kubo feel natural, not like a
missing default.

### Variant actions speak for themselves

"Integrate" means bring the work home. "Discard" means throw it away. "Dismiss"
means stop working but keep the history. These are the only actions on variants —
no "merged" badges, no "catch up" buttons, no "N new" counts. The user sees what
exists and decides what to do with it.

## Conventions

- Rust: `axum` patterns, `tracing` for logging, `anyhow`/`thiserror` for errors
- Client: Flutter Web (WASM), Riverpod state management, xterm.js via HtmlElementView
- All rendering on `<canvas>` — DOM only for xterm.js, IME input, clipboard
- Sessions are the core abstraction (not files)
- The UI term is "facet" (not panel, window, or plate)

## Development

```
cd flutter_client && flutter build web --wasm   # Build Flutter client
cargo run -- start                               # Start daemon + server
cargo run -- serve                               # Server only (daemon must be running)
cargo test                                       # Run Rust tests
npx playwright test                              # Run e2e tests (server must be running)
```
