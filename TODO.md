# TODO — Iterate Codebase Toward Documented Vision

The docs (CLAUDE.md, docs/) clearly describe the north star. These items
track where the codebase hasn't caught up yet.

## Blocking

- [x] **Unify session creation through the worktree model.**
      All session creation paths now go through `ensure_abot_in_kubo` which
      creates a canonical abot + worktree + kubo manifest entry. WS auto-create
      removed. Ctrl+N removed. First launch shows empty kubo onboarding.

## Confusing (code contradicts docs)

- [x] **Stop using `"main"` as the first session name.**
      Removed `"main"` fallback from `facet_shell.dart`. First launch shows
      empty kubo onboarding page instead of auto-creating a session.

- [ ] **Read kubo-level credentials.**
      Docs describe `credentials.json` at the kubo level as the standard, but
      `Kubo::start()` never reads it. Container starts with only base env vars.
      Kubo credentials should be read and injected into the container environment.

- [ ] **Migration should create proper worktrees.**
      `migrate_data_dir` creates empty stub dirs in the default kubo. The actual
      data lives in canonical repos under `abots/`, but sessions would use the
      empty kubo dirs. Migration should set up worktrees linking kubo dirs back
      to canonical repos.

- [x] **Fix `OpenBundle` split state.**
      `OpenBundle` now creates a worktree in the default kubo and points
      `bundle_path` at the worktree. Autosave writes to the correct location.

- [ ] **Remove vestigial `image` field from bundle manifest.**
      `save_bundle` writes `image: "abot-session"` to manifest.json but it's
      never read. Kubos determine their own image via `resolve_image`. Remove
      the dead field and update docs that reference it.

- [ ] **Remove or implement per-bundle resource limits.**
      `config.json` writes `memory_mb`, `cpu_percent`, `shell` but they're
      never read. All abots share one container with hardcoded limits. Either
      implement per-abot resource configuration or remove the dead fields and
      update docs.

## Minor

- [ ] **Remove `docker.rs` from docs/architecture.md module layout.**
      File was deleted; functionality absorbed into `kubo.rs` and `kubo_exec.rs`.

- [ ] **`abot-git status` should use `--porcelain` not `--short`.**
      Docs say `--porcelain` (machine-readable); code uses `--short`.

- [ ] **Add "remove abot from kubo" endpoint.**
      `worktree_remove_abot` exists in `bundle.rs` but no IPC message or REST
      endpoint calls it. Users cannot remove an abot from a kubo through any API.

- [ ] **Document that REST `POST /sessions` defaults kubo to `"default"`.**
      The REST layer silently fills in the default; API docs say kubo is required.

## Future (documented as not-this-PR)

- [x] Empty kubo onboarding landing page on main stage
- [ ] Abot visual identity (avatar, color, icon) across kubos
- [ ] Update indicator with human-language messaging
- [ ] AI-assisted merge conflict resolution
- [ ] Kubo sharing/import flow
