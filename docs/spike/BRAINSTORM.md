# abot

*intelligence within reach*

A spatial interface between human intelligence and computer intelligence.

## The Core Idea

The terminal is the current interface between humans and AI. It works, but it constrains both sides. The human is limited to text. The AI is limited to text. When Claude Code wants to show you a diagram, it writes ASCII art. When you want to show Claude a layout, you describe it in words. The interface is a bottleneck.

abot is a space where that bottleneck goes away. It's a surface where:

- The AI can render anything: diagrams, previews, interactive prototypes, spatial layouts
- The human can draw, type, speak, gesture, drag
- The interface can reshape itself mid-conversation
- New interaction patterns can be invented on the fly

The terminal isn't gone. It's one **mode** the surface can take. But the surface can also be a whiteboard, a conversation, a visualization, a multi-agent workspace, or something that doesn't have a name yet.

We render on canvas (WebGL/Canvas2D) rather than HTML/CSS so we're not limited by the DOM when we need shaders, non-standard layouts, or UI patterns that haven't been invented yet. It's a pragmatic choice, not the identity — the identity is the space between human and machine.

## What Is This?

It's a **shared space** where human and machine intelligence meet.

Think of it less like an operating system and more like a **studio** — a place where you and AI collaborators work together on whatever you're working on. The space adapts to the work, not the other way around.

