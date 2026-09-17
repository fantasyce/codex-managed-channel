# Version 0.2.0 Acceptance

The release candidate was validated from a disposable source copy with the
official app-server and a loopback mock provider. Test identities, homes,
configuration, state and logs were synthetic and temporary. This record retains
no machine identifier, address, username, filesystem path, key material,
fingerprint, task identifier, prompt, raw protocol message or process ID.

| Check | Result |
| --- | --- |
| Rust formatting | Pass |
| Rust unit and integration suite (68 tests) | Pass |
| Strict Clippy across all targets and features | Pass |
| Python installer, documentation, packaging and privacy suite (29 tests) | Pass |
| Shell syntax checks | Pass |
| Independent official-runtime lifecycle suite (19 scenarios) | Pass |
| Worktree and reachable Git-history privacy scan | Pass |
| Commit and tag metadata email policy | Pass |
| Native Apple silicon release build | Pass |
| Release archive manifest, checksum and privacy scan | Pass |

The lifecycle scenarios covered single-writer contention, same-worker reconnect,
identity isolation, detach expiry, absolute lifetime, cumulative detach budget,
offline completion, startup/owner/server/guardian failure, active-idle protection,
empty idle, in-place idle-thread unsubscribe, stubborn MCP shutdown, hard FD
stop, exact client stop, pending approval and pending user input. Every scenario
ended without forced test cleanup and with no remaining process owned by that
scenario.

The checks prove the documented lifecycle and packaging behavior under their
isolated conditions. They do not prove network availability, model quality,
third-party service availability, or exactly-once semantics for external tool
side effects. Version 0.2.0 also does not map descendant FD pressure to an
individual tool call; see the documented
[per-task FD isolation boundary](bounded-recovery.md#per-task-fd-isolation-is-not-implemented).
