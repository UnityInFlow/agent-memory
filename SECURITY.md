# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |
| < 0.1   | No. Please upgrade. |

Fixes land on `main` and ship in the next 0.1.x release.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it privately through GitHub's private vulnerability reporting: open the
repository's **Security** tab → **Report a vulnerability**, or go to
<https://github.com/UnityInFlow/agent-memory/security/advisories/new>.

Please include the agent-memory version (`agent-memory --version`), your
platform, the transport involved (MCP stdio, REST, or the CLI), and steps to
reproduce. We aim to acknowledge a report within 7 days. We will agree a
disclosure date with you and credit you in the advisory unless you ask us not
to.

## Scope

agent-memory is a **local** binary. It runs an MCP stdio server, an optional
REST daemon and a set of CLI subcommands over an embedded SQLite file on your
own machine. It has no cloud service and no accounts, and it makes no network
calls except to the Ollama URL you configure (`http://localhost:11434` by
default).

In scope, for example:

- Leaking storage-engine detail (SQLite text, table or column names) through
  MCP or REST error responses. These are redacted by design.
- The REST daemon accepting a non-loopback bind without `--allow-remote`, or
  any path that exposes it beyond loopback by default.
- A crafted file passed to `agent-memory import` (JSONL or GSD `STATE.md`)
  causing a panic, memory corruption, writes outside the database, phantom
  links to existing rows, or a partially applied import.
- `agent-memory export --output` creating a file readable by other users
  (it must be `0600` on Unix) or leaving a half-written file behind.
- Data written by one MCP client being lost or silently altered by another
  client sharing the same database.
- Tampering with a release artifact that still passes `SHA256SUMS.txt` or
  `gh attestation verify`.

Out of scope (by design):

- **The REST API has no authentication.** It binds to loopback by default and
  refuses other addresses unless `--allow-remote` is passed. Anyone who can
  reach a daemon you exposed with `--allow-remote` can read and write every
  memory. Put your own authenticating proxy in front of it if you need remote
  access.
- Other processes running as your user. They can read the database file and
  its backups directly, just as they can read any file you own.
- Export written to **stdout** ending up in terminal scrollback or shell logs.
  Use `--output` for a private file.
- Content you store in a memory being sent to the Ollama instance you
  configured. That is how semantic search works.
- Vulnerabilities in Ollama or in third-party MCP clients themselves.
