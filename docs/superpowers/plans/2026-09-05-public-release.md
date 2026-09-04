# Privacy-Safe Public Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build, validate, publish, and promote a new-history public release of `codex-managed-channel` with a checksum-verified quick installer and no development-machine information.

**Architecture:** A clean repository receives generic runtime files through an allowlist and newly written public artifacts. A small POSIX-shell bootstrap downloads a signed-by-GitHub, SHA-256-verified architecture bundle; the bundle generates a dedicated local SSH key, installs the restricted remote entry through an existing administrative alias, writes exact marked SSH blocks, and verifies the managed handshake. Privacy gates scan the worktree, reachable Git objects, and release archives before any public push or community post.

**Tech Stack:** Rust 2024, POSIX shell available on macOS, Python 3 standard-library test harness, OpenSSH, GitHub Actions, GitHub CLI.

**Spec:** `docs/superpowers/specs/2026-09-05-public-release-design.md`

## Global Constraints

- The public repository must be a new `.git` repository with no parent commit or remote relationship to the private development repository.
- No real machine identifier, network identifier, key material, conversation identifier, project identifier, credential, proxy detail, or raw incident log may enter a public file, Git object, workflow log, release archive, or community post.
- A generated Ed25519 private key never leaves the installing user's local Mac; only its public key crosses the administrative SSH connection.
- The installer must preserve existing SSH configuration and `authorized_keys`, use exact markers, back up before mutation, be idempotent, and fail before mutation on ambiguity.
- The runtime continues to launch the official Codex app-server; it does not fork or expose app-server on a public listener.
- Version 0.1 supports local macOS, remote macOS, arm64 and x86_64, and an already working administrative SSH alias.
- Existing managed aliases, shared app-server daemons, Codex history, ChatGPT authentication, network software, and unrelated processes must remain unchanged during disposable acceptance.
- Public push, tag, release, and community publication happen only after local, remote, history, and archive privacy scans pass.

---

### Task 1: Create the clean repository and privacy gate

**Files:**
- Create: `.gitignore`
- Create: `scripts/privacy-scan.sh`
- Create: `tests/test_privacy_scan.py`
- Copy by allowlist: `Cargo.toml`, `Cargo.lock`, `src/**`, `tests/*.rs`

**Interfaces:**
- Consumes: private runtime files only through explicit path arguments to a one-time copy command; never consumes `.git`, `docs`, `tools`, `target`, or local configuration.
- Produces: `scripts/privacy-scan.sh [--history] [--archive PATH] [--extra-patterns PATH]` with exit code 0 only when the selected content is safe.

- [ ] **Step 1: Write the failing privacy tests**

  Create Python tests that build temporary repositories at runtime and assert rejection of a non-placeholder macOS home path, IPv4 literal, thread-like identifier, private-key block, SSH public-key payload, credential prefix, and a forbidden value supplied by an external pattern file. Assert acceptance of `example-host`, `example-managed`, `/Users/user`, official OpenAI/GitHub URLs, and plain Ed25519 algorithm documentation.

- [ ] **Step 2: Verify RED**

  Run `python3 -m unittest tests/test_privacy_scan.py` and confirm failure because `scripts/privacy-scan.sh` does not exist.

- [ ] **Step 3: Implement the privacy scanner**

  Scan regular files without printing matching content. Report only relative path, rule name, and line number. Support worktree scanning, `git rev-list --objects --all` reachable-history scanning through temporary extraction, tar archive scanning, and newline-delimited literal extra patterns held outside the repository. Exclude `.git` internals and build output, but never exclude committed tests or documentation.

- [ ] **Step 4: Verify GREEN and import generic runtime files**

  Run the focused tests, copy only the declared runtime allowlist, replace machine-specific test/tool defaults with neutral temporary paths, dynamically build marketplace catalogs from installed plugin manifests, and run the scanner with a private external literal list. Confirm no old `.git` directory exists.

- [ ] **Step 5: Initialize new history and commit**

  Run `git init -b main`, set repository-local generic release authorship if needed, verify `git rev-list --max-parents=0 HEAD` will contain exactly one root after commit, and commit as `chore: initialize privacy-safe source tree`.

