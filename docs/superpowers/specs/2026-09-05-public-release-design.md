# Public Release Design

## Status

Approved in principle on 2026-09-05. This specification is the implementation
gate for producing and publishing the first public release.

## Goal

Publish an unofficial, reusable macOS component that lets Codex Desktop launch
an independently supervised Codex app-server on a remote Mac through SSH. The
component must preserve the remote host's normal Codex authentication, persisted
threads, skills, plugins, MCP servers, browser integration, and Computer Use
while bounding each SSH connection's process and file-descriptor lifetime.

The public release must contain no information that identifies either machine
used during development or any private project, account, network, task, or
credential.

## Non-negotiable privacy requirements

The public working tree, release archives, workflow logs, Git history, issue
templates, discussion post, screenshots, test fixtures, and generated metadata
must not contain:

- real hostnames, IP addresses, SSH aliases, ports, usernames, or home paths;
- private or public personal SSH keys, key fingerprints, `known_hosts` entries,
  or real `authorized_keys` lines;
- GitHub, ChatGPT, Codex, API, proxy, or subscription credentials;
- conversation IDs, task IDs, project names, application names, process IDs,
  timestamps, or raw incident logs from development;
- proxy providers, VPN/VPS details, local network topology, or subscription
  URLs;
- Git authorship metadata copied from the private repository's history.

Ed25519 is the selected key algorithm, not a secret. A generated Ed25519 private
key is secret and must never leave the installing user's local machine. Its
public key may be sent only to the selected remote host during installation and
must never be embedded in the repository or release assets.

## Repository separation

The private development repository remains a local incident and acceptance
archive. It is never assigned as the source of the public GitHub repository and
none of its Git refs are pushed.

A separate directory is created as a new repository. Files enter it only through
an explicit allowlist:

- generic Rust source and tests;
- generic marketplace templates required by the runtime;
- newly written public installation, uninstallation, verification, and release
  scripts;
- newly written public documentation and project metadata.

Private documentation, acceptance reports, logs, build outputs, `.git`, and all
machine-specific defaults are excluded. The public repository starts with a new
root commit so private history is not reachable from any published ref.

After publication, the public repository is the source of truth for generic
product code. The private repository remains the source of truth only for local
incident evidence and machine-specific acceptance records.

## Public product scope

Version 0.1 supports:

- a local Mac running Codex Desktop;
- a remote Mac already reachable through a working administrative SSH alias;
- remote Apple Silicon and Intel architectures;
- an installed and authenticated Codex CLI on the remote host;
- explicit SSH aliases with ordinary hostname, user, and port resolution;
- one managed alias per installation;
- per-connection app-server isolation, lifecycle observation, idle and archive
  reclamation, process-tree cleanup, file-descriptor guards, and safe idle-thread
  takeover.

The installer must fail closed when it encounters an unsupported SSH topology,
missing Codex capability, ambiguous configuration, failed checksum, failed
preflight, or unsafe existing marker. It must not guess or weaken SSH security.

Version 0.1 does not promise automatic support for arbitrary `ProxyCommand`,
multi-hop enterprise bastions, non-macOS remote hosts, Codex Cloud, or direct
public app-server listeners. Unsupported cases receive a bounded manual setup
guide without partial mutation.

## Runtime architecture

The runtime keeps OpenAI's app-server unchanged:

```text
Codex Desktop
  -> OpenSSH managed alias
  -> remote sshd forced-command
  -> codex-managed-entry
  -> official codex app-server proxy
  -> per-connection Unix socket
  -> official codex app-server
  -> remote skills, plugins, MCP, browser, and Computer Use
```

The managed entry validates only the fixed Desktop bootstrap actions and nonce.
It never evaluates arbitrary `SSH_ORIGINAL_COMMAND` text. It creates a sanitized
runtime home, starts one isolated official app-server process group, forwards
protocol bytes unchanged, observes only lifecycle metadata, and reclaims the
entire descendant tree on archive, idle timeout, EOF, or file-descriptor limits.

