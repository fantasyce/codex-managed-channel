# 90-second walkthrough

This walkthrough is intentionally synthetic. `example-host`, `example-managed`,
and `/Users/user` are placeholders; do not paste real host details into issues,
screenshots, or recordings.

## 0–20 seconds: install

Start with an existing administrative SSH alias and install the signed release:

```sh
curl -fsSL https://raw.githubusercontent.com/fantasyce/codex-managed-channel/v0.2.0/install.sh | \
  sh -s -- --remote example-host --alias example-managed \
  --repository fantasyce/codex-managed-channel --version v0.2.0
```

The installer resolves the existing alias, checks the supported topology,
verifies the release checksum, creates a dedicated key, and adds exact marked
configuration entries. It does not replace existing SSH hosts or keys.

## 20–45 seconds: use the managed connection

Open Codex Desktop and select `example-managed`. Projects, conversation history,
ChatGPT authentication, installed plugins, and Computer Use still come from the
remote Mac. The supervisor only owns the app-server process group created for
this connection.

## 45–70 seconds: reclaim safely

Continue using an active task normally. When the connection is archived,
disconnected, idle beyond policy, or under file-descriptor pressure, the
supervisor waits through its cancelable drain window and reclaims its verified
process group. New activity cancels idle cleanup; active turns are protected.

## 70–90 seconds: inspect or uninstall

Follow the privacy-safe checks in the
[acceptance template](acceptance-template.md). Remove only this managed alias and
its restricted authorization entry with:

```sh
./uninstall.sh --remote example-host --alias example-managed
```

Logs and state remain available by default. Permanent state removal requires the
explicit `--purge purge` option.

## Privacy rule

Share only pass/fail results and aggregate counts. Never publish usernames,
hostnames, addresses, key material or fingerprints, SSH configuration, task IDs,
prompts, raw logs, plugin inventories, or local/remote filesystem paths.
