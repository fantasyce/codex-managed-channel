# Troubleshooting

## New alias is not visible

Confirm `ssh example-managed codex --version` works, then restart Codex Desktop
once. Do not remove or rename the administrative alias.

## Installation stops before creating a key

This is expected for validation, SSH, platform, or checksum failures. Correct
the reported prerequisite and rerun; no configuration should have changed.

## Computer Use is unavailable

Open ChatGPT Desktop on the remote Mac, verify its Computer Use plugin is
installed, and rerun the installer. If Desktop is installed in a nonstandard
location, set `CODEX_MANAGED_DESKTOP_RESOURCES` to its `Contents/Resources`
directory for preflight and runtime.

## A task says it is owned elsewhere

Finish any active turn in the other client. The managed channel can transfer an
idle task through official archive and restore calls, but deliberately refuses
to take over an active task.

## Connection closes after inactivity

The default idle policy drains after ten minutes without user activity. Sending
a new request reconnects through the official protocol and loads the same task
history. Background polling does not keep an abandoned connection alive.

## Reporting a problem

Run only the minimal diagnostic needed and redact usernames, hostnames,
addresses, SSH lines, task identifiers, and tokens before posting. Never attach
an entire Codex home or raw SSH configuration.
