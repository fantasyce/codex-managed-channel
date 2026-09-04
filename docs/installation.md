# Installation

## Prerequisites

- local and remote macOS on Apple silicon or Intel;
- official ChatGPT Desktop and Codex installed and authenticated remotely;
- an administrative alias such as `example-host` that already succeeds with
  non-interactive SSH;
- `curl`, OpenSSH, `shasum`, and `tar` on the local Mac.

## Review-first installation

Download `install.sh`, `scripts/install-lib.sh`, the architecture release
archive, and `SHA256SUMS` from the same `v0.1.0` GitHub release. Inspect the two
scripts, verify the archive with `shasum -a 256`, then run:

```sh
./install.sh --remote example-host --alias example-managed \
  --repository OWNER/codex-managed-channel --version v0.1.0
```

The optional `--key PATH` reuses a dedicated Ed25519 key and its `.pub` file.
Do not pass a general-purpose personal key. `--repository OWNER/REPOSITORY`
selects a fork, and `--version VERSION` pins its release.

## What changes

On the client, the installer creates a dedicated key under
`/Users/user/.ssh/codex-managed-channel/` and one marked block in SSH config.
On the remote, it installs two binaries and an uninstall helper under the
current user's local library directory and appends one restricted, marked
authorization line. Both configuration files are backed up before replacement.

The installer validates aliases, resolved SSH settings, batch connectivity,
remote OS and CPU, checksums, bundle manifest, Desktop resources, Codex
app-server features, and plugin manifests before activating the managed alias.
Arbitrary `ProxyCommand` programs are rejected in version 0.1; `ProxyJump` is
preserved.

Repeated installation is idempotent. It neither changes the administrative
alias nor any existing managed alias.
