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

abot is a single Rust binary with two main modes:

| Component | Command | Responsibility |
|-----------|---------|---------------|
| **Daemon** | `abot daemon` | Owns PTY sessions, manages Docker containers, ring buffers for output replay. Long-lived — survives server restarts. |
| **Server** | `abot serve` | HTTP routes, WebSocket handler, asset serving, WebAuthn auth. Stateless — can be restarted freely. |
| **Both** | `abot start` | Spawns the daemon as a detached process, waits for socket, then runs the server in foreground. |

The daemon and server communicate over a **Unix domain socket** using **NDJSON** (newline-delimited JSON). This separation means you can restart the server (for updates, config changes, etc.) without losing any terminal sessions.

## Module Layout

```
src/
  main.rs           CLI entrypoint, subcommand dispatch
  daemon/           PTY sessions, ring buffer, NDJSON IPC
  server/           HTTP routes, asset serving, daemon client
  auth/             WebAuthn, sessions, setup tokens, lockout, middleware
  stream/           WebSocket handler, client tracking, message protocol
  pid.rs            PID file management
  error.rs          Error types
```

## Docker Backend

When built with the `docker` feature (`--features docker`), sessions run inside Docker containers instead of local PTY processes. Each session's `.abot` bundle `home/` directory is bind-mounted as `/home/dev` in the container.

This provides:

- **Isolation** — each session is sandboxed in its own container
- **Persistence** — the home directory survives container restarts
- **Portability** — bundles can be moved between machines

## Flutter Web Client

The client is a Flutter Web app compiled to WASM and embedded in the binary via `rust-embed`. It renders entirely on `<canvas>` — the DOM is only used for:

- **xterm.js** terminal emulator (via `HtmlElementView`)
- **IME input** for international keyboards
- **Clipboard** access

State management uses Riverpod. The client communicates with the server over WebSocket for real-time terminal I/O and REST for session management.

## WebSocket Protocol

The WebSocket connection carries terminal I/O between the client and server:

- **Client → Server**: keyboard input tagged with session ID
- **Server → Client**: terminal output, session events (create, close, resize)

On reconnection, the server replays buffered output from the daemon's ring buffer so the client picks up exactly where it left off.

## Authentication Flow

1. **Localhost** — auto-bypass, no auth needed
2. **Remote — first time** — create a setup token via `abot token create`, use it to register a WebAuthn passkey
3. **Remote — returning** — authenticate with your registered passkey

All auth state is stored in a local SQLite database at `~/.abot/abot.db`.