### Task 2: Make the runtime distribution-generic

**Files:**
- Modify: `src/managed_home.rs`
- Modify: `src/supervisor.rs`
- Modify: `tests/managed_home.rs`
- Modify: `tests/supervisor_cleanup.rs`
- Create: `docs/compatibility.md`

**Interfaces:**
- Consumes: `HOME`, `CODEX_HOME`, `CODEX_MANAGED_CODEX_BIN`, and installed official Desktop resource paths.
- Produces: a runtime with no development-user defaults and a documented macOS/architecture/version support boundary.

- [ ] **Step 1: Write failing portability tests**

  Add tests proving all per-user state derives from the supplied home directory, no source/runtime default contains a development username or host path, and official Desktop resource lookup returns a bounded error when no supported application bundle exists.

- [ ] **Step 2: Verify RED**

  Run the exact new Rust tests and confirm the current hard-coded resource assumption or missing validation produces the expected failure.

- [ ] **Step 3: Implement minimal generic discovery**

  Resolve official Desktop resources from `CODEX_MANAGED_DESKTOP_RESOURCES` when set, otherwise from the supported standard macOS application locations. Validate required `codex`, `cua_node`, and plugin files before writing managed configuration. Keep all user state relative to the runtime home.

- [ ] **Step 4: Verify GREEN and regression suite**

  Run the focused tests, `cargo test --locked`, `cargo fmt --all -- --check`, and `cargo clippy --locked --all-targets --all-features -- -D warnings`.

- [ ] **Step 5: Commit**

  Commit as `refactor: make runtime discovery distribution-safe`.

### Task 3: Build the local quick installer and exact SSH configuration editor

**Files:**
- Create: `install.sh`
- Create: `scripts/install-lib.sh`
- Create: `tests/test_install.py`

**Interfaces:**
- Consumes: `--remote ADMIN_ALIAS`, `--alias MANAGED_ALIAS`, optional `--version`, optional `--key PATH`, optional `--repository OWNER/REPOSITORY`, and environment-injected command paths used only by tests.
- Produces: a dedicated key under `${HOME}/.ssh/codex-managed-channel/`, one exact marked block in `${HOME}/.ssh/config`, and a verified remote install.

- [ ] **Step 1: Write failing argument and no-mutation tests**

  Test invalid aliases, equal administrative/managed aliases, an alias already defined outside the managed marker, missing commands, non-macOS, failed administrative SSH, unsupported `proxycommand`, and failed checksum. For every failure, assert SSH config, key directory, and fake remote state are unchanged.

- [ ] **Step 2: Verify RED**

  Run `python3 -m unittest tests/test_install.py` and confirm failure because the installer interface is absent.

- [ ] **Step 3: Implement validation and resolved SSH policy**

  Parse arguments without `eval`, accept aliases matching `[A-Za-z0-9._-]+`, resolve `hostname`, `user`, `port`, `proxyjump`, and known-host settings through `ssh -G`, reject unsupported non-`none` proxy commands in version 0.1, and perform a batch administrative SSH probe before mutation.

- [ ] **Step 4: Implement verified bundle acquisition**

  Map `arm64` to `aarch64-apple-darwin` and `x86_64` to `x86_64-apple-darwin`, download the selected release tarball and `SHA256SUMS` into `mktemp -d`, verify with `shasum -a 256`, extract only after verification, and use traps to clean local and remote temporary directories.

- [ ] **Step 5: Implement key and config mutation**

  Generate a no-passphrase dedicated Ed25519 key only when absent, chmod the private directory and file, never print private content, back up SSH config, and atomically replace exactly one `# BEGIN codex-managed-channel ALIAS` through `# END codex-managed-channel ALIAS` block. Repeated installation must produce byte-identical configuration and no duplicate key.

- [ ] **Step 6: Verify GREEN**

  Run all installer tests twice, including idempotency and simulated interruption, and run `sh -n install.sh scripts/install-lib.sh`.

- [ ] **Step 7: Commit**

  Commit as `feat: add checksum-verified quick installer`.

