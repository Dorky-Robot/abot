# Configuration

## Data Directory

All abot data lives under `~/.abot/` by default. Override with `--data-dir`:

```bash
abot start --data-dir /path/to/data
```

The directory is created automatically with `0700` permissions (owner-only access).

### Directory Layout

```
~/.abot/
  config.toml          ← CLI defaults (port, bind)
  config.json          ← Instance config (instanceName, bundleDir)
  abot.db              ← SQLite database (auth, sessions, tokens)
  daemon.sock          ← Unix domain socket for daemon IPC (mode 0600)
  daemon.pid           ← Daemon PID file
  daemon.log           ← Daemon stdout/stderr log
  server.pid           ← Server PID file (for rolling updates)
  bundles/             ← Session bundles
    main.abot/
      manifest.json    ← Name, version, timestamps, image
      credentials.json ← API keys
      config.json      ← Shell, resource limits, env vars
      home/            ← Bind-mounted as /home/dev in Docker container
```

## config.toml

CLI defaults. These are overridden by CLI flags if provided.

```toml
# Server port (default: 6969)
port = 6969

# Bind address (default: "0.0.0.0")
bind = "0.0.0.0"
```

**Resolution order:** CLI flag > config.toml > built-in default

## config.json

Instance configuration managed via the REST API.

```json
{
  "instanceName": "my-abot",
  "bundleDir": "~/.abot/bundles"
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `instanceName` | `"abot"` | Display name for this instance |
| `bundleDir` | `~/.abot/bundles` | Where session bundles are stored |

**Endpoints:**

- `GET /api/config` — read current config
- `PUT /api/config/instance-name` — update instance name
- `PUT /api/config/bundle-dir` — update bundle directory

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `abot=info` | Log level filter ([`EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) syntax) |

### Log Levels

```bash
# Default (info)
abot start

# Debug — shows IPC messages, auth decisions
RUST_LOG=abot=debug abot start

# Trace — very verbose, includes every WebSocket message
RUST_LOG=abot=trace abot start

# Specific module
RUST_LOG=abot::auth=debug,abot::stream=trace abot start
```

## Bundle Configuration

Each `.abot` bundle has its own configuration files. See [Concepts: .abot Bundles](concepts.md#abot-bundles) for the full format.

### manifest.json

Session metadata. Managed automatically by save/open operations.

```json
{
  "version": 1,
  "name": "my-project",
  "created_at": "2025-06-15T10:30:00Z",
  "updated_at": "2025-06-15T14:22:00Z",
  "image": "abot-session"
}
```

| Field | Description |
|-------|-------------|
| `version` | Format version (always `1`) |
| `name` | Session name |
| `created_at` | Original creation time (preserved across saves) |
| `updated_at` | Last save time |
| `image` | Docker image (default: `abot-session`, fallback: `alpine:3`) |

### config.json (per-bundle)

Session runtime configuration.

```json
{
  "shell": "/bin/bash",
  "memory_mb": 512,
  "cpu_percent": 50,
  "env": {
    "EDITOR": "vim",
    "MY_PROJECT_ENV": "production"
  }
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `shell` | `/bin/bash` | Shell to spawn in the container |
| `memory_mb` | `512` | Container memory limit in MB |
| `cpu_percent` | `50` | CPU limit as percentage of one core |
| `env` | `{}` | Custom environment variables |

### credentials.json

Sensitive credentials injected into the container environment.

```json
{
  "api_key": "sk-ant-...",
  "claude_token": "oauth-..."
}
```

| Field | Environment Variable(s) |
|-------|------------------------|
| `api_key` | `ANTHROPIC_API_KEY` + `CLAUDE_API_KEY` |
| `claude_token` | `CLAUDE_CODE_OAUTH_TOKEN` |

!!! warning
    `credentials.json` contains sensitive data. The `~/.abot/` directory is created with `0700` permissions. Never commit bundle directories to version control.

## SQLite Database

Auth state is stored in `~/.abot/abot.db`. Schema:

| Table | Purpose |
|-------|---------|
| `users` | Registered users (id, name, created_at) |
| `credentials` | WebAuthn passkeys (public key, counter, device info, linked token) |
| `sessions` | Auth sessions (token, credential_id, csrf_token, expiry) |
| `setup_tokens` | Device enrollment tokens (argon2 hash, name, expiry) |
| `config` | Key-value config store |
| `anthropic_api_key` | Stored API key (single row) |

## Ports

| Port | Service | Notes |
|------|---------|-------|
| `6969` | HTTP + WebSocket | Default, configurable via `-p` or config.toml |

abot serves everything on a single port — HTTP pages, REST API, WebSocket upgrades, and embedded assets.

## Docker Configuration

When using the Docker backend, abot expects:

- **Docker socket** at `/var/run/docker.sock`
- **Image**: `abot-session` (custom) or `alpine:3` (fallback, pulled automatically)
- **Build with**: `cargo build --features docker`

### Container Defaults

| Setting | Value | Configurable? |
|---------|-------|--------------|
| User | `1000:1000` | No |
| Memory | 512 MB | Yes (per-bundle `config.json`) |
| CPU | 50% of one core | Yes (per-bundle `config.json`) |
| PIDs | 256 max | No |
| Capabilities | All dropped | No |
| Security | no-new-privileges | No |
