# Intelligence Within Reach

A spatial interface between human and computer intelligence, rendered on canvas, served by a single Rust binary.

*abot* is a touch-first terminal environment where each session lives in its own Docker sandbox. Translucent floating panels called **facets** give you a spatial workspace — drag, resize, and focus sessions the way you think about them.

```bash
abot start
# → open http://localhost:6969
```

[Get Started](getting-started.md){ .md-button .md-button--primary }
[View on GitHub](https://github.com/Dorky-Robot/abot){ .md-button }

---

| | | |
|:---:|:---:|:---:|
| **Spatial** | **Sandboxed** | **Passwordless** |
| Canvas UI with facets | Docker containers per session | WebAuthn passkey auth |

## How It Works

1. **Start abot** — a single binary launches a daemon (PTY owner) and an HTTP/WebSocket server
2. **Open in browser** — a Flutter Web (WASM) canvas renders your workspace
3. **Create sessions** — each session gets its own Docker container with a persistent home directory
4. **Arrange facets** — drag, resize, and focus translucent panels on a spatial canvas

## Why abot

Traditional terminal multiplexers (tmux, screen) give you session persistence but no spatial awareness. IDE integrated terminals give you spatial layout but no isolation. abot combines both:

- **Spatial canvas** — arrange terminals in 2D space, not tabs or panes
- **Container isolation** — each session is sandboxed in its own Docker container
- **Touch-first** — designed for tablets and touch screens, works great with mouse and keyboard
- **Single binary** — daemon + server + embedded Flutter client, nothing else to install
- **Passwordless** — WebAuthn passkeys, no passwords ever
- **Session persistence** — the daemon survives server restarts; rolling updates don't drop sessions

## Architecture at a Glance

```
Browser (Flutter WASM) ──WebSocket──→ Server (HTTP/WS) ──Unix Socket──→ Daemon
                                      abot serve           NDJSON        abot daemon
                                      stateless                          PTY sessions
                                                                         Docker containers
                                                                         ring buffers
```

The daemon owns all sessions. The server is stateless — restart it freely, your sessions survive. The browser reconnects and the daemon replays the output buffer. You pick up exactly where you left off.

## Quick Links

| Page | What You'll Learn |
|------|-------------------|
| [Getting Started](getting-started.md) | Install, first launch, register a passkey |
| [Features](features.md) | Everything abot can do |
| [Concepts](concepts.md) | Sessions, facets, bundles, and the sandbox model |
| [Architecture](architecture.md) | Daemon/server split, Docker backend, Flutter client |
| [CLI Reference](cli-reference.md) | All commands and flags |
| [Configuration](configuration.md) | Config files, env vars, data directory |
| [Security](security.md) | Auth model, CSRF, lockout, container hardening |
| [API Reference](api-reference.md) | REST endpoints, WebSocket protocol, IPC |
