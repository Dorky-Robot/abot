# Getting Started

## Prerequisites

- **Docker** — abot runs each session in a Docker container (falls back to local PTY if Docker is unavailable)
- **macOS or Linux** — the binary uses Unix sockets, PTY, and `setsid`

## Installation

=== "Homebrew (macOS)"

    ```bash
    brew tap dorky-robot/abot
    brew install abot
    ```

=== "Build from Source"

    ```bash
    git clone https://github.com/Dorky-Robot/abot.git
    cd abot

    # Build the Flutter client
    cd flutter_client && flutter build web --wasm
    cd ..

    # Build the Rust binary
    cargo build --release

    # The binary is at target/release/abot
    ```

    !!! note
        abot requires Docker to run sessions. Make sure Docker Desktop or the Docker daemon is running.

## Quick Start

### 1. Start abot

```bash
abot start
```

This launches the **daemon** (PTY session owner) as a detached process and the **server** (HTTP/WS) in the foreground on port **6969**.

```
INFO abot starting (daemon + server)
INFO daemon ready, starting server
INFO listening on 0.0.0.0:6969
```

### 2. Open in Browser

Visit **http://localhost:6969** in your browser.

On localhost, authentication is bypassed automatically — you'll go straight to the workspace. No passkey, no setup token, no login page.

### 3. Create a Session

Click the **+** button or use the search bar to create a new session. Each session spins up a Docker container with:

- A persistent home directory (`~/.abot/bundles/{name}.abot/home/`)
- TTY with xterm-256color and truecolor support
- Memory limit (512 MB), CPU limit (50%), PID limit (256)
- Non-root user (uid 1000)

### 4. Arrange Your Workspace

Drag facets around the canvas to arrange your workspace. Focus a facet to route keyboard input to its session. Unfocused facets slide into the sidebar.

### 5. Set Up Remote Access

For access from other devices, register a WebAuthn passkey:

```bash
# On the machine running abot, create a setup token
abot token create "My Phone"
```

The token prints once — save it. Then on the remote device:

1. Navigate to `https://your-host:6969`
2. Enter the setup token
3. Register a passkey (Touch ID / Face ID / security key)
4. You're in — future visits just need your passkey

## Verify It's Working

### Check service status

```bash
# Is the daemon running?
ls ~/.abot/daemon.pid && echo "daemon PID: $(cat ~/.abot/daemon.pid)"

# Is the socket alive?
ls ~/.abot/daemon.sock && echo "daemon socket exists"

# Hit the health endpoint
curl -s http://localhost:6969/health
# → {"ok":true}
```

### Check auth status

```bash
curl -s http://localhost:6969/auth/status
# → {"setup":true,"accessMethod":"localhost","authenticated":true}
```

- `setup: true` means no credentials registered yet (first-time setup)
- `accessMethod: "localhost"` means you're auto-authenticated

### View daemon logs

```bash
tail -f ~/.abot/daemon.log
```

## What's Next

- [Features](features.md) — everything abot can do
- [Concepts](concepts.md) — understand sessions, facets, and bundles
- [CLI Reference](cli-reference.md) — all available commands
- [Configuration](configuration.md) — customize ports, bind address, and more
- [Security](security.md) — how auth and isolation work
