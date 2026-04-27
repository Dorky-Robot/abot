# Worktree Model Implementation — Scratchpad

## Status: CODE IMPLEMENTATION COMPLETE

All code changes are done. `cargo test` passes (65/65), `cargo check` clean, `flutter analyze` clean.

## What was done

### Session 1 (design + docs)

Redesigned how abots are incorporated into kubos. Replaced the git subtree
approach with **git worktrees**. All design decisions documented in:

- `CLAUDE.md` — Git Worktree Model section + UX Principles section
- `docs/concepts.md` — Git Worktree Model section (replaces old Subtree Scheme)
- `docs/` — all docs updated to match (architecture, features, config, api-ref, getting-started)

### Session 2 (code implementation)

Implemented the worktree model end-to-end:

**Rust daemon (`src/daemon/`):**
- `bundle.rs` — Removed 4 dead subtree functions. Added `resolve_abots_dir()`, `create_canonical_abot()`, `worktree_add_abot()`, `worktree_remove_abot()`. Updated `ensure_bundle_home` and `migrate_data_dir` to use `resolve_abots_dir`. Made `run_git` pub(crate).
- `kubo.rs` — Updated doc comment (kubos NOT git repos). `ensure_abot_home` skips git init when `.git` exists (handles worktree files).
- `ipc.rs` — `AddAbotToKubo` now takes `create_session`, `cols`, `rows`, `env`. Handler: validate → create canonical abot → worktree add → update manifest → optionally create session. `CloneAbot` and `AbotGit` use `resolve_abots_dir`.
- `mod.rs` — Autosave loop already handles worktree `.git` files (no changes needed).

**Rust server (`src/server/`):**
- `kubos.rs` — `add_abot_to_kubo` passes `createSession`, `cols`, `rows`, `env` to daemon RPC. Returns session name.

**Flutter client (`flutter_client/lib/`):**
- `kubo_service.dart` — Added `addAbotToKubo(kuboName, abotName, {createSession})`.
- `facet_manager.dart` — Added `createAbotInKubo(abotName, {kubo})` — calls kubo service, creates facet, attaches WS.
- `facet_shell.dart` — "+" on kubo now calls `_addAbotToKubo` → name dialog → `createAbotInKubo`. Added `_showNewAbotDialog`.

## Key design decisions

1. **Abot = git repo (the worker).** Kubo = infrastructure (the office). Kubo is NOT a git repo.
2. **Git worktrees** instead of subtrees. Each abot in a kubo is a `git worktree` on a `kubo/<kubo-name>` branch.
3. **Credentials belong to the kubo**, not the abot. Standalone abots keep their own credentials.
4. **Autosave** is just `git commit` in the worktree — commits land on the kubo branch automatically, no push needed.
5. **No hardcoded branch names** — `git init` uses system default (`init.defaultBranch`).
6. **Empty kubo** shows an onboarding landing page with "Add abot" / "Open abot" buttons.
7. **Session names must not be `main`** — collides with git default branch.

## Files with uncommitted changes

All changes are unstaged. The full list:
- `CLAUDE.md` — Git Worktree Model + UX Principles sections
- `docs/` — 8 doc files updated for worktree model
- `src/daemon/bundle.rs` — subtree→worktree, resolve_abots_dir, create_canonical_abot
- `src/daemon/kubo.rs` — worktree-aware ensure_abot_home
- `src/daemon/ipc.rs` — enhanced AddAbotToKubo handler
- `src/daemon/mod.rs` — pre-existing changes (not from this work)
- `src/daemon/session.rs` — pre-existing changes
- `src/daemon/docker.rs` — deleted (pre-existing)
- `src/server/kubos.rs` — pass new fields to daemon RPC
- `src/server/sessions.rs` — pre-existing changes
- `src/stream/handler.rs` — pre-existing changes
- `flutter_client/lib/core/network/kubo_service.dart` — addAbotToKubo
- `flutter_client/lib/core/network/session_service.dart` — pre-existing changes
- `flutter_client/lib/features/facet/facet_manager.dart` — createAbotInKubo
- `flutter_client/lib/features/facet/facet_shell.dart` — name dialog + _addAbotToKubo

## What's next (not this PR)

- Kubo-level credentials reading (read `credentials.json` from kubo path, inject into session env)
- Abot visual identity (avatar, color, icon) across kubos
- Update indicator with human-language messaging
- AI-assisted merge conflict resolution
- Empty kubo onboarding landing page on main stage
- Kubo sharing/import flow

## Verification plan

1. `cargo test` — all Rust tests pass ✅ (65/65)
2. `cargo check` — 3 warnings (unused functions for future use) ✅
3. `flutter analyze` — no issues ✅
4. `flutter build web --wasm` — needs manual verification
5. Manual: click "+" on kubo → name dialog → creates canonical abot + worktree + terminal
6. Check `~/.abot/abots/{name}.abot/.git` exists (canonical git repo, directory)
7. Check `~/.abot/kubos/{kubo}.kubo/{name}/.git` exists (worktree, file not directory)
8. Check `git worktree list` in canonical abot shows the kubo worktree
9. Check `git branch` in canonical abot shows `kubo/<kubo-name>` branch