### Task 4: Build the remote binary installer and safe uninstaller

**Files:**
- Create: `scripts/install-remote.sh`
- Create: `uninstall.sh`
- Create: `scripts/uninstall-remote.sh`
- Create: `tests/test_uninstall.py`

**Interfaces:**
- Consumes: verified release directory, one public-key file, selected alias marker, and an administrative SSH alias.
- Produces: atomically installed entry/preflight binaries, one exact restricted `authorized_keys` line, reversible backups, and exact local/remote removal.

- [ ] **Step 1: Write failing remote-install and uninstall tests**

  Use temporary home directories to prove malformed keys are rejected, a valid key is added once, existing authorization lines survive byte-for-byte, reinstall is idempotent, uninstall removes only the matching marker, default uninstall retains logs, and purge requires the literal confirmation `purge`.

- [ ] **Step 2: Verify RED**

  Run `python3 -m unittest tests/test_uninstall.py` and confirm failure because the scripts are absent.

- [ ] **Step 3: Implement remote install**

  Validate release metadata, executable hashes, app-server/proxy capability, and plugin manifests before mutation. Install with `.new` plus rename, back up `authorized_keys`, and append a forced command with `no-port-forwarding,no-X11-forwarding,no-agent-forwarding,no-pty,no-user-rc` and an alias-derived nonsecret marker.

- [ ] **Step 4: Implement exact uninstall**

  Back up files, remove only exact marked blocks and authorization entries, stop only descendants whose executable path matches the installed entry, remove sockets and binaries, retain state by default, and move an installer-generated dedicated key to the local Trash when its public half matches the installed marker.

- [ ] **Step 5: Verify GREEN**

  Run remote-install and uninstall tests, all installer tests, and `sh -n` on every shell script.

- [ ] **Step 6: Commit**

  Commit as `feat: add reversible remote installation`.

### Task 5: Add public documentation, license, metadata, and CI

