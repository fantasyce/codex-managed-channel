# Privacy

Codex Managed Channel has no telemetry, analytics, crash upload, or external
control plane. Runtime logs remain on the remote Mac under its managed state
directory and contain lifecycle counters, reasons, and process identifiers.

The installer sends only the generated public key and verified release bundle
through the administrative SSH connection. It never reads or transmits the
private key. Existing ChatGPT authentication remains in the user's Codex home
and is not copied into this repository or a release archive.

Before release, the project scans the worktree, reachable Git history, and
release archives for credential and identity patterns. Maintainers may add a
private, external literal list; that list must never be committed.
