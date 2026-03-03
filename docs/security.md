# Security

Every layer of abot is built with security in mind — from authentication to container isolation to transport protection.

## Authentication

### WebAuthn (FIDO2) Passkeys

Passwordless, phishing-resistant authentication using public key cryptography. Supports Touch ID, Face ID, security keys, and platform authenticators.

- **RP ID:** `localhost` for local access, hostname for remote
- **RP Origin:** `http://localhost:PORT` for local, `https://HOST:PORT` for remote
- Uses the `webauthn-rs` library with counter validation (detects credential cloning)

### Localhost Auto-Bypass

Requests from loopback addresses (`127.0.0.1`, `::1`) are auto-authenticated — no login page, no passkey prompt. This is intentional:

!!! info "Why localhost is trusted"
    If an attacker has localhost access to your machine, they already have full system access (they can read your files, run processes, etc.). Adding an auth layer on localhost provides no meaningful security benefit — it just adds friction.

**Three-level validation** prevents tunnel/proxy bypass:

1. **Socket address** must be loopback (`127.0.0.1`, `::1`, `::ffff:127.0.0.1`)
2. **Host header** must match localhost patterns (`localhost`, `127.0.0.1`, `[::1]`)
3. **Origin header** (if present) must also match localhost

This blocks attacks where a tunnel (e.g., ngrok) forwards traffic to localhost — the Host header won't match.

### Setup Tokens

Device enrollment tokens for remote passkey registration:

- **Generation:** 32 random bytes, hex-encoded (64 characters)
- **Storage:** argon2 hash only — plaintext never saved to disk
- **TTL:** 24 hours
- **Single-use:** consumed after one successful registration
- **Cascade delete:** revoking a token also revokes the passkey registered with it

### Brute-Force Protection

Rate limiting on authentication endpoints:

| Parameter | Value |
|-----------|-------|
| Max attempts | 5 |
| Window | 15 minutes |
| Lockout duration | 15 minutes |
| Tracking | Per-credential ID (`_global` for login endpoint) |
| Cleanup | Background task every 60 seconds |

When locked out, the server returns HTTP 400 with a message including the retry-after time.

### Challenge Store

WebAuthn challenges are single-use and time-limited:

- **TTL:** 5 minutes
- **Storage:** In-memory HashMap (not persisted to disk)
- **Lifecycle:** Created during `register/options` or `login/options`, consumed during `verify`
- **Cleanup:** Background task every 60 seconds

## Session Security

### Cookies

| Attribute | Value |
|-----------|-------|
| Name | `abot_session` |
| Expiry | 30 days |
| HttpOnly | Yes (no JavaScript access) |
| SameSite | Lax (CSRF mitigation) |
| Secure | Yes (on non-localhost) |
| Auto-refresh | Extends expiry if idle > 24 hours |

### CSRF Protection

All state-changing operations (POST, PUT, DELETE) require a CSRF token:

- Generated at registration/login
- Stored in the `sessions` database table
- Validated via `X-CSRF-Token` header
- **Constant-time comparison** prevents timing attacks
- Not required on localhost

### Credential Revocation

Deleting a credential triggers immediate cleanup:

1. All auth sessions for that credential are deleted
2. All WebSocket connections for that credential are closed (code 1008)
3. The credential record is removed from the database

!!! warning
    Cannot delete the last credential from a remote connection — this would lock you out. Use localhost to remove the last credential.

## Container Isolation

When using the Docker backend, each session is sandboxed:

| Security Control | Setting |
|-----------------|---------|
| **User** | `1000:1000` (non-root) |
| **Capabilities** | All dropped |
| **Privileges** | `no-new-privileges` |
| **Memory limit** | 512 MB (configurable) |
| **CPU limit** | 50% of one core (configurable) |
| **PID limit** | 256 processes max |
| **Filesystem** | Only `/home/dev` is writable (bind-mounted from host) |

### What containers prevent

- A compromised session cannot access other sessions' data
- A runaway process cannot consume all system memory or CPU
- A fork bomb is limited to 256 processes
- Root escalation is blocked by dropped capabilities and no-new-privileges

### What containers don't prevent

- A session can read/write its own `home/` directory on the host
- Network access from within the container (no network isolation by default)
- Filling the bind-mounted home directory (limited by host disk space)

## Transport Security

### WebSocket

- **Origin validation** — rejects connections where Origin doesn't match Host
- **Session validation** — every WebSocket upgrade requires a valid session cookie
- **Continuous validation** — session checked on every message, not just at upgrade

### P2P (WebRTC)

- **Signaling** — WebRTC offer/answer/ICE exchanged over the authenticated WebSocket
- **DataChannel** — encrypted by default (DTLS)
- **Fallback** — if P2P fails, falls back to WebSocket (no security downgrade)

## Input Validation

### Path Traversal Prevention

- All file paths are canonicalized before use
- Bundle paths are validated to prevent nesting inside other bundles
- The file browser API rejects `..`, `//`, and hidden file patterns

### Database

- All SQL queries use parameterized statements (no string interpolation)
- SQLite with bundled compilation (no system library dependency)

## Data Directory Permissions

- `~/.abot/` created with `0700` (owner-only read/write/execute)
- `daemon.sock` created with `0600` (owner-only read/write)
- `abot.db` inherits directory permissions
- `credentials.json` in bundles protected by directory permissions

## Security Checklist

If you're deploying abot for remote access:

- [ ] Create setup tokens from localhost, distribute out-of-band
- [ ] Use HTTPS for remote access (put abot behind a reverse proxy or tunnel)
- [ ] Review which Docker image your sessions use (`abot-session` vs `alpine:3`)
- [ ] Monitor `~/.abot/daemon.log` for unexpected session creation
- [ ] Periodically review `abot token list` for expired or unused tokens
- [ ] Use `RUST_LOG=abot::auth=debug` to audit auth decisions
