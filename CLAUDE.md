# abot — *intelligence within reach*

A spatial interface between human and computer intelligence, rendered on canvas, served by a Rust binary.

## Architecture

- **Rust binary** (`src/`): daemon (PTY session owner) + server (HTTP/WS), single binary with subcommands
- **Browser client** (`client/`): vanilla JS canvas rendering, facet-based spatial UI
- **Assets** embedded in binary via rust-embed for single-binary distribution

### Daemon/Server Split

- `abot start` — launches both (daemon first, server 500ms later)
- `abot daemon` — PTY session owner, Unix socket IPC (NDJSON)
- `abot serve` — HTTP/WS server, connects to daemon
- `abot update` — rolling update: drain server, swap binary, restart

### Module Layout

```
src/daemon/     PTY sessions, ring buffer, NDJSON IPC
src/server/     HTTP routes, asset serving, daemon client
src/auth/       WebAuthn, sessions, setup tokens, lockout, middleware
src/stream/     WebSocket handler, client tracking, message protocol
```

## Terminology

- **Facet** — a translucent floating panel (the visual primitive). Drawn on canvas with edge glow, depth gradients. The server knows nothing about facets — all positioning/focus/z-order is client-side.
- **Session** — a server-side resource (PTY process). Only session I/O crosses the wire.

## Key Patterns

- Passkey auth (WebAuthn) — no passwords
- Session persistence across restarts (daemon survives server restarts)
- Rolling updates with client reconnection
- Touch-first design
- Localhost auto-auth bypass
- Focus-based routing: client tracks which facet has focus, tags outgoing input with session ID

## Conventions

- Rust: `axum` patterns, `tracing` for logging, `anyhow`/`thiserror` for errors
- Client JS: vanilla JS, no framework, canvas-rendered everything
- All rendering on `<canvas>` — DOM only for xterm.js, IME input, clipboard
- Sessions are the core abstraction (not files)
- The UI term is "facet" (not panel, window, or plate)

## Development

```
cargo run -- start    # Start daemon + server
cargo run -- serve    # Server only (daemon must be running)
cargo test            # Run tests
```
