# Version 0.1.0 Acceptance

The acceptance environment used disposable aliases, keys, configuration, state,
and installation paths. All disposable artifacts were removed after validation.
No machine identifier, address, key fingerprint, task identifier, raw log, or
timestamp is retained in this record.

| Check | Result |
| --- | --- |
| Clean repository with one independent root | Pass |
| Rust unit and integration suite | Pass |
| Installer and uninstaller suite | Pass |
| Shell syntax and strict Rust linting | Pass |
| Worktree and reachable-history privacy scan | Pass |
| Native arm64 release build and archive checksum | Pass |
| Release archive manifest and privacy scan | Pass |
| Checksum verified installation | Pass |
| Repeated installation preserves key and configuration | Pass |
| Official app-server WebSocket initialization | Pass |
| Official Computer Use read-only tool call | Pass |
| Clean transport exit | Pass |
| Remaining managed process and socket count | Zero |
| Exact uninstall and disposable cleanup | Pass |

The acceptance did not switch, overwrite, or stop an existing remote channel.
