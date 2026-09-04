# Community announcement draft

## Title

Codex Managed Channel: lifecycle-controlled remote macOS SSH sessions for Codex Desktop

## Body

I have released Codex Managed Channel, an unofficial open-source SSH boundary
for using Codex Desktop with an existing remote Mac.

It keeps the official Codex app-server, ChatGPT authentication, plugins,
Computer Use, and browser surfaces on the remote Mac. The added supervisor
creates an isolated app-server socket per connection and reclaims only its own
process group after archive, disconnect, bounded idle, or file-descriptor
pressure. Active turns are protected by the lifecycle policy.

The installer uses an existing administrative SSH alias, verifies release and
bundle checksums, creates a dedicated Ed25519 key, and adds an exact restricted
authorization entry. The private key never leaves the client. The project has
no telemetry and includes worktree, Git-history, and archive privacy scanning.

Version 0.1 supports macOS clients and remote hosts on Apple silicon or Intel.
It intentionally fails closed for unsupported Desktop layouts and arbitrary
SSH ProxyCommand programs.

Repository: https://github.com/fantasyce/codex-managed-channel

I would especially value feedback on compatibility across Codex Desktop
updates, cleanup behavior after interrupted transports, and additional
fail-closed SSH topologies worth supporting.