**Files:**
- Create: `README.md`
- Create: `README.zh-CN.md`
- Create: `LICENSE`
- Create: `SECURITY.md`
- Create: `PRIVACY.md`
- Create: `CONTRIBUTING.md`
- Create: `CHANGELOG.md`
- Create: `docs/architecture.md`
- Create: `docs/installation.md`
- Create: `docs/security-model.md`
- Create: `docs/troubleshooting.md`
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml`
- Create: `.github/ISSUE_TEMPLATE/bug_report.yml`

**Interfaces:**
- Consumes: tested command-line interfaces from Tasks 1–4.
- Produces: complete public user, contributor, privacy, security, compatibility, support, CI, and release documentation.

- [ ] **Step 1: Write documentation contract tests**

  Add a Python test that asserts every required file exists, every documented flag appears in installer help, the unofficial disclaimer and no-telemetry statement exist, all quick-start commands use neutral aliases, and no unsupported capability is promised.

- [ ] **Step 2: Verify RED**

  Run the documentation contract test and confirm missing public files cause failure.

- [ ] **Step 3: Write public documentation and legal files**

  Use English as the primary README and provide an equivalent Chinese quick start. Document prerequisites, safe download path, one-line path, architecture, lifecycle behavior, key boundaries, no telemetry, uninstall, compatibility matrix, fail-closed behavior, and the unofficial-project disclaimer. Add the full MIT license text.

- [ ] **Step 4: Add CI and release workflows**

  CI runs formatting, Rust tests, clippy, Python installer tests, shell syntax checks, and privacy scan. Release builds architecture-specific macOS binaries on native runners when available, creates tarballs with a deterministic manifest, writes `SHA256SUMS`, rescans archives, and uploads artifacts only for `v*` tags.

- [ ] **Step 5: Verify GREEN and commit**

  Run documentation tests, privacy scan, workflow syntax inspection, and the full local suite. Commit as `docs: prepare public project and release automation`.

### Task 6: Package and perform private disposable acceptance

**Files:**
- Create: `scripts/package-release.sh`
- Create: `tools/managed-handshake-probe.py`
- Create: `docs/acceptance-template.md`

**Interfaces:**
- Consumes: a clean commit, release version, target triple, administrative SSH alias supplied only at runtime, and a disposable managed alias supplied only at runtime.
- Produces: checksum-verified release archives and a redacted pass/fail acceptance record without host identifiers.

- [ ] **Step 1: Write failing packaging tests**

  Assert the archive contains only the manifest allowlist, executable modes are preserved, manifest hashes match, version and target metadata match filenames, and packaging refuses a dirty tree or privacy-scan failure.

- [ ] **Step 2: Verify RED**

  Run the packaging tests and confirm failure because the packaging interface is absent.

- [ ] **Step 3: Implement packaging and generic handshake probe**

  Package only binaries, installer runtime, license, privacy/security summaries, and metadata. The probe emits booleans and counts only; it must not print hostnames, paths, thread IDs, prompts, tokens, process IDs, or raw protocol messages.

- [ ] **Step 4: Verify locally and on a disposable alias**

  Build a local release bundle, scan it, install through a newly generated disposable alias, prove a real app-server initialize/resume-or-start handshake, archive reclamation, browser, and Computer Use, then uninstall. Confirm the disposable local key/config block, remote key line, processes, sockets, and test thread are absent afterward and the existing production alias still performs a read-only handshake.

- [ ] **Step 5: Commit**

  Commit as `test: add release packaging and acceptance gates`.

### Task 7: Create and verify the public GitHub release

**Files:**
- Modify: public repository metadata only through GitHub CLI/API.
- Create locally outside Git: a private extra-pattern file containing development identifiers for the final scan.

**Interfaces:**
- Consumes: authenticated `gh`, clean `main`, available repository name `codex-managed-channel`, and the private external scan pattern file.
- Produces: public repository, protected/default `main`, tag `v0.1.0`, release assets, and stable quick-start URLs.

- [ ] **Step 1: Prove account and name without mutation**

  Run `gh auth status`, resolve the authenticated login through `gh api user`, and query repository availability. Stop if the requested name exists and is not this new project.

- [ ] **Step 2: Run pre-publication privacy gates**

  Scan the working tree with generic and private external patterns, create the root commit if not already created, scan every reachable Git object, build release archives, scan extracted archives, and verify no private source repository remote or object ID appears.

- [ ] **Step 3: Create and push the public repository**

  Create exactly `LOGIN/codex-managed-channel` as public with an unofficial description, add it as the only remote, push only `main`, and verify the remote contains exactly the intended new root history.

- [ ] **Step 4: Publish and verify `v0.1.0`**

  Create an annotated tag, push it, wait for CI/release workflows, download release assets into a fresh temporary directory, verify checksums and privacy scan, and run the documented installer from the public URL through another disposable alias followed by exact uninstall.

- [ ] **Step 5: Record stable URLs**

  Add only the public repository and release URLs to the private implementation record. Do not copy local authentication or machine details.

### Task 8: Publish the OpenAI Codex community discussion

**Files:**
- Create locally outside Git: a redacted English discussion draft.

**Interfaces:**
- Consumes: verified public repository, release, security, compatibility, and installer URLs.
- Produces: one public `openai/codex` Discussion URL.

- [ ] **Step 1: Draft and privacy-scan the post**

  Describe the generic lifecycle problem, anonymized aggregate FD/child-process evidence, architecture, boundaries, release link, and request for feedback. State that the project is unofficial, wraps rather than forks app-server, and does not prove every observed symptom is an upstream bug.

- [ ] **Step 2: Resolve the official discussion category**

  Query `openai/codex` repository and discussion-category IDs through authenticated GitHub GraphQL without mutation. Select the general ideas/show-and-tell category intended for community projects.

- [ ] **Step 3: Publish and verify**

  Create one discussion, fetch it back anonymously, verify rendered links and text, run the privacy scanner against the fetched body, and record its public URL in the private implementation record.

- [ ] **Step 4: Final system verification and cleanup**

  Re-run the public CI-equivalent suite, verify the public tag and assets, verify no disposable installations remain, confirm the original managed alias and unrelated services remain healthy, remove local/remote staging artifacts that are no longer useful, and retain the public working clone plus private implementation record.
