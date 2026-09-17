# Security Model

## Trust assumptions

The local user trusts the existing administrative SSH path, the selected GitHub
release, the remote macOS account, and the official software already installed
there. Compromise of any of those is outside this project's protection.

## Controls

- The dedicated private key stays local; only its public half is uploaded.
- The remote authorization forbids forwarding, agent forwarding, X11, PTY, and
  user rc files, and forces the managed entry executable.
- App-server listens on a per-connection Unix socket, never a network socket.
- Release assets and their internal full-file manifest are SHA-256 verified.
- Binary archives include the project MIT license plus the exact license and
  notice files for every Rust dependency resolved for that target.
- SSH and authorization edits use exact markers, backups, and atomic rename.
- Cleanup targets only a verified process group or exact installation marker.
- The privacy gate redacts matches and scans reachable history and archives.

## Non-goals

This project does not sandbox Codex tools, replace macOS permissions, secure a
compromised remote account, bypass ChatGPT authentication, or guarantee
compatibility with unpublished Desktop internals.
