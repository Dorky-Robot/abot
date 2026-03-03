# CLI Reference

abot provides a command-line interface for starting services, managing tokens, and performing rolling updates.

## Global Options

| Option | Default | Description |
|--------|---------|-------------|
| `--data-dir <PATH>` | `~/.abot` | Data directory for config, database, and bundles |
| `-p, --port <PORT>` | `6969` | Port to listen on |
| `-b, --bind <ADDR>` | `0.0.0.0` | Bind address |

## Commands

### start

Start both the daemon and server. This is the default command.

```bash
abot start
abot                   # same as abot start
abot start -p 8080     # custom port
```

The daemon is spawned as a detached process. If a daemon is already running, it is reused. The server runs in the foreground.

A supervisor task monitors the daemon and restarts it if it dies.

### daemon

Run the daemon directly (PTY session owner). Normally you don't call this directly — `abot start` handles it.

```bash
abot daemon
```

The daemon listens on a Unix domain socket at `~/.abot/daemon.sock` and communicates via NDJSON.

### serve

Run the HTTP/WebSocket server only. The daemon must already be running.

```bash
abot serve
abot serve -p 8080 -b 127.0.0.1
```

### update

Perform a rolling update: drain the current server and start a new one without dropping sessions.

```bash
abot update
```

The update process:

1. Checks that the daemon is running (falls back to `start` if not)
2. Sends SIGTERM to the old server process
3. Waits for graceful shutdown (10s timeout, then SIGKILL)
4. Starts the new server

Since the daemon owns all sessions, they survive the server restart. Clients reconnect automatically.

## Token Management

Setup tokens allow pairing new devices via WebAuthn. Each token can be used once to register a passkey. Tokens expire after 24 hours.

### token create

Create a new setup token.

```bash
abot token create "My Phone"
```

The token value is printed to stdout and shown **once** — save it immediately. Metadata is printed to stderr.

### token list

List all setup tokens with their expiry and enrollment status.

```bash
abot token list
```

### token revoke

Revoke a token by ID. If a credential was registered with the token, the credential is also revoked.

```bash
abot token revoke <ID>
```

!!! warning
    Revoking a token also revokes any passkey that was registered with it.
