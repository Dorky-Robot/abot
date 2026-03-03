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

## Session Sandbox Model

Each `.abot` bundle IS the container's sandbox. The bundle's `home/` subdirectory is bind-mounted as `/home/dev` in the Docker container — no snapshot/restore cycle.

```
~/.abot/bundles/main.abot/
  manifest.json      ← name, version, timestamps
  credentials.json   ← API keys
  config.json        ← shell, env vars
  home/              ← bind-mounted as /home/dev in container
```

- **Create session** → auto-creates `~/.abot/bundles/{name}.abot/home/`
- **Terminal I/O** → writes directly to bind-mounted `home/` (live)
- **Save** → writes metadata files only (filesystem is always live)
- **Save As** → copies entire bundle directory to new path
- **Delete** → kills container + deletes bundle directory
- **Close** → kills container, bundle directory stays for reopening

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
