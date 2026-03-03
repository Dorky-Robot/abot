# Intelligence Within Reach

A spatial interface between human and computer intelligence, rendered on canvas, served by a single Rust binary.

*abot* is a touch-first terminal environment where each session lives in its own Docker sandbox. Translucent floating panels called **facets** give you a spatial workspace — drag, resize, and focus sessions the way you think about them.

```bash
abot start
```

[Get Started](getting-started.md){ .md-button .md-button--primary }
[View on GitHub](https://github.com/Dorky-Robot/abot){ .md-button }

---

| | | |
|:---:|:---:|:---:|
| **Spatial** | **Sandboxed** | **Passwordless** |
| Canvas UI with facets | Docker containers per session | WebAuthn passkey auth |

## How It Works

1. **Start abot** — a single binary launches both a daemon (PTY owner) and an HTTP/WebSocket server
2. **Open in browser** — a Flutter Web (WASM) canvas renders your workspace
3. **Create sessions** — each session gets its own Docker container with a persistent home directory
4. **Arrange facets** — drag, resize, and focus translucent panels on a spatial canvas

## Key Features

- **Single binary** — daemon + server + embedded Flutter client, nothing else to install
- **Session persistence** — the daemon survives server restarts; your sessions keep running
- **Rolling updates** — swap the binary and restart without dropping sessions
- **Touch-first** — designed for tablets and touch screens, works great with mouse and keyboard too
- **Passkey auth** — WebAuthn with localhost auto-bypass, no passwords ever
- **.abot bundles** — portable session snapshots with home directory, config, and credentials
