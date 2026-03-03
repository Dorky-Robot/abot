# API Reference

Complete reference for abot's REST API, WebSocket protocol, and daemon IPC.

## REST API

All endpoints are served on the same port (default `6969`). Auth-protected endpoints require a valid `abot_session` cookie.

### Health

#### `GET /health`

Check if the server is running.

```bash
curl http://localhost:6969/health
```

```json
{"ok": true}
```

No authentication required.

### Auth

#### `GET /auth/status`

Check authentication state and access method.

```bash
curl http://localhost:6969/auth/status
```

```json
{
  "setup": true,
  "accessMethod": "localhost",
  "authenticated": true
}
```

| Field | Description |
|-------|-------------|
| `setup` | `true` if no credentials registered (first-time setup) |
| `accessMethod` | `"localhost"` or `"internet"` |
| `authenticated` | Whether the request is authenticated |

#### `POST /auth/register/options`

Get WebAuthn registration challenge. First registration requires localhost or a valid setup token.

**Request:**

```json
{
  "setupToken": "a1b2c3d4..."
}
```

`setupToken` is optional on localhost, required for remote registration.

**Response:**

```json
{
  "options": { /* WebAuthn PublicKeyCredentialCreationOptions */ },
  "userId": "uuid",
  "challengeId": "uuid"
}
```

#### `POST /auth/register/verify`

Complete WebAuthn registration.

**Request:**

```json
{
  "challengeId": "uuid",
  "credential": { /* WebAuthn AuthenticatorAttestationResponse */ },
  "deviceName": "My Laptop"
}
```

**Response:**

```json
{
  "success": true,
  "csrfToken": "hex-string"
}
```

Sets `abot_session` cookie (30-day expiry, HttpOnly, SameSite=Lax).

#### `POST /auth/login/options`

Get WebAuthn authentication challenge.

**Response:**

```json
{
  "options": { /* WebAuthn PublicKeyCredentialRequestOptions */ },
  "challengeId": "uuid"
}
```

#### `POST /auth/login/verify`

Complete WebAuthn authentication.

**Request:**

```json
{
  "challengeId": "uuid",
  "credential": { /* WebAuthn AuthenticatorAssertionResponse */ }
}
```

**Response:**

```json
{
  "success": true,
  "csrfToken": "hex-string"
}
```

#### `POST /auth/logout`

Clear session cookie and delete server-side session.

**Response:**

```json
{"success": true}
```

### Setup Tokens

All token endpoints require authentication and CSRF token.

#### `GET /auth/tokens`

List all setup tokens and orphaned credentials.

```json
{
  "tokens": [
    {
      "id": "uuid",
      "name": "My Phone",
      "expiresAt": 1719500000,
      "credential": {
        "id": "base64",
        "name": "My Phone",
        "createdAt": "2025-06-15T10:30:00Z"
      }
    }
  ],
  "orphanedCredentials": []
}
```

#### `POST /auth/tokens`

Create a new setup token.

**Request:**

```json
{"name": "My Phone"}
```

**Response:**

```json
{
  "id": "uuid",
  "token": "hex-string-64-chars",
  "expiresAt": 1719500000
}
```

Headers: `X-CSRF-Token: <token>`

#### `DELETE /auth/tokens/{id}`

Revoke a token and its linked credential.

Headers: `X-CSRF-Token: <token>`

```json
{"success": true}
```

### Sessions

All session endpoints require authentication.

#### `GET /sessions`

List all sessions.

```json
[
  {
    "name": "main",
    "alive": true,
    "exitCode": null,
    "bundlePath": "/home/user/.abot/bundles/main.abot",
    "dirty": false
  }
]
```

#### `POST /sessions`

Create a new session.

**Request:**

```json
{"name": "my-project"}
```

**Response:**

```json
{"name": "my-project"}
```

#### `GET /sessions/{name}`

Get session details.

```json
{
  "name": "main",
  "alive": true,
  "exitCode": null,
  "bundlePath": "/home/user/.abot/bundles/main.abot",
  "dirty": false
}
```

#### `PUT /sessions/{name}`

Rename a session.

**Request:**

```json
{"name": "new-name"}
```

**Response:**

```json
{"oldName": "main", "newName": "new-name"}
```

#### `DELETE /sessions/{name}`

Delete a session — kills the container AND removes the bundle directory.

```json
{"name": "main"}
```

### Bundle Operations

#### `POST /sessions/open`

Open an existing `.abot` bundle and create a session from it.