No app-server port is exposed to a public or shared network.

## Installation experience

The primary quick-start interface is:

```sh
curl -fsSL https://raw.githubusercontent.com/OWNER/codex-managed-channel/main/install.sh \
  | sh -s -- --remote example-host --alias example-managed
```

The safer documented path downloads the installer and checksum, verifies both,
allows inspection, and then runs the same arguments.

`--remote` names an already working administrative SSH alias. `--alias` names
the new restricted Codex alias. Optional flags allow an existing dedicated key,
a release version, a Codex binary override, and non-interactive operation. No
flag accepts a private key value.

The local installer performs these ordered phases:

1. Validate arguments, supported local OS, required commands, and marker safety.
2. Resolve the administrative alias with `ssh -G` and reject unsupported or
   ambiguous routing before modifying files.
3. Prove non-interactive administrative SSH access.
4. Query only the remote OS, architecture, and required Codex capabilities.
5. Download the matching versioned release bundle and `SHA256SUMS` over HTTPS.
6. Verify the asset checksum before extracting or executing it.
7. Generate a dedicated local Ed25519 key with restrictive permissions when no
   key was supplied. The private key is never printed or transmitted.
8. Upload the verified bundle and public key to a newly created remote temporary
   directory.
9. Run remote preflight without changing the real Codex configuration.
10. Back up and atomically add one exact forced-command entry to remote
    `authorized_keys`.
11. Back up and atomically add one marked block to local SSH config.
12. Verify the restricted bootstrap and app-server handshake through the new
    alias.
13. Remove all temporary local and remote artifacts.
14. Print the one remaining UI action: select the explicit alias in Codex
    Desktop. A Desktop restart is suggested only if the alias is not refreshed.

Every mutation is idempotent. A repeated install with the same alias and key
updates binaries and validates configuration without duplicating SSH entries.

## SSH configuration policy

The installer obtains hostname, user, port, host-key files, and routing from the
resolved administrative alias. It copies only a reviewed safe subset into a
new marked block. It always sets:

- a dedicated `IdentityFile`;
- `IdentitiesOnly yes`;
- `StrictHostKeyChecking yes`;
- bounded keepalive settings;
- the resolved user, host, and port.

Existing user blocks are never edited. The installer writes only between exact
begin/end markers for the selected managed alias. If the alias already exists
outside its marker or the resolved route uses unsupported directives, install
stops before mutation and prints a minimal manual block with placeholders.

## Remote installation policy

The release bundle contains architecture-specific entry and preflight binaries,
the bounded marketplace preparation script, license, and release metadata. The
remote installer:

- verifies bundle metadata and executable hashes;
- validates the installed Codex CLI and required app-server/proxy flags;
- validates the required official plugin manifests without copying credentials;
- installs through `.new` files followed by atomic rename;
- backs up `authorized_keys` before change;
- accepts exactly one Ed25519 public key with the expected generic comment;
- adds restrictive forced-command options;
- does not restart the Mac, Codex Desktop, the shared app-server daemon, network
  software, or unrelated processes.

## Uninstallation and recovery

The public uninstaller takes the same `--remote` and `--alias` arguments. It:

- validates exact local and remote markers;
- backs up both SSH configuration files;
- removes only the selected managed block and public-key entry;
- terminates only processes provably launched from the installed managed entry;
- removes current binaries and transient sockets;
- retains logs and state by default;
- moves the dedicated local key to the user's Trash when safe;
- leaves the administrative alias, Codex history, projects, authentication,
  plugins, shared daemon, and network configuration unchanged.

`--purge` removes retained managed logs and state only after an explicit typed
confirmation. Rollback instructions remain available if either atomic update is
interrupted.

## Public documentation

The repository provides English primary documentation and a Chinese quick start:

