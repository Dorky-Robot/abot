# abot

A headless CLI that manages AI agent identities as git repositories.

abot does one thing: agent lifecycle management. Each agent is a git repo
with a name, config, instructions, and a `home/` working directory.
Cloning, employing into rooms (worktrees), and folding work back
(merging) are just git operations under the hood.

abot intentionally does not run containers (kubo's job), serve
HTTP/WebSocket (no server, no daemon), render UI (CLI-only), or
orchestrate workflows (humOS or shell scripts compose these tools).

## Install

```sh
brew install dorky-robot/tap/abot
```

Requirements:
- macOS aarch64 (Apple Silicon)
- An [Ollama](https://ollama.com) install with at least one chat model
  pulled, for `abot run`

## Quick start

```sh
# One-time: tell abot which identity to use for git commits.
echo "you@example.com" > ~/.abot/commit_email
echo "Your Name"       > ~/.abot/commit_name

# Create an agent.
abot create alice

# Tune her disposition and model.
abot config alice instructions "You are alice, a terse triager."
abot config alice model gemma4:31b

# Read stdin, dispatch to Ollama, stream stdout.
echo "say hi in one word" | abot run alice
```

## Verbs

```
abot create <name>                     New agent identity
abot list                              All agents (alias: ls)
abot show <name>                       Manifest, config, branches, worktrees
abot clone <source> <new>              Full repo copy with fresh manifest
abot employ <name> <room>              Worktree at ~/.abot/kubos/<room>/<name>/
abot dismiss <name> <room>             Remove worktree, keep branch
abot integrate <name> <room>           Merge kubo/<room> into main, drop branch
abot discard <name> <room>             Force-drop kubo/<room> branch
abot run <name> [--in <room>]          stdin → LLM → stdout (Ollama)
abot log <name> [--room <room>]        git log
abot diff <name> <room>                git diff <main>..kubo/<room>
abot config <name> [key] [value]       Get/set per-agent config
abot rm <name>                         Delete agent entirely
```

## Data model

```
~/.abot/
  commit_email              one line — applied as repo-local user.email
  commit_name               one line — applied as repo-local user.name
  agents/
    alice.abot/             canonical git repo (alice's identity)
      .git/
      manifest.json         {name, version, created, updated}
      config.json           {shell, env, instructions, model}
      home/                 working directory
  kubos/
    daily-room/             a room
      alice/                git worktree on branch kubo/daily-room
        .git                file (not dir) pointing to alice.abot
        manifest.json
        config.json
        home/
```

`commit_email` and `commit_name` are file-per-fact settings (matching
humOS's "no god files" convention). If unset, abot falls back to the
user's git global identity. If neither is set, `abot create` errors with
a helpful pointer.

Each agent's `config.json` has `model` (default `gemma4:31b`),
`instructions` (the system prompt), `shell`, and `env`.

## Composition

abot pairs naturally with two siblings:

- **kubo** runs containers / rooms. Bind-mount
  `~/.abot/kubos/<room>/<name>/home/` into the container; commits
  autosave to `kubo/<room>` in the agent's canonical repo.
- **tao** suspends a pipeline on a human. abot agents and humans both
  implement the same actor interface — same pipe shape works for either.

```sh
echo "what's wrong with this PR?" \
  | abot run alice \
  | tao approve developer felix
```

## License

MIT OR Apache-2.0.
