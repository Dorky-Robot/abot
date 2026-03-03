# Features

Built for developers who want spatial, sandboxed terminal sessions accessible from any device.

## Spatial Canvas UI

A Flutter Web (WASM) canvas that renders translucent floating panels — **facets** — instead of tabs or panes. Arrange your terminals in 2D space.

- **Drag and resize** — move facets anywhere on the canvas
- **Focus routing** — keyboard input goes to the focused facet; other facets show live output
- **Sidebar** — unfocused facets slide into a sidebar strip via CSS transforms
- **FLIP animations** — smooth layout transitions when rearranging
- **Search bar** — filter sessions, create new ones
- **Touch-first** — all interactions designed for tablets and phones

## Container Sandbox

Every session runs in its own Docker container. No shared state, no accidental cross-contamination.

- **Isolated filesystem** — each container has its own rootfs
- **Resource limits** — memory (512 MB), CPU (50%), PID count (256)
- **Security hardened** — all capabilities dropped, no-new-privileges, non-root user
- **Persistent home** — `home/` directory bind-mounted from the host, survives container restarts
- **Fallback** — when Docker is unavailable, falls back to local PTY (same UX, no isolation)

## .abot Bundles

Portable session packages. Each bundle IS the container's sandbox — the `home/` directory is bind-mounted live, no export/import step.

- **Save / Save As** — snapshot metadata (filesystem is always live)
- **Open** — restore a session from a bundle
- **Close** — kill container, keep bundle for later
- **Delete** — kill container and remove bundle
- **Auto-save** — dirty sessions saved every 5 minutes

## Passwordless Authentication

WebAuthn (FIDO2) passkeys — no passwords, no one-time codes, no SMS.

- **Localhost bypass** — auto-authenticated on `127.0.0.1` / `localhost`
- **Setup tokens** — create from CLI, use once to register a passkey on a remote device
- **Brute-force protection** — 5 failed attempts in 15 minutes triggers a 15-minute lockout
- **Session management** — 30-day sessions with auto-refresh, HttpOnly cookies
- **Credential revocation** — deleting a passkey immediately closes all its WebSocket connections

## Session Persistence

The daemon owns all sessions independently of the server. This means:

- **Server restart** — sessions keep running, clients reconnect
- **Rolling update** — swap the binary, `abot update`, zero downtime
- **Daemon supervisor** — if the daemon crashes, the server restarts it within 5 seconds
- **Ring buffer replay** — on reconnect, clients receive the last 5000 lines (or 5 MB) of output

## Real-Time Terminal I/O

WebSocket-based terminal I/O with optional WebRTC upgrade for lower latency.

- **WebSocket** — reliable transport for all terminal I/O
- **WebRTC DataChannel** — peer-to-peer upgrade for ultra-low latency (client sends offer, server answers)
- **Automatic fallback** — if P2P fails (firewall, NAT), falls back to WebSocket seamlessly
- **Multi-client** — multiple clients can attach to the same session (shared terminal, like tmux)

## Single Binary Distribution

One binary contains everything:

- **Rust server** — HTTP, WebSocket, auth, session management
- **Rust daemon** — PTY management, Docker orchestration
- **Flutter client** — embedded via rust-embed, served as static assets
- **No external dependencies** — no Node.js, no npm, no separate frontend build in production

## Anthropic API Key Management

Built-in support for injecting API keys into session containers:

- **Store key** — `POST /api/anthropic/key` saves and pushes to all running sessions
- **Auto-detect** — distinguishes `sk-ant-api*` keys from OAuth tokens
- **Environment injection** — sets `ANTHROPIC_API_KEY`, `CLAUDE_API_KEY`, or `CLAUDE_CODE_OAUTH_TOKEN` inside containers
- **Per-session override** — bundle `credentials.json` can override the global key

## File Browser

Built-in file browser for selecting bundle locations:

- **Directory listing** — browse the host filesystem with hidden file filtering
- **Path expansion** — `~` expands to home directory
- **Native pickers** — OS-native file/directory picker dialogs
- **Path validation** — canonicalization prevents directory traversal

## Multi-Session Support

Create, rename, switch between, and delete sessions freely.

- **Named sessions** — each session has a unique name
- **Rename** — rename sessions without restarting them
- **Dirty tracking** — sessions with unsaved changes show a dirty indicator
- **Concurrent attach** — multiple browser tabs can show the same session
- **Independent lifecycle** — closing a facet doesn't kill the session

## CSRF Protection

All state-changing operations (POST, PUT, DELETE) require a CSRF token:

- Token generated at registration/login and stored in the session
- Validated via `X-CSRF-Token` header
- Constant-time comparison prevents timing attacks
- Not required on localhost (same-origin by definition)

## Developer Experience

- **Verbose logging** — `RUST_LOG=abot=debug` for detailed output
- **Health endpoint** — `GET /health` for monitoring
- **Auth status** — `GET /auth/status` shows access method and setup state
- **CLI token management** — create, list, revoke tokens without touching the browser
