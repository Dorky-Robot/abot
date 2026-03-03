# Architecture

```
Browser (Flutter WASM) ──WebSocket──┐
                                     ├── Server (HTTP/WS) ──Unix Socket──  Daemon
Tablet browser ─────────WebSocket──┘    (abot serve)         (NDJSON)     (abot daemon)
                                                                           PTY sessions
                                                                           Docker containers
                                                                           Ring buffers
```

## Daemon / Server Split

abot is a single Rust binary with two main runtime modes that communicate over a Unix domain socket:

| Component | Command | Responsibility | Lifetime |
|-----------|---------|---------------|----------|
| **Daemon** | `abot daemon` | Owns PTY sessions, manages Docker containers, ring buffers for output replay | Long-lived — survives server restarts |
| **Server** | `abot serve` | HTTP routes, WebSocket handler, asset serving, WebAuthn auth | Stateless — can be restarted freely |
| **Both** | `abot start` | Spawns daemon as detached process (with `setsid`), waits for socket, runs server in foreground | Normal way to run abot |

This separation is the key architectural decision. Because the daemon owns all sessions, you can:

- **Restart the server** without losing any terminal sessions
- **Rolling update** (`abot update`) — swap the binary, restart the server, clients reconnect automatically
- **Crash recovery** — a supervisor task checks the daemon every 5 seconds and restarts it if it dies

## Module Layout

```
src/
  main.rs             CLI entrypoint, subcommand dispatch, config loading
  error.rs            Error types (NotFound, Unauthorized, Forbidden, BadRequest,
                      LockedOut, Internal, Anyhow, Db, Json) → HTTP status codes
  pid.rs              PID file management, process liveness, abot process verification

  daemon/
    mod.rs            Daemon main loop, Unix socket listener, connection handlers
    session.rs        Session state (name, alive, exit_code, dirty, bundle_path)
    ring_buffer.rs    FIFO buffer (5000 items / 5 MB) for output replay
    pty.rs            Local PTY backend (portable-pty)
    docker.rs         Docker container backend (bollard)
    bundle.rs         .abot bundle format (read/write manifest, config, credentials)
    ipc.rs            NDJSON request/response types for daemon IPC

  server/
    mod.rs            Axum router, middleware stack, state initialization
    assets.rs         rust-embed asset serving, CSRF token injection, cache headers
    browse.rs         File browser API (directory listing, path expansion)
    config.rs         Instance config (config.json) read/write
    anthropic_oauth.rs  Anthropic API key storage and push to daemon

  auth/
    mod.rs            WebAuthn configuration, registration/login handlers
    state.rs          SQLite schema, CRUD for users/credentials/sessions/tokens
    tokens.rs         Setup token generation (32 bytes hex), argon2 hashing
    lockout.rs        Brute-force protection (5 attempts / 15 min → 15 min lockout)
    challenge.rs      Challenge store (in-memory, 5 min TTL, single-use)
    middleware.rs      Auth middleware, CSRF validation, localhost detection

  stream/
    mod.rs            WebSocket upgrade handler, message dispatch
    messages.rs       Client/server message types (attach, input, resize, detach, p2p)
    clients.rs        Client tracker (sessions, P2P channels, credential IDs)
    p2p.rs            WebRTC peer connection, DataChannel, ICE signaling
```

## Startup Sequence

### `abot start`

```
1. Check daemon.pid → is daemon already running?
2. If not: spawn daemon as detached process (setsid)
3. Wait for daemon.sock to appear (5s timeout: 50 × 100ms)
4. Start supervisor task (checks daemon every 5s, restarts if dead)
5. Start server in foreground
```

### Server Startup

```
1. Initialize SQLite database (create tables if needed)
2. Connect to daemon Unix socket (~/.abot/daemon.sock)
3. Build AppState (auth state, daemon client, client tracker)
4. Push stored Anthropic API key to daemon (if exists)
5. Bind TCP listener on configured address
6. Write server.pid
7. Serve requests
```

### Daemon Startup

```
1. Check for stale daemon.pid → remove if process is dead
2. Detect backend: Docker socket exists? → DockerBackend : LocalPTY
3. Start autosave loop (every 5 min for dirty sessions)
4. Listen on Unix socket (daemon.sock, mode 0o600)
5. Spawn handler task per connection
```

