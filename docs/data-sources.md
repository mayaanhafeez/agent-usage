# Data Sources and Privacy

`agent-usage` combines local history written by coding-agent clients with quota
information exposed by their account APIs. It does not proxy prompts, inspect
project source files, or maintain its own usage database.

## Local reads

| Provider | Paths read | Purpose |
| --- | --- | --- |
| Claude Code | `~/.claude/projects/**/*.jsonl` | Seven-day token history and model totals |
| Claude Code | macOS Keychain or `~/.claude/.credentials.json` | Plan name and OAuth usage request |
| Codex | `$CODEX_HOME`, or `~/.codex/sessions/**/*.jsonl` | Seven-day token history and model totals |
| Codex | `$CODEX_HOME/auth.json`, or `~/.codex/auth.json` | ChatGPT plan and quota request |
| OpenCode | `$XDG_DATA_HOME/opencode/opencode.db` or the platform data directory | Seven-day session and model totals |
| Gemini CLI | `~/.gemini/tmp/**/chats/*.json` | Seven-day token history and model totals |

OpenCode's database is opened read-only. When OpenCode history includes GPT or
Codex models, an existing local Codex login may also be read to display the
associated ChatGPT quota.

Only recently modified history files and records from the last seven calendar
days are included.

## Network requests

The application makes at most one short, authenticated request per applicable
provider during collection or refresh:

| Provider | Endpoint |
| --- | --- |
| Claude Code | `https://api.anthropic.com/api/oauth/usage` |
| Codex / ChatGPT | `https://chatgpt.com/backend-api/wham/usage` |

Claude Code and Codex both expose a rolling five-hour window and a weekly
window; each is shown as a separate limit, labelled `Session` and `Weekly`. A
window that the account's plan does not report is omitted.

Requests use credentials already managed by the corresponding official CLI.
Credentials are held in memory for the request and are not logged or written by
`agent-usage`. Gemini CLI does not expose quota details, so no Gemini account
request is made.

## Token totals

Codex, OpenCode, and Gemini totals use the token counts recorded by their local
clients. Claude totals use the following weighted count to better represent
billed token usage:

```text
input + output + (cache creation x 1.25) + (cache reads x 0.1)
```

Totals are informational and can differ from provider billing dashboards due to
client schema changes, local history retention, timezone boundaries, or
provider-side accounting.

## Process detection

Automatic detection inspects process names and command lines for supported
agent executables. Use `--provider` or `AGENT_USAGE_PROVIDER` when an agent is
running through a wrapper that cannot be identified reliably.