- `README.md`: value proposition, five-minute quick start, limitations, and
  unofficial-project disclaimer;
- `README.zh-CN.md`: equivalent Chinese quick start;
- `docs/architecture.md`: runtime and ownership boundaries;
- `docs/installation.md`: safe and one-line installation paths;
- `docs/security-model.md`: keys, credentials, threat model, and fail-closed
  behavior;
- `docs/compatibility.md`: supported OS, architecture, Codex, and Desktop matrix;
- `docs/troubleshooting.md`: redacted diagnostics and rollback;
- `PRIVACY.md`: no telemetry and no credential collection;
- `SECURITY.md`: private vulnerability-reporting guidance;
- `CONTRIBUTING.md`, `CHANGELOG.md`, and an actual `LICENSE` file.

Examples use only reserved domains and neutral placeholders such as
`example-host`, `example-managed`, `user`, `/Users/user`, and RFC 5737 addresses
when an address is unavoidable.

## Tests and release gates

No public push or community post occurs until every gate passes:

1. Existing Rust unit and integration tests pass.
2. New installer tests first fail against missing behavior, then pass after
   implementation. Tests cover argument validation, key isolation, SSH marker
   idempotency, checksum failure, unsupported routing, interrupted installation,
   and exact uninstall.
3. Shell syntax and static analysis pass.
4. GitHub Actions passes on macOS Apple Silicon and Intel runners where
   available, with build-only cross-target fallback documented when a native
   runner is unavailable.
5. Release bundles are reproducible enough to verify their manifest and
   checksums after download.
6. A recursive working-tree and complete-history scan rejects development
   usernames, IPs, aliases, paths, task IDs, credential markers, private-key
   blocks, token patterns, proxy URLs, and high-risk generated files.
7. A fresh clone of the public repository installs through a disposable managed
   alias on the acceptance Mac, establishes a real app-server session, and
   exercises history resume, archive or bounded idle reclamation, browser, and
   Computer Use.
8. The disposable alias, key, remote authorization, processes, sockets, and test
   tasks are removed immediately after acceptance.
9. The existing production managed alias and unrelated host services remain
   unchanged.

The secret scanner runs before the first commit, against the new commit, against
the tag, and against the generated release archives.

## GitHub publication

The authenticated GitHub CLI on the release host is used only to create and push
the new public repository. Its credentials are never read or copied. Publication
creates:

- a public repository named `codex-managed-channel` when available;
- a protected/default `main` branch;
- a `v0.1.0` annotated tag;
- a GitHub Release with architecture-specific bundles and `SHA256SUMS`;
- CI, issue templates, and a concise security policy;
- a repository description that identifies the project as unofficial.

If the name is unavailable, publication stops before creating a differently
named repository so the owner can approve the name.

## OpenAI community publication

After a fresh-clone release acceptance succeeds, publish one English post in the
official `openai/codex` Discussions area. The post:

- describes the generic SSH app-server lifecycle problem without claiming that
  every symptom is an upstream Codex defect;
- presents redacted aggregate evidence and the supervisor architecture;
- states that the project wraps, but does not fork, the official app-server;
- links the public repository, release, security model, and compatibility
  matrix;
- labels app-server and Desktop wrapper compatibility as version-sensitive;
- asks for feedback from users with other Mac architectures and SSH topologies.

Do not file an upstream bug issue in version 0.1 unless the public repository
contains a minimal reproduction that isolates an upstream defect from the
wrapper. A discussion is the first feedback channel.

## Completion criteria

The release is complete only when:

- the public repository and every reachable object pass the privacy scan;
- `v0.1.0` release assets are downloadable and checksum-valid;
- the documented quick-start installs successfully from a fresh clone;
- the disposable acceptance installation is fully removed;
- the original managed installation still passes a read-only handshake;
- the OpenAI Codex Discussion is publicly accessible;
- the private development document records only the public repository and
  discussion URLs, without copying credentials.

