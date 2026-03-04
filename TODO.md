# TODO — Iterate Codebase Toward Documented Vision

The docs (CLAUDE.md, docs/) clearly describe the north star. These items
track where the codebase hasn't caught up yet.

## Blocking

- [ ] **Unify session creation through the worktree model.**
      `CreateSession` (Ctrl+N, auto-create on attach, REST `POST /sessions`)
      creates a standalone dir inside the kubo — no canonical abot, no
      worktree, no `kubo/<name>` branch. Only `AddAbotToKubo` follows the
      proper flow. All session creation should go through the worktree model:
      every session IS an abot with a canonical repo and a worktree in its kubo.

## Confusing (code contradicts docs)

- [ ] **Stop using `"main"` as the first session name.**
      `facet_shell.dart` creates a session named `"main"` on first launch,
      directly contradicting the documented rule that session names must not
      collide with git's default branch.

- [ ] **Read kubo-level credentials.**
      Docs describe `credentials.json` at the kubo level as the standard, but
      `Kubo::start()` never reads it. Container starts with only base env vars.
      Kubo credentials should be read and injected into the container environment.

- [ ] **Migration should create proper worktrees.**
      `migrate_data_dir` creates empty stub dirs in the default kubo. The actual
      data lives in canonical repos under `abots/`, but sessions would use the
      empty kubo dirs. Migration should set up worktrees linking kubo dirs back
      to canonical repos.

- [ ] **Fix `OpenBundle` split state.**
      `OpenBundle` handler sets `bundle_path` to the original bundle location
      but the session filesystem lives inside the kubo dir. Autosave writes
      metadata to the original, not the kubo copy. Should create a worktree in
      the kubo and point `bundle_path` at it.

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

- [ ] Empty kubo onboarding landing page on main stage
- [ ] Abot visual identity (avatar, color, icon) across kubos
- [ ] Update indicator with human-language messaging
- [ ] AI-assisted merge conflict resolution
- [ ] Kubo sharing/import flow