**Request:**

```json
{"path": "/path/to/bundle.abot"}
```

**Response:**

```json
{"name": "my-project", "path": "/path/to/bundle.abot"}
```

#### `POST /sessions/{name}/save`

Save session metadata to its existing bundle path.

```json
{"session": "main", "path": "/home/user/.abot/bundles/main.abot"}
```

#### `POST /sessions/{name}/save-as`

Copy the entire bundle to a new path.

**Request:**

```json
{"path": "/new/location/backup.abot"}
```

**Response:**

```json
{"session": "main", "path": "/new/location/backup.abot"}
```

#### `POST /sessions/{name}/close`

Close a session — kills the container but keeps the bundle on disk for reopening.

```json
{"session": "main"}
```

### Session Credentials

#### `POST /sessions/{name}/credentials`

Set credentials for a session.

**Request:**

```json
{"apiKey": "sk-ant-..."}
```

#### `GET /sessions/{name}/credentials/status`

Check if credentials are configured for a session.

```json
{"session": "main", "status": "configured"}
```

#### `DELETE /sessions/{name}/credentials`

Remove credentials from a session.

### Anthropic API Key

#### `POST /api/anthropic/key`

Store an API key and push it to all running sessions.

**Request:**

```json
{"key": "sk-ant-api03-..."}
```

**Detection logic:**

- `sk-ant-api*` → sets `ANTHROPIC_API_KEY` + `CLAUDE_API_KEY`
- Other tokens → sets `CLAUDE_CODE_OAUTH_TOKEN`

#### `GET /api/anthropic/key/status`

Check if an API key is stored.

```json
{"status": "configured"}
```

#### `DELETE /api/anthropic/key`

Remove the stored API key and clear from all running sessions.

### Configuration

#### `GET /api/config`

Get instance configuration.

```json
{
  "instanceName": "abot",
  "bundleDir": "~/.abot/bundles"
}
```

#### `PUT /api/config/instance-name`

```json
{"instanceName": "my-abot"}
```

#### `PUT /api/config/bundle-dir`

```json
{"bundleDir": "/path/to/bundles"}
```

### File Browser

#### `GET /api/browse`

List directory contents.

**Query parameters:**

| Param | Default | Description |
|-------|---------|-------------|
| `path` | `~` | Directory to list |
| `show_hidden` | `false` | Include hidden files |

```json
{
  "path": "/home/user",
  "parent": "/home",
  "entries": [
    {"name": "Documents", "isDir": true, "size": 4096, "modified": 1719500000},
    {"name": "file.txt", "isDir": false, "size": 1234, "modified": 1719500000}
  ]
}
```

Directories listed first, then files, both sorted alphabetically (case-insensitive).

#### `POST /api/pick-directory`

Open native OS directory picker dialog. Returns selected path.

#### `POST /api/pick-file`

Open native OS file picker dialog. Returns selected path.

#### `POST /api/pick-save-location`

Open native OS save dialog. Returns selected path.

### Credentials (Auth)

#### `GET /api/credentials`

List all registered WebAuthn credentials.

```json
{
  "credentials": [
    {
      "id": "base64-credential-id",
      "name": "My Laptop",
      "userAgent": "Mozilla/5.0...",
      "createdAt": "2025-06-15T10:30:00Z",
      "lastUsedAt": "2025-06-20T08:00:00Z"
    }
  ]
}
```

#### `DELETE /api/credentials/{id}`

Delete a credential. Cascades: deletes sessions, closes WebSockets.

Headers: `X-CSRF-Token: <token>`

!!! warning
    Cannot delete the last credential from a remote connection.

---

## WebSocket Protocol

Connect to `ws://host:port/stream` (or `wss://` for HTTPS). Requires a valid `abot_session` cookie.

### Client → Server Messages

#### attach

Join a session. Creates the session if it doesn't exist.

```json
{
  "type": "attach",
  "session": "main",
  "cols": 120,
  "rows": 40
}
```

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `session` | Yes | | Session name |
| `cols` | No | `120` | Terminal columns |
| `rows` | No | `40` | Terminal rows |

#### input

Send keystrokes to a session.

```json
{
  "type": "input",
  "data": "ls -la\n",
  "session": "main"
}
```

#### resize

Update terminal dimensions.

```json
{
  "type": "resize",
  "cols": 80,
  "rows": 24,
  "session": "main"
}
```

#### detach

Leave a session. Omit `session` to detach from all.

