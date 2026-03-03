# CLI Reference

abot is a single binary with subcommands for starting services, managing tokens, and performing rolling updates.

```bash
abot [OPTIONS] [COMMAND]
```

## Global Options

| Option | Default | Description |
|--------|---------|-------------|
| `--data-dir <PATH>` | `~/.abot` | Data directory for config, database, bundles, and sockets |
| `-p, --port <PORT>` | `6969` | Port for the HTTP/WS server |
| `-b, --bind <ADDR>` | `0.0.0.0` | Bind address |
| `--help` | | Show help |

Global options can also be set in `~/.abot/config.toml`. CLI flags take precedence over config file values.

## Commands

### start

Start both the daemon and server. This is the **default command** — running `abot` with no subcommand is equivalent to `abot start`.

```bash
abot start                     # default port 6969
abot start -p 8080             # custom port
abot start --bind 127.0.0.1   # localhost only
abot start --data-dir /tmp/abot  # custom data directory
```

**Behavior:**

1. Checks if a daemon is already running (via `daemon.pid`)
2. If not: spawns daemon as a detached process (using `setsid`)
3. Waits for `daemon.sock` to appear (5 second timeout)
4. Starts a supervisor task that monitors the daemon every 5 seconds
5. Runs the server in the foreground

If the daemon is already running, it is reused — only the server starts.

### daemon

Run the daemon directly. The daemon is the PTY session owner — it manages Docker containers (or local PTY processes) and communicates with the server over a Unix socket.

```bash
abot daemon
abot daemon --data-dir /tmp/abot
```

!!! note
    You normally don't call this directly. `abot start` spawns the daemon automatically.

**What it does:**

- Detects backend: Docker (if `/var/run/docker.sock` exists) or local PTY
- Listens on `~/.abot/daemon.sock` (mode `0600`)
- Writes `~/.abot/daemon.pid`
- Starts autosave loop (every 5 minutes for dirty sessions)
- Handles NDJSON IPC requests from the server

### serve

Run the HTTP/WebSocket server only. The daemon must already be running.

```bash
abot serve
abot serve -p 8080 -b 127.0.0.1
```

**What it does:**

- Connects to `daemon.sock`
- Initializes SQLite database (`abot.db`)
- Serves the Flutter client (embedded via rust-embed)
- Handles WebSocket connections, REST API, and auth
- Writes `~/.abot/server.pid`

### update

Perform a rolling update — restart the server without dropping sessions.

```bash
abot update
```

**Process:**

1. Verifies daemon is running (falls back to `abot start` if not)
2. Reads `server.pid` and verifies the PID is actually an abot process
3. Sends `SIGTERM` to the old server
4. Waits for graceful shutdown (10 second timeout with 100ms polling)
5. Sends `SIGKILL` if the old server hasn't exited
6. Starts the new server

Since the daemon owns all sessions, they survive the restart. Clients receive a `server-draining` message and reconnect automatically.

## Token Management

Setup tokens allow pairing new devices via WebAuthn. Each token can be used once to register a passkey.

### token create

Create a new setup token.

```bash
abot token create "My Phone"
abot token create "Work Laptop"
```

**Output:**

```
Token created:
  ID:      550e8400-e29b-41d4-a716-446655440000
  Name:    My Phone
  Expires: 23h 59m

a1b2c3d4e5f6...  ← the token (64 hex chars)

Save this token — it will not be shown again.
```

- Token value is printed to **stdout** (for piping)
- Metadata is printed to **stderr** (for display)
- Tokens expire after **24 hours**
- Token is stored as an **argon2 hash** (plaintext is never saved)

### token list

List all setup tokens with expiry and enrollment status.

```bash
abot token list
```

**Output:**

```
ID                                     NAME                 EXPIRES
550e8400-e29b-41d4-a716-446655440000   My Phone             22h 15m (enrolled: My Phone)
660e8400-e29b-41d4-a716-446655440001   Work Laptop          expired
```

- Shows remaining time or "expired"
- Shows linked credential name if a passkey was registered with this token

### token revoke

Revoke a token by ID. If a credential was registered with this token, the credential is also deleted.

```bash
abot token revoke 550e8400-e29b-41d4-a716-446655440000
```

!!! warning
    Revoking a token also revokes any passkey registered with it, and closes all WebSocket connections for that credential.

## Examples

### Development workflow

```bash
# Start abot
abot start

# In another terminal: create a token for your phone
abot token create "iPhone"
# Copy the token, open https://your-ip:6969 on phone, paste token, register passkey

# Check what tokens exist
abot token list

# Update abot binary and restart (sessions survive)
cargo build --release --features docker
abot update
```

### Custom port and data directory

```bash
abot start -p 8080 --data-dir ~/my-abot-data
```

### Server-only restart

```bash
# Daemon is already running from a previous `abot start`
abot serve -p 7070  # restart server on a different port
```
