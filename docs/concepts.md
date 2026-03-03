# Concepts

## Sessions

A **session** is the core abstraction in abot. Each session is a server-side resource — a PTY process running inside a Docker container. Only session I/O (terminal input/output) crosses the wire between client and server.

Sessions persist across server restarts because the daemon owns them independently.

### Session Lifecycle

| Action | What Happens |
|--------|-------------|
| **Create** | Auto-creates `~/.abot/bundles/{name}.abot/home/`, starts Docker container |
| **Terminal I/O** | Reads/writes directly to the bind-mounted `home/` directory (live) |
| **Save** | Writes metadata files only — the filesystem is always live |
| **Save As** | Copies the entire bundle directory to a new path |
| **Close** | Kills the container; bundle directory stays for reopening |
| **Delete** | Kills the container and deletes the bundle directory |

## Facets

A **facet** is a translucent floating panel — the visual primitive of abot's UI. Facets are drawn on a `<canvas>` with edge glow and depth gradients.

The server knows nothing about facets. All positioning, focus tracking, and z-ordering happens client-side in the Flutter Web app. Each facet displays one session's terminal.

### Focus-Based Routing

The client tracks which facet has focus and tags outgoing keyboard input with the corresponding session ID. This means you can have multiple sessions visible at once, and input always goes to the focused facet.

## .abot Bundles

Each `.abot` bundle **is** the container's sandbox. The bundle's `home/` subdirectory is bind-mounted as `/home/dev` in the Docker container — there's no snapshot/restore cycle.

```
~/.abot/bundles/main.abot/
  manifest.json      ← name, version, timestamps
  credentials.json   ← API keys
  config.json        ← shell, env vars
  home/              ← bind-mounted as /home/dev in container
```

Because the home directory is bind-mounted live, every file you create or modify in the terminal is immediately visible on the host, and vice versa. "Save" only needs to write metadata.

## Authentication

abot uses **WebAuthn passkeys** — no passwords. On localhost, authentication is bypassed automatically.

For remote access, you create a **setup token** from the CLI, then use it to register a passkey from the browser.

!!! note
    If you're accessing abot from `localhost` or `127.0.0.1`, you don't need to register a passkey. Auth is bypassed automatically.