```json
{
  "type": "detach",
  "session": "main"
}
```

#### p2p-signal

WebRTC signaling (offer, answer, ICE candidates).

```json
{
  "type": "p2p-signal",
  "data": { /* SDP or ICE candidate */ }
}
```

### Server → Client Messages

#### attached

Confirmation of successful attach, with ring buffer snapshot.

```json
{
  "type": "attached",
  "session": "main",
  "buffer": "$ last 5000 lines of output..."
}
```

#### output

Terminal output from a session.

```json
{
  "type": "output",
  "data": "file1.txt\nfile2.txt\n",
  "session": "main"
}
```

#### exit

Session process exited.

```json
{
  "type": "exit",
  "code": 0,
  "session": "main"
}
```

#### session-removed

Session was deleted.

```json
{
  "type": "session-removed",
  "session": "main"
}
```

#### server-draining

Server is shutting down gracefully (rolling update). Client should prepare to reconnect.

```json
{
  "type": "server-draining"
}
```

#### error

Server error.

```json
{
  "type": "error",
  "message": "session not found"
}
```

#### p2p-signal

WebRTC answer or ICE candidate from server.

```json
{
  "type": "p2p-signal",
  "data": { /* SDP answer or ICE candidate */ }
}
```

#### p2p-ready

P2P DataChannel successfully established.

```json
{"type": "p2p-ready"}
```

#### p2p-closed / p2p-unavailable

P2P DataChannel closed or could not be established.

```json
{"type": "p2p-closed"}
{"type": "p2p-unavailable"}
```

---

## Daemon IPC Protocol

The server and daemon communicate over a **Unix domain socket** (`~/.abot/daemon.sock`) using **NDJSON** (newline-delimited JSON).

### Protocol Rules

- **RPC requests** include an `"id"` field → daemon responds with matching `"id"`
- **Fire-and-forget** messages have no `"id"` → no response
- **Broadcast events** from daemon have no `"id"` → pushed to all server connections

### RPC Requests

#### list-sessions

```json
{"type": "list-sessions", "id": "req-1"}
```

Response:

```json
{"id": "req-1", "sessions": [{"name": "main", "alive": true, ...}]}
```

#### create-session

```json
{
  "type": "create-session", "id": "req-2",
  "name": "my-project", "cols": 120, "rows": 40,
  "env": {"EDITOR": "vim"}
}
```

#### get-session

```json
{"type": "get-session", "id": "req-3", "name": "main"}
```

#### delete-session

```json
{"type": "delete-session", "id": "req-4", "name": "main"}
```

#### rename-session

```json
{
  "type": "rename-session", "id": "req-5",
  "oldName": "main", "newName": "production"
}
```

#### open-bundle

```json
{"type": "open-bundle", "id": "req-6", "path": "/path/to/bundle.abot"}
```

#### save-session

```json
{"type": "save-session", "id": "req-7", "session": "main"}
```

#### save-session-as

```json
{
  "type": "save-session-as", "id": "req-8",
  "session": "main", "path": "/new/path.abot"
}
```

#### close-session

```json
{"type": "close-session", "id": "req-9", "session": "main", "save": false}
```

`save` defaults to `false`. If `true`, saves bundle metadata before closing.

#### update-agent-env

Push environment variables to all running sessions.

```json
{
  "type": "update-agent-env", "id": "req-10",
  "env": {"ANTHROPIC_API_KEY": "sk-...", "OLD_VAR": null}
}
```

Set a key to `null` to delete it.

#### update-session-env

Push environment variables to a specific session.

```json
{
  "type": "update-session-env", "id": "req-11",
  "session": "main",
  "env": {"ANTHROPIC_API_KEY": "sk-..."}
}
```

#### ping

```json
{"type": "ping", "id": "req-12"}
```

Response:

```json
{"id": "req-12"}
```

### Fire-and-Forget Messages

#### input

```json
{"type": "input", "clientId": "client-1", "session": "main", "data": "ls\n"}
```

#### resize

```json
{"type": "resize", "clientId": "client-1", "session": "main", "cols": 80, "rows": 24}
```

#### detach

```json
{"type": "detach", "clientId": "client-1", "session": "main"}
```

### Broadcast Events (Daemon → Server)

#### output

```json
{"type": "output", "session": "main", "data": "prompt$ "}
```

#### exit

```json
{"type": "exit", "session": "main", "code": 0}
```

#### session-removed

```json
{"type": "session-removed", "session": "main"}
```
