# Version 0.2.1 acceptance

Validated on macOS Apple silicon with official Codex CLI 0.159.2.

- The regression fixture reproduced selection of a stale standalone executable
  despite configured Desktop resources, then passed with the new resolver.
- Explicit binary overrides retain precedence over Desktop resources.
- Rust tests, formatting, strict Clippy, 30 Python tests, shell syntax and the
  source/history privacy gate passed.
- Real-runtime reconnect acceptance retained one worker and one mock request.
- Real-runtime idle-thread acceptance returned FD/pipe counts to baseline;
  both scenarios ended with no cleanup intervention or owned-process residue.
- The official runtime model catalog included both Sol model identifiers that
  were absent from the older managed runtime catalog.
- Deployment preflight and a restricted SSH version probe reported 0.159.2;
  the newly started managed worker used that runtime.

These checks do not assert that every lifecycle scenario was rerun for this
patch, or that a real model inference request was executed. Model availability
remains subject to the signed-in account. No account credentials or host
identifiers are recorded here.
