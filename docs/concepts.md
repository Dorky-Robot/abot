# Concepts

## Sessions

A **session** is the core abstraction in abot. Each session is a server-side resource — a PTY process running inside a Docker container (or a local PTY if Docker is unavailable). Only terminal I/O crosses the wire between client and server.

Sessions persist across server restarts because the **daemon** owns them independently. You can restart, update, or crash the server and every session keeps running.

### Session Lifecycle

| Action | What Happens |
|--------|-------------|
| **Create** | Spins up a Docker container, creates `~/.abot/bundles/{name}.abot/home/` |
| **Attach** | Client joins session, receives ring buffer snapshot (output history) |
| **Input** | Keystrokes routed to the session's PTY stdin |
| **Output** | PTY stdout broadcast to all attached clients |
| **Resize** | Terminal dimensions updated (TIOCSWINSZ) |
| **Detach** | Client leaves session — session keeps running |
| **Close** | Kills the container; bundle directory stays for reopening later |
| **Delete** | Kills the container AND deletes the bundle directory |
| **Save** | Writes metadata files (manifest, config, credentials) — the filesystem is always live |
| **Save As** | Copies the entire bundle directory to a new path |
| **Rename** | Updates the session name in-place |
| **Open** | Reads an existing `.abot` bundle and creates a session from it |

### Multi-Client Attach

Multiple clients can attach to the same session simultaneously. All clients see the same output in real time — useful for pair programming or monitoring. Input from any client goes to the same PTY.

### Auto-Save

Dirty sessions (those with unsaved environment changes) are automatically saved every **5 minutes**. The autosave only writes metadata — the home directory is bind-mounted live, so filesystem changes are always persisted.

### Session Environment

Each session gets a base environment:

| Variable | Value |
|----------|-------|
| `TERM` | `xterm-256color` |
| `COLORTERM` | `truecolor` |
| `LANG` | `en_US.UTF-8` |

Additional environment variables can be injected per-session (via bundle config) or globally (via the Anthropic API key feature, which pushes `ANTHROPIC_API_KEY` and `CLAUDE_API_KEY` to all running sessions).

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

Each `.abot` bundle **is** the container's sandbox. The bundle's `home/` subdirectory is bind-mounted as `/home/dev` in the Docker container — there's no snapshot/restore cycle.

```
~/.abot/bundles/main.abot/
  manifest.json      ← name, version, timestamps, image
  credentials.json   ← API keys (ANTHROPIC_API_KEY, CLAUDE_CODE_OAUTH_TOKEN)
  config.json        ← shell, memory_mb, cpu_percent, env vars
  home/              ← bind-mounted as /home/dev in container
```

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
  "version": 1,
  "name": "my-project",
  "created_at": "2025-06-15T10:30:00Z",
  "updated_at": "2025-06-15T14:22:00Z",
  "image": "abot-session"
}
```

- `version` — always `1` (for future format changes)
- `created_at` — preserved across saves (original creation time)
- `updated_at` — updated on every save
- `image` — Docker image to use (defaults to `abot-session`, falls back to `alpine:3`)

#### config.json

```json
{
  "shell": "/bin/bash",
  "memory_mb": 512,
  "cpu_percent": 50,
  "env": {
    "EDITOR": "vim",
    "MY_VAR": "value"
  }
}
```

- `shell` — shell to spawn in the container (default: `/bin/bash`)
- `memory_mb` — container memory limit (default: 512)
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
    `credentials.json` contains sensitive data. The `~/.abot/` directory is created with `0700` permissions (owner-only access).

### Bundle Operations

| Operation | CLI/API | What Happens |
|-----------|---------|-------------|
| **Save** | `POST /sessions/{name}/save` | Writes metadata to existing bundle path |
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

When Docker is available, each session runs in an isolated container:

| Setting | Value |
|---------|-------|
| **Image** | `abot-session` (falls back to `alpine:3`) |
| **User** | `1000:1000` (non-root) |
| **Memory** | 512 MB |
| **CPU** | 50% of one core |
| **PIDs** | Max 256 processes |
| **Security** | All capabilities dropped, no-new-privileges |
| **Home** | Bind-mounted from `~/.abot/bundles/{name}.abot/home/` |

When Docker is unavailable, abot falls back to **local PTY** sessions using the `portable-pty` crate — same session management, but without container isolation.
