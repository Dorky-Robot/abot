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
  abots/               ← Canonical abot repos (v2, git-backed)
    main.abot/
      .git/            ← Auto-initialized git repo, owns all history
      .gitignore       ← Excludes credentials, scrollback, caches
      manifest.json    ← Name, version 2, timestamps, image
      credentials.json ← API keys (standalone use; excluded from git)
      config.json      ← Shell, resource limits, env vars
      scrollback       ← Terminal scrollback (persisted across close/reopen)
      home/            ← Bind-mounted into kubo container
  kubos/               ← Shared runtime rooms (NOT git repos)
    default.kubo/
      manifest.json    ← Name, version, abots list
      credentials.json ← Kubo-level API keys (injected into container)
      Dockerfile       ← Optional custom image
      alice/           ← Git worktree of alice.abot on branch kubo/default
        .git           ← File (not dir), points to alice.abot/.git
        home/          ← Bind-mounted as /home/abots/alice/home
      bob/             ← Git worktree of bob.abot on branch kubo/default
        .git
        home/
```

!!! note "Migration from v1"
    If you have an existing `~/.abot/bundles/` directory from a v1 install, the daemon auto-migrates it to `~/.abot/abots/` on startup. Each abot is initialized as a git repo, and a default kubo is created.

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
  "bundleDir": "~/.abot/abots"
}
```

| Field | Default | Description |
|-------|---------|-------------|
| `instanceName` | `"abot"` | Display name for this instance |
| `bundleDir` | `~/.abot/abots` | Where session bundles are stored |

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
  "version": 2,
  "name": "my-project",
  "created_at": "2026-03-01T10:30:00Z",
  "updated_at": "2026-03-01T14:22:00Z",
  "image": "abot-session"
}
```

| Field | Description |
|-------|-------------|
| `version` | Format version (`2` for git-backed bundles; v1 auto-migrated on open) |
| `name` | Session name |
| `created_at` | Original creation time (preserved across saves) |
| `updated_at` | Last save time |
| `image` | Docker image (default: `abot-session`, fallback: `alpine:3`) |

### config.json (per-bundle)

Session runtime configuration.

```json
{
  "shell": "/bin/bash",
  "memory_mb": 2048,
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
| `memory_mb` | `2048` | Container memory limit in MB |
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
    `credentials.json` contains sensitive data and is excluded from git via `.gitignore`. The `~/.abot/` directory is created with `0700` permissions. The `credentials.json` file itself has `0600` permissions.

!!! info "Kubo vs abot credentials"
    When an abot is employed in a kubo, **credentials come from the kubo** (`kubo-name.kubo/credentials.json`), not from the abot's own `credentials.json`. This ensures credentials never travel when abots are shared between users or kubos. Standalone abots (not in any kubo) use their own `credentials.json`.

## Kubo Configuration

Each kubo has a manifest, optional Dockerfile, and kubo-level credentials. A kubo is **not** a git repo — it's infrastructure. Abots inside it are git worktrees from their canonical repos.

### Kubo Credentials

```json
{
  "api_key": "sk-ant-...",
  "claude_token": "oauth-..."
}
```

Stored at `~/.abot/kubos/{name}.kubo/credentials.json`. These credentials are injected into the container environment for all abots in the kubo, replacing any per-abot credentials.

### Kubo Manifest

```json
{
  "version": 1,
  "name": "default",
  "created_at": "2026-03-01T10:00:00Z",
  "updated_at": "2026-03-01T10:00:00Z",
  "abots": ["alice", "bob"]
}
```

| Field | Description |
|-------|-------------|
| `version` | Kubo manifest version (currently `1`) |
| `name` | Kubo name |
| `created_at` | Creation timestamp |
| `updated_at` | Last modification timestamp |
| `abots` | List of abot names currently in the kubo |

### Custom Dockerfile

Place a `Dockerfile` in the kubo directory to build a custom image:

```
~/.abot/kubos/ml.kubo/Dockerfile
```

When present, abot builds an image named `abot-kubo-{name}` (e.g., `abot-kubo-ml`) from it on container start.

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

abot requires Docker for all sessions:

- **Docker socket** at `/var/run/docker.sock`
- **Image**: `abot-session` (custom) or `alpine:3` (fallback, pulled automatically)

### Container Defaults (Kubo)

All sessions run inside kubo containers.

| Setting | Value | Configurable? |
|---------|-------|--------------|
| User | `1000:1000` | No |
| Memory | 2 GB | No |
| CPU | 100% of one core | No |
| PIDs | 512 max | No |
| Capabilities | All dropped | No |
| Security | no-new-privileges | No |
| Image | Custom Dockerfile per kubo | Yes |
| Home mount | `→ /home/abots/` | No |
| Idle timeout | 5 minutes | No |
