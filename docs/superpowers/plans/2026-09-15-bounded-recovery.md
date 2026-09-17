# Bounded session recovery implementation plan

> Execute inline with test-first checks; independent acceptance is required before declaring completion.

**Goal:** Preserve per-client isolation and bounded resource ownership while reconnecting a surviving official turn.
**Architecture:** A server-configured `--client-id` selects a private, single-writer session owner. The owner starts an isolated official app-server, creates a fresh official proxy per attachment, retains deadlines across attachments, and reaps the worker on expiry. No task replay or synthetic continuation.
**Tech Stack:** Rust, Unix sockets, flock, official app-server, existing managed-home and cleanup helpers.
**Spec:** User-approved option 1, 2026-09-15 preflight report.

## Constraints

- No installed-service changes, key changes, or modifications to official Codex.
- Fixed forced-command client identity; no client-selected runtime namespace.
- Original no-argument entry remains compatible.
- Preserve nonce and protocol bytes, restricted bootstrap parsing, FD 160/192/240 thresholds, TERM/KILL descendant cleanup, account/history/plugins.
- Unknown runtime state never authorizes idle recycling; hard resource limits still apply.
- Reconnect must not renew worker birth time or accumulated detached time.

## Tasks

- [x] Protocol: carry pending turn thread identity, track `(thread, turn)` sets and terminal tombstones, fail closed for identity-less events. Files: `src/protocol.rs`, `tests/protocol.rs`, `tests/recovery_preflight.rs`. Run three known failures first, then add duplicate/late-response/unknown-identity tests and pass them.
- [x] Session transport: add `src/session.rs`, fixed `--client-id` dispatch in entry, reuse worker setup from supervisor. Start with process tests proving second attach rejection, reconnect same worker, expiry and separate IDs. Private directories and nonblocking flock serialize owner startup. Per-attachment proxy teardown must finish before granting another writer.
- [x] Reclaim safety: add bounded read-only loaded-thread state query in `src/takeover.rs`; final idle decision compares observer activity generation after querying. Hard FD protection remains independent of reliable parsing. Test active and unknown states cannot authorize recycle.
- [x] Lifecycle: absolute worker deadline and accumulated detach budget, fresh EOF lease bounded by both; bounded I/O and cleanup on error. Metadata-only serialized logging. Tests cover owner/proxy exit, refusal during drain, no orphaned proxy threads, hard FD stop.
- [x] Operational integration: document one key per client, fixed forced command, explicit keepalive limits, rollout/rollback and lease-expiry behavior. Document Unix-socket worker identification for monitoring; the installed private monitor is deliberately untouched.
- [x] Independent acceptance: run full Rust and existing Python suites, formatting/lints, independent real official runtime with mock provider, concurrency and resource convergence tests. Report untested live Desktop/real model/tool boundaries separately. Do not install or claim live acceptance.

## Commands

Development copy: local `work/repo`; remote isolated mirror `/tmp/managed-recovery-development`.

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
python3 -m unittest discover -s tests -p 'test_*.py'
```

Regression acceptance requires `unrelated_child_completion_does_not_clear_parent`, `resumed_in_progress_turn_is_active`, and `server_started_turn_is_active_without_client_start` to change from failure to pass without changing their assertions.
