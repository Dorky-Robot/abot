# Configuration

## Data Directory

All abot data lives under `~/.abot/` by default. Override with `--data-dir`:

```bash
abot start --data-dir /path/to/data
```

### Directory Structure

```
~/.abot/
  config.toml        ← server configuration
  abot.db            ← SQLite database (auth, tokens, credentials)
  daemon.sock        ← Unix domain socket for daemon IPC
  daemon.pid         ← daemon PID file
  server.pid         ← server PID file
  daemon.log         ← daemon log output
  bundles/           ← session bundles
    main.abot/
      manifest.json
      credentials.json
      config.json
      home/           ← bind-mounted into Docker container
```

## Config File

abot reads `~/.abot/config.toml` for default settings. CLI flags override config file values.

```toml
# Server port (default: 6969)
port = 6969

# Bind address (default: "0.0.0.0")
bind = "0.0.0.0"
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `abot=info` | Log level filter (uses `tracing-subscriber` `EnvFilter`) |

```bash
RUST_LOG=abot=debug abot start    # verbose logging
RUST_LOG=abot=trace abot start    # very verbose
```

## Bundle Configuration

Each `.abot` bundle can have its own configuration:

### manifest.json

```json
{
  "name": "main",
  "version": "1.0.0",
  "created_at": "2025-01-01T00:00:00Z",
  "updated_at": "2025-01-01T00:00:00Z"
}
```

### config.json

```json
{
  "shell": "/bin/zsh",
  "env": {
    "EDITOR": "vim"
  }
}
```

### credentials.json

```json
{
  "api_keys": {}
}
```

!!! warning
    `credentials.json` contains sensitive data. The `~/.abot/` directory is created with `0700` permissions (owner-only access).

## Ports

| Port | Service |
|------|---------|
| `6969` | HTTP server + WebSocket (default) |

Change the port with `-p` flag or `config.toml`:

```bash
abot start -p 8080
```
