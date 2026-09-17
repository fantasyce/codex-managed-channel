# Compatibility

Version 0.2 targets a macOS client and a macOS remote host.

| Component | Supported boundary |
| --- | --- |
| Client CPU | Apple silicon or Intel |
| Remote CPU | Apple silicon or Intel |
| SSH | An existing administrative alias that succeeds in batch mode |
| Codex | The official Codex binary from ChatGPT Desktop or a user-supplied `CODEX_MANAGED_CODEX_BIN` |
| Computer use | The `unified-computer-use` plugin shipped with the installed ChatGPT Desktop build |
| Browser | Surfaces exposed by that official plugin on the remote Mac |
| Authentication | Existing ChatGPT authentication on the remote Mac |

The runtime first honors `CODEX_MANAGED_DESKTOP_RESOURCES`. Without that
override it checks the system Applications folder and the current user's
Applications folder. It requires the bundled Codex executable, Node runtime,
Node REPL executable, Node modules, plugin manifest, and Computer Use service
before it writes a managed configuration.

The runtime deliberately stops with a bounded error when these resources are
missing. It does not download, emulate, or replace proprietary Desktop
components. Linux, Windows, headless browser substitution, password-based SSH
bootstrapping, and arbitrary SSH `ProxyCommand` programs are outside the 0.2
support boundary.
