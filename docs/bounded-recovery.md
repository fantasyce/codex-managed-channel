# Bounded recovery (unreleased)

This development version adds opt-in per-client sessions. It does not patch the
official Codex binary or turn SSH into a shared, permanent app-server.

## Identity and ownership

Use a separate dedicated key and alias for **each client device**. Do not copy
that key to another Desktop. The remote installer assigns a separate state root
per alias and fixes `--client-id desktop` in its restricted forced command. The
installer rejects reuse of a key already registered under another authorization.
Manually managed entries may choose a 1–32 character alphanumeric, dash or
underscore client ID; it must be fixed by the server, never taken from the
original SSH command or a client-supplied environment variable.

An existing active connection wins: a second connection is rejected, not allowed
to steal control. After the previous connection and proxy have finished closing,
the same identity may attach to the surviving isolated runtime. Different
identities never select the same runtime. Authentication/history/plugin data
remain the existing user's data; this is not an isolation boundary against that
OS user's administrative shell.

```text
fixed identity -> one session owner -> independent guardian -> official worker
                         |
                         +-> at most one attached official proxy
                              (replaced on reconnect, worker retained)
```

The guardian holds the ownership lock before starting the worker. If the owner
dies during startup or normal execution, its pipe closes and the guardian reaps
the worker. Another owner cannot start before cleanup releases the lock.

## Deadlines and resources

| Setting | Default | Meaning |
|---|---:|---|
| `CODEX_MANAGED_EOF_GRACE_SECS` | 45 | Maximum detached interval |
| `CODEX_MANAGED_DETACH_BUDGET_SECS` | 300 | Cumulative detached time across this worker's life |
| `CODEX_MANAGED_SESSION_MAX_SECS` | 86400 | Absolute worker lifetime; reconnect cannot renew it |
| `CODEX_MANAGED_TERM_GRACE_SECS` | 5 | Graceful TERM before forced cleanup |
| `CODEX_MANAGED_THREAD_UNLOAD_SECS` | 2 | Official app-server delay after the last subscriber leaves |
| `CODEX_MANAGED_THREAD_RECLAIM_WAIT_SECS` | 10 | Maximum wait for `thread/closed` before logging and retrying later |
| FD warn / idle recycle / hard limit | 160 / 192 / 240 | Existing resource thresholds |

The session-age and detach-budget settings must be positive and at most seven
days. Expiry and hard FD limits may terminate active work: bounded resource
ownership takes precedence over unlimited execution. These limits are not
unlimited keepalive knobs. A task needing more than the configured absolute
lifetime must be planned accordingly. Closing Desktop also starts the finite
detach lease; it does not leave permanent MCP runtimes behind.

Turn observation uses thread/turn identities, counts server-started and resumed
work, tolerates duplicate terminal events, and treats missing identity as
unreliable. Pending command execution also protects against idle reclamation.
Before idle or soft-FD reclamation, client forwarding is gated and the full
official loaded-thread inventory is checked. Only explicit `idle` statuses and
a complete successful query authorize that check; active, waiting for approval,
waiting for input, unknown statuses and timeouts do not. The check uses a shared
absolute deadline and bounded responses. The owner then sends
`thread/unsubscribe` for each known idle thread over the existing WebSocket and
waits for `thread/closed`. Successful unload keeps the app-server, proxy and SSH
connection alive while releasing thread-scoped MCP children, pipes and file
descriptors. An empty idle connection is a no-op. A failed write or unload
timeout is logged and leaves the connection intact for a later retry. A soft FD
recycle still falls back to bounded disconnect cleanup when unsubscribe fails or
the post-unload count remains above the recycle threshold. Hard FD limits remain
independent and always stop the owned process group.

The guardian records observed descendant PID/start-time identities while the
worker is alive. This lets cleanup find a previously observed `setsid` child
after its server dies and it is reparented. This polling is not a kernel process
container: a child that forks, escapes and is orphaned entirely between samples
can remain unobserved. Simultaneous SIGKILL of owner and guardian, machine crash,
and deliberately adversarial process escapes are not guaranteed recovery cases.
Do not advertise this as perfect process containment.

## Recovery semantics

Reconnection sends no additional `turn/start`, no synthetic “continue” message,
no automatic approval, and no replay of tool side effects. It reconnects the
official proxy; Desktop's normal `thread/resume`/read/subscription flow obtains
the surviving turn. A turn completed offline remains completed. A runtime that
has been reclaimed cannot be resumed in memory: subsequent attachment starts a
fresh isolated runtime, and persisted history remains available. Never present
that as the original execution still running.

Pending approvals must still be decided by the user. A transport reconnect does
not authorize accepting an approval or retrying an uncertain external action.

## Preventing avoidable disconnects

The generated alias uses `ServerAliveInterval 15`, `ServerAliveCountMax 3`,
`ControlMaster no`, and `ControlPath none`. These provide SSH liveness detection
and prevent connection multiplexing from blurring client ownership. They do not
eliminate Wi-Fi outages, sleep, network blackholes, server crashes or provider
stream failures. They do not modify global sshd, sleep or proxy settings.

Inspect `ssh -G ALIAS` during rollout: earlier wildcard blocks or included SSH
configuration can take precedence over generated settings. Do not claim those
settings are effective from the generated text alone.

Recovery begins when the old remote connection is known to be gone. A half-open
connection may initially cause the new connection to be rejected. This is
intentional single-writer safety, not permission to take over a possibly live
connection. Validate real SSH liveness timing during deployment acceptance.

## Maintenance and diagnostics

The no-argument entry remains the legacy per-connection mode for compatibility;
bounded sessions require the fixed `--client-id` option. Existing installations
are not silently switched by rebuilding the code.

Local administrative maintenance can stop exactly one identity:

```sh
CODEX_MANAGED_ROOT=/absolute/private/state \
  /absolute/path/codex-managed-entry --stop-client desktop
```

This operation is rejected through an SSH original-command invocation, waits for
the cleanup lock, and does not stop a different identity. The installer-owned
uninstaller uses it before removing a bounded session's installation. Do not
purge state or remove the binary while cleanup is still pending.

`log/supervisor.jsonl` records metadata only: worker start/exit, attached/detached,
refused active attachment, lease expiry, absolute lifetime, FD warnings/stops,
unsubscribe completion/timeout/fallback and forwarding OS error codes. Concurrent records and rotation use
a common lock; the active log and one rotated predecessor are each about 1 MiB.
No task text, raw protocol or credentials are journaled. Real workers use
`app-server ... --listen unix://...`; do not count only `--stdio` processes when
checking FD/PIPE/descendant convergence.

## Validation and rollout boundary

Run the Rust suite, strict clippy/format checks, Python installer tests, and the
independent entry acceptance script under `tests/acceptance`. It uses an isolated
HOME and a loopback mock provider with the real official runtime. That proves
protocol/lifecycle behavior, not real-model quality or live Desktop UI behavior.

Deploy only in a separate acceptance alias/key first. Exercise actual Desktop
reconnect, approval UI, remote tools and local Computer Use, then verify resource
convergence after closing it. Do not replace an in-use installed entry to claim
acceptance. Preserve the administrative alias and previous release for rollback;
stop the acceptance identity and restore only its forced command/config block.

This feature is not in the previously published v0.1.0 release. Build and validate
this development source before choosing a new release version.
