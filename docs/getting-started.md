# Getting Started

## Prerequisites

- **Docker** — abot runs each session in a Docker container
- **macOS or Linux** — the binary uses Unix sockets and PTY

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

    # Build the Rust binary (with Docker backend)
    cargo build --release --features docker

    # The binary is at target/release/abot
    ```

## Quick Start

### 1. Start abot

```bash
abot start
```

This launches both the daemon (PTY session owner) and the HTTP/WebSocket server on port **6969**.

### 2. Open in Browser

Visit **http://localhost:6969** in your browser.

On localhost, authentication is bypassed automatically — you'll go straight to the workspace.

### 3. Register a Passkey

For remote access, register a WebAuthn passkey through the setup flow. You can create setup tokens from the CLI:

```bash
abot token create "My Laptop"
```

### 4. Create a Session

Click the **+** button or use the search bar to create a new session. Each session gets its own Docker container with a persistent home directory.

### 5. Arrange Your Workspace

Drag facets around the canvas to arrange your workspace. Focus a facet to route keyboard input to its session.

## What's Next

- [Concepts](concepts.md) — understand sessions, facets, and bundles
- [CLI Reference](cli-reference.md) — all available commands
- [Configuration](configuration.md) — customize ports, bind address, and more
