# Architecture

```text
Codex Desktop
    │ app-server proxy protocol over SSH
    ▼
restricted authorized_keys forced command
    ▼
codex-managed-entry supervisor
    ├── isolated official codex app-server socket
    ├── official app-server proxy transport
    ├── official unified-computer-use MCP process tree
    └── bounded lifecycle and file-descriptor policy
```

The supervisor is a transport boundary, not an alternative agent harness. It
does not interpret prompts or tool results. A protocol observer copies bytes
unchanged while tracking request completion, active turns, archive completion,
and client activity. Cleanup is always scoped to the process group created for
that connection.

Each connection gets a private Codex runtime directory. Authentication, skills,
plugin cache, and conversation history are linked read-only by location or
referenced from the user's real Codex home, while SQLite runtime files, sockets,
logs, and temporary state remain isolated. Marketplace catalogs are generated
from currently installed plugin manifests, so no user's plugin inventory ships
in the repository.

When a task is still owned by another idle app-server, the takeover flow asks
the official servers to archive and restore that task. It does not edit lock or
database files. An active task is never taken over.