## IPC Protocol

The daemon and server communicate over a **Unix domain socket** using **NDJSON** (newline-delimited JSON).

### RPC vs Fire-and-Forget

- **RPC requests** include an `"id"` field — the daemon responds with the same `"id"`
- **Fire-and-forget** messages (input, resize) have no `"id"` — no response expected
- **Broadcast events** from daemon (output, exit, session-removed) have no `"id"` — pushed to all server connections

### Message Flow

```
Server → Daemon:  {"type":"create-session","id":"abc","name":"main","cols":120,"rows":40}
Daemon → Server:  {"id":"abc","name":"main"}

Server → Daemon:  {"type":"input","clientId":"xyz","session":"main","data":"ls\n"}
(no response — fire-and-forget)

Daemon → Server:  {"type":"output","session":"main","data":"file1.txt\nfile2.txt\n"}
(broadcast — no id)
```

See [API Reference](api-reference.md) for the complete IPC message catalog.

## Docker Backend

When the Docker socket (`/var/run/docker.sock`) is available and the `docker` feature is enabled:

### Image Selection

1. Check for `abot-session` image → use if available
2. Fall back to `alpine:3` → pull if needed

### Container Configuration

| Setting | Value |
|---------|-------|
| TTY | Enabled |
| User | `1000:1000` (non-root) |
| Memory | 512 MB (configurable per bundle) |
| CPU | 50% of one core (100ms period, 50ms quota) |
| PIDs | Max 256 processes |
| Capabilities | All dropped |
| Security | `no-new-privileges` |
| Home mount | `{bundle}/home/` → `/home/dev` (read-write bind) |

### Container Lifecycle

```
1. Remove any stale container with the same name
2. Create container with config above
3. Start container
4. Resize TTY to requested cols/rows
5. Attach stdin/stdout/stderr
6. Spawn reader task → relay output to daemon → broadcast to clients
```

### Local PTY Fallback

When Docker is unavailable, abot uses `portable-pty` to spawn shells directly on the host. Same session management, same ring buffers, same WebSocket protocol — just no container isolation.

## Flutter Web Client

The client is a Flutter Web app compiled to **WASM** and embedded in the binary via `rust-embed`.

### Rendering Model

Everything renders on `<canvas>`. The DOM is only used for:

- **xterm.js** — terminal emulator via `HtmlElementView`
- **IME input** — international keyboard support
- **Clipboard** — cut/copy/paste

### State Management

[Riverpod](https://riverpod.dev/) for reactive state. Key providers manage:

- Session list and active session
- Facet layout and focus state
- WebSocket connection and reconnection
- Auth state and CSRF tokens

### Key Client Features

- **Facet tiling** — multi-facet layout with FLIP animations
- **Drag and resize** — gesture-based facet management
- **Sidebar** — unfocused terminals CSS-transformed to sidebar strip
- **Search bar** — filter and launch sessions
- **Keyboard shortcuts** — Cmd→Ctrl translation on macOS
- **Touch-first** — all interactions designed for touch screens

### Keyboard Handling

On macOS, `Cmd` is translated to `Ctrl` for terminal shortcuts (Cmd+C → Ctrl+C), except for browser-reserved combinations: Cmd+C/V/A/X/Z (clipboard), Cmd+R/L/T/Q (browser controls).

## Rolling Update

`abot update` performs a zero-downtime server restart:

```
1. Check daemon is running (fall back to full start if not)
2. Read server.pid
3. Verify PID is actually an abot process (prevents signaling wrong process)
4. Send SIGTERM to old server
5. Old server broadcasts "server-draining" to all WebSocket clients
6. Wait for graceful shutdown (100ms intervals, 10s timeout)
7. If not dead: send SIGKILL
8. Start new server
9. Clients reconnect, daemon replays ring buffers
```

## Graceful Shutdown

When the server receives SIGTERM:

1. Broadcast `server-draining` message to all WebSocket clients
2. Wait 500ms (give clients time to prepare for reconnection)
3. Exit

The daemon continues running independently. When the new server starts, clients reconnect and receive ring buffer replays.
