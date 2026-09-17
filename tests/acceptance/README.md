# Independent entry acceptance

Run on a Mac with the official Codex CLI installed. Supply the newly built entry,
not an installed/in-use managed entry. `--codex` optionally selects the official
CLI; by default the script resolves it under the invoking user's local bin.

```sh
python3 tests/acceptance/entry_acceptance.py target/debug/codex-managed-entry --case reconnect
```

Cases: `contention`, `reconnect`, `isolation`, `expiry`, `lifetime`, `crash`,
`budget`, `offline`, `startup-crash`, `idle-active`, `idle-empty`, `hard-fd`,
`idle-thread`, `idle-thread-stubborn`, `server-crash`, `stop-isolation`,
`approval`, `user-input`, `guardian-crash`.

Each uses a separate temporary HOME and a loopback mock provider with the real
official app-server and proxy. It asserts nonce/protocol behavior, exact worker
and turn identity where appropriate, request counts, and natural resource
convergence. Any necessary test-forced cleanup is a failure, not a passing test.
The approval case explicitly cancels; it does not execute the requested command.

The first-generation entry failed contention and reconnect. Development faults
were also reproduced before fixing macOS accepted-socket flags, stdout flushing,
startup-owner death, reparented descendants, and independent guardian failure.

These tests do not install keys, change an active SSH alias, exercise a real
network blackhole, validate Desktop UI, or validate real remote/local tools.
Those remain deployment acceptance, not claims inferred from these tests.