Some moments it might look like:
- A terminal (when you're running commands)
- A conversation (when you're thinking through a problem)
- A canvas (when you're designing something visual)
- A dashboard (when you're monitoring systems)
- Multiple of these at once, spatially arranged
- Something entirely new that emerges from the collaboration

The key shift: **the interface is not fixed**. It's a medium, not a product. The product is what you make with it.

### Sessions, Not Apps

The core abstraction isn't "apps" or "windows." It's **sessions** — persistent contexts where work happens.

A session is:
- A running context (terminal, conversation, canvas, or something custom)
- Persistent across disconnects and restarts
- Isolated in Docker containers
- Shareable across devices
- Aware of who's in it (human, AI, or both)

Sessions can be **composed** spatially. Put a terminal session next to a chat session, and they can share context. The spatial arrangement IS the interface — you're not clicking through tabs, you're seeing everything at once and moving things around as your focus shifts.

### What We Carry From Katulong

Katulong proved the model: passkey auth, daemon-owned sessions, rolling updates, touch-first design. abot takes those patterns and expands the surface area:

- Katulong's terminal becomes one session type in abot
- Katulong's daemon/server split becomes abot's session engine
- Katulong's auth flow carries over directly (passkeys, device pairing, setup tokens)
- Katulong's rolling update mechanism applies to the Rust binary
- Touch-first: virtual keyboard handling, gesture support, mobile-optimized input

---

## Architecture

### High-Level

```
┌─────────────────────────────────────────────────────────┐
│                    Browser (any device)                   │
│                                                           │
│  ┌─────────────────────────────────────────────────────┐ │
│  │                   Client                             │ │
│  │                                                       │ │
│  │   Rendering (canvas — WebGL/2D as needed)            │ │
│  │   Spatial compositor (sessions arranged in space)     │ │
│  │   Input (keyboard, mouse, touch, gestures, IME)      │ │
│  │   Session protocol (WebSocket, reconnection)         │ │
│  └──────────────────────┬──────────────────────────────┘ │
│                         │ WebSocket + HTTP                │
└─────────────────────────┼─────────────────────────────────┘
                          │
┌─────────────────────────┼─────────────────────────────────┐
│                    Rust Binary (abot)                       │
│                                                             │
│  Asset server (client embedded in binary)                   │
│  Session engine (lifecycle, persistence, I/O relay)         │
│  Auth (WebAuthn passkeys, from katulong)                    │
│  Runtime (Docker containers, PTY, resource management)      │
└─────────────────────────────────────────────────────────────┘
                          │
              ┌───────────┴───────────┐
              │  Isolated Containers   │
              │  (sessions run here)   │
              └───────────────────────┘
```

### Rust Server

Single binary. Embeds the client assets. Self-provisions.

**What it does:**
- Serves the client (embedded via `rust-embed`)
- WebAuthn passkey authentication (from katulong)
- Session lifecycle (create, persist, restore, destroy)
- WebSocket relay for session I/O
- Docker container orchestration (via `bollard`)
- Self-update

**Boot sequence:**
```
$ abot
  → Initialize data directory (~/.abot/)
  → Start daemon + server
  → Open browser
  → Ready.
```

No config needed. The binary runs on the host with zero dependencies. Docker is only checked when a session is created — if it's missing, the UI guides you through setup.

**Key crates:**
- `axum` — HTTP + WebSocket
- `tokio` — Async runtime
- `bollard` — Docker API (pure Rust, no Docker CLI needed)
- `webauthn-rs` — Passkey auth
- `rust-embed` — Embed client in binary
- `rusqlite` — Session state (SQLite, embedded)
- `tracing` — Structured logging
- `clap` — CLI

### Client

The client renders to a `<canvas>` element with a minimal hidden DOM layer for things the canvas can't handle natively (IME text input, clipboard, accessibility). This is the same pattern Figma, VS Code, and Google Docs use.

We use canvas so we have access to shaders, custom rendering, and non-standard UI when we need it — but the first version can be straightforward Canvas2D drawing (rects, text, images). We graduate to WebGL/shaders as specific features demand it.

For the terminal specifically: render xterm.js to an OffscreenCanvas and composite it into our surface. Battle-tested terminal emulation without reinventing it.

### Session Engine

Sessions are the heart. Everything is a session.

```rust
struct Session {
    id: Uuid,
    name: String,
    kind: SessionKind,
    state: SessionState,
    created_at: DateTime<Utc>,
    last_active: DateTime<Utc>,
    container_id: Option<String>,
    buffer: RingBuffer,
    connected_clients: Vec<ClientId>,
}

enum SessionKind {
    Terminal { shell: String, cols: u16, rows: u16 },
    Chat { model: String, messages: Vec<Message> },
    Canvas { objects: Vec<CanvasObject> },
    Custom { type_id: String, state: Value },
}

enum SessionState {
    Running,
    Paused,     // Disconnected but preservable
    Stopped,    // Explicitly ended
}
```

**Container isolation:**
- Each terminal session can run in its own Docker container
- Base image includes common tools + AI tooling
- Project directories bind-mounted
- Container survives disconnects (session persistence)
- Without Docker: session creation fails with a clear error; setup wizard guides installation

### Auth (From Katulong)

Passkey-only. No passwords. No usernames on first visit.

```
First visit: register passkey → session cookie → you're in
Return visit: authenticate passkey → session cookie → you're in
New device: QR code + PIN (LAN) or setup token (remote)
Localhost: auto-authenticated
```

30-day sliding session expiry. HttpOnly cookies. Platform authenticators (Touch ID, Windows Hello, phone biometrics).

### WebSocket Protocol

```
Session I/O:
  → { type: "session.create", kind: "terminal", config: {...} }
  ← { type: "session.created", id: "...", kind: "terminal" }
  → { type: "session.attach", id: "..." }
  ← { type: "session.output", id: "...", data: "..." }
  → { type: "session.input", id: "...", data: "..." }
  → { type: "session.resize", id: "...", cols: 80, rows: 24 }

Lifecycle:
  ← { type: "server.draining" }  // Rolling update
  ← { type: "server.ready" }     // Reconnect now
  ← { type: "session.exit", id: "...", code: 0 }
```

---

## AI-Native

The interface is designed with the assumption that **AI is always a participant**, not a feature you bolt on.

**The surface is a shared workspace.** Both human and AI can see and act on it. The human sees rendered output. The AI receives a structured representation of what's visible and can emit draw commands, arrange sessions, or modify the layout.

**Context flows between sessions.** Terminal output can feed into a chat conversation. A conversation can produce artifacts that appear on the canvas. A diagram can reference running processes in a terminal. Everything is connected through the surface.

**The interface adapts to the work.** Coding session? Mostly terminal with a small chat. Brainstorming? Mostly canvas with conversation threaded through it. Monitoring? Dashboards and status indicators. The surface morphs based on what you're doing.

**AI can render beyond text.** Because we have canvas access with shaders available, an AI agent can produce visualizations, interactive diagrams, or entirely new UI patterns — not just text responses. The rendering layer doesn't constrain what the AI can express.

### Claude Code Integration

Claude Code is a primary use case:
- Runs inside Docker containers (isolated, per-session)
- Terminal I/O proxied through abot
- abot detects AI agent activity and can render richer views
- File changes, test results, build output can be rendered beyond plain terminal text

---

## Self-Provisioning

The binary runs on the host with zero prerequisites. The server and UI always work. Docker is only needed when you create a session — and if it's missing, a setup wizard guides you through installing it.

This is the key insight: **the server should never fail to start.** A normal person downloads the binary, runs it, opens a browser, and sees a working UI. No terminal commands, no config files, no "install these 5 things first." The wizard handles provisioning progressively.

```
Step 1: Run the binary
  → Web server + client + passkey auth
  → UI loads, setup wizard available
  → Works on macOS, Linux, Windows

Step 2: Create first session (Docker needed)
  → If Docker is missing, wizard walks you through setup
  → If Docker is present, session launches in a container
  → Each session is sandboxed with resource limits

Step 3: AI tooling (optional)
  → Claude Code, other AI tools in containers
  → Rich rendering of AI agent output
  → Context bridging between sessions

Step 4: Network access (optional)
  → Remote access with full auth
  → Device pairing
  → Multi-device session sharing
```

Each step builds on the previous. The binary detects what's available and the wizard handles what's missing. No configuration required.

---

## Development Phases

### Phase 0: The Surface
- [ ] Rust server with axum + embedded assets
- [ ] Canvas rendering: draw rects, text, handle mouse/keyboard
- [ ] Single terminal session (host PTY, no Docker)
- [ ] xterm.js → OffscreenCanvas → compositor
- [ ] Passkey auth (port from katulong/dorky_robot)

### Phase 1: Multi-Session
- [ ] Session creation/destruction
- [ ] Spatial arrangement of multiple sessions
- [ ] Focus management
- [ ] Session persistence (survive server restart)
- [ ] Touch gesture support

### Phase 2: Isolation
- [ ] Docker container management (bollard)
- [ ] Container-per-session
- [ ] Base image with dev tools
- [ ] Bind mount management

### Phase 3: Intelligence
- [ ] Chat session type with AI model integration
- [ ] AI agent detection and rich rendering
- [ ] Context bridging between sessions
- [ ] Streaming response rendering

### Phase 4: Evolution
- [ ] Self-update mechanism
- [ ] Device pairing
- [ ] Session sharing across devices
- [ ] Custom session types
- [ ] Rolling updates with zero downtime

---

## Open Questions

1. **Terminal rendering**: xterm.js on OffscreenCanvas composited into our surface, or canvas-native terminal? (Leaning xterm.js — don't reinvent terminal emulation.)

2. **State persistence**: SQLite or flat files? (Leaning SQLite — sessions need indexed queries as they grow.)

3. **Client language**: Plain JavaScript (zero build step, like katulong) or TypeScript? (Leaning TypeScript — the rendering engine benefits from types.)

4. **Container base image**: Minimal (alpine + shell) or batteries-included? (Start minimal, add layers.)

---

## References

- [Katulong](https://github.com/Dorky-Robot/katulong) — Session persistence, passkey auth, rolling updates, touch design
- [Figma](https://figma.com) — Canvas-rendered app with hidden DOM for input (WebGL + IME bridge)
- [tldraw](https://tldraw.com) — Spatial canvas interface, open source
- [xterm.js](https://xtermjs.org) — Terminal emulation for the browser
