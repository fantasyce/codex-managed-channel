# Changelog

## 0.2.0 - 2026-09-17

- Add fixed per-client, single-writer sessions with bounded reattachment to the
  same official worker; retain legacy entry mode without silently upgrading it.
- Correct turn identity tracking, resumed/server-started work and pending
  commands; confirm official idle status before idle/soft-FD cleanup.
- Add guardian-owned startup, crash cleanup and tracked reparented descendants;
  preserve hard FD limits and non-renewable age/detach budgets.
- Serialize and bound metadata logs, add exact client stop, dedicated-key
  registration checks and SSH liveness settings.
- Add independent real-runtime/mock-provider acceptance covering reconnect,
  isolation, expiry, resource limits, crashes and pending user interactions.
- Reclaim idle thread resources in place with `thread/unsubscribe`; retain the
  connection on success and use bounded disconnect cleanup only as fallback.
- Document that descendant FD pressure is not yet mapped to individual tool
  calls: v0.2.0 still uses whole-runtime cleanup at the hard app-server limit.
- Include the project license and target-specific third-party dependency license
  texts in every binary release archive.

## 0.1.0 - 2026-09-05

- Isolated official app-server sessions behind a restricted SSH forced command.
- Added archive, disconnect, idle, and file-descriptor cleanup policies.
- Added thread takeover support for an idle task previously owned elsewhere.
- Added dynamic personal and bundled plugin discovery, including official
  Computer Use and browser surfaces.
- Added checksum-verified installation, exact uninstall, privacy gates, and
  macOS Apple silicon and Intel packaging.
