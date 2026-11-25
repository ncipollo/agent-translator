# agent-translator

Translate between AI coding agent formats. `agent-translator run` launches an agent CLI,
reads its native output, and turns it into one common stream of events, while optionally
archiving the native output or sending streams to several places at once.

## Usage

```
agent-translator run --agent <name|path/to/agent.toml> [--cwd DIR]
    [--var KEY=VALUE]... [--session-id ID] [--resume]
    [--out FORMAT[:DEST]]... [--err DEST]... [--] <prompt | ->
```

| Flag | Meaning |
|---|---|
| `--agent` | A bundled agent name (`claude`, `claude-opus`, `claude-haiku`, `claude-fable`, `codex`, `cortex`, `opencode`, `cursor`, `pi`) or the path to an agent TOML file. An argument containing a path separator or ending in `.toml` is treated as a path. |
| `--cwd` | Working directory for the agent. |
| `--var KEY=VALUE` | Extra `{{variable}}` for the command template. Repeatable. |
| `--session-id` | Session id for the run. A UUID v4 is generated when omitted. |
| `--resume` | Continue the session given by `--session-id`, using the agent's `headless_resume` template. Requires `--session-id`. |
| `--out FORMAT[:DEST]` | Where stdout goes. Repeatable. See [Sinks](#sinks). |
| `--err DEST` | Where the agent's stderr goes. Repeatable. Defaults to this process's stderr. |
| `<prompt>` | The prompt, passed to the agent as a single argument (never through a shell). `-` reads it from stdin. Put `--` before a prompt that starts with `-`. |

The exit code mirrors the agent's (`128 + signal` if it was killed by one). Argument errors
exit with 2. Configuration or spawn errors exit with 1 and print a one-line message on stderr.
When the agent's config sets `parse_session_id = true`, the captured session id is printed on
stderr at exit as `session_id: <id>` so scripts can resume later.

```sh
agent-translator run --agent claude --out events --out raw:/tmp/native.ndjson -- "say hello"
echo "summarize this repo" | agent-translator run --agent codex -
agent-translator run --agent claude --session-id "$ID" --resume -- "now add tests"
```

## Sinks

`--out` takes `FORMAT[:DEST]`:

- `FORMAT` is `raw` (the agent's bytes exactly as emitted) or `events` (one JSON event
  envelope per line: `seq`, `ts_ms`, and the event itself).
- `DEST` is `stdout`, `stderr`, or a file path. It defaults to `stdout`. Files are truncated
  when opened and flushed after every write, so another process can tail them.

```sh
--out events                            # events on stdout (the default when no --out is given)
--out raw:/tmp/native.ndjson            # archive the native stream
--out events:/tmp/events.ndjson         # events to a file as well
```

`--err` takes only a destination, and stderr is always passed through raw. `events` is rejected
on `--err`. Giving any `--out` replaces the default, so add `--out events` back if you still
want events on stdout. When the agent uses raw parsing and an `events` sink is configured, one
`text` event carrying the whole stdout is emitted when the agent exits.

### Events

Each event has a `type` of `text`, `thinking`, `tool_call`, `tool_result`, `system_init`,
`system_notice`, `session_result`, or `unknown`. Anything the parsing rules do not recognize
(including invalid JSON and a trailing line without a newline at end of output) becomes an
`unknown` event with the original text and a `reason` where there is one.

## Agent config

An agent is one TOML file:

```toml
cmd = ["claude", "-p", "--output-format", "stream-json", "--verbose", "--session-id", "{{session_id}}"]
resume = ["claude", "--resume", "{{session_id}}"]
headless_resume = ["claude", "-p", "--output-format", "stream-json", "--verbose", "--resume", "{{session_id}}"]
parse_session_id = false

[output_parsing]
type = "typed"                  # or "raw"
rules_file = "parsing/claude.json"

[env]
allow = ["HOME", "PATH"]
deny = ["AWS_SECRET_ACCESS_KEY"]

[env.overrides]
NO_COLOR = "1"
```

- `cmd` is the start command. The prompt is appended as the final argument.
- `resume` is the interactive resume command. `headless_resume` is the streaming command for
  follow-up turns. Without `headless_resume`, `--resume` falls back to `cmd` with a new session id.
- Templates may use `{{variable}}`. `session_id` is always set, and `--var` adds more. Unknown
  tokens are left as written.
- `parse_session_id = true` means the agent mints its own session id, captured from its first
  `system_init` event instead of being injected.
- `output_parsing.type = "raw"` skips translation. `"typed"` translates with the rules in
  `rules_file`, resolved relative to the config file.
- `[env]` filters the agent's environment. The order is: inherited variables, then
  `overrides`, then `allow` (when non-empty, only listed names are kept), then `deny`.

To start from the bundled configs, write them out with `agent_translator::feature::defaults::write_all`
and edit the copies.

## Parsing rules

A rules file is a JSON array (or a TOML file with a `rules = [...]` key) of rules. Rules run in
order against each JSON line of the agent's output.

```json
[
  {
    "name": "assistant-text",
    "when": [{ "op": "eq", "path": "type", "value": "assistant" }],
    "iterate": "message.content",
    "when_each": [{ "op": "eq", "path": "type", "value": "text" }],
    "emit": { "event": "text", "fields": { "text": { "path": "text" } } }
  }
]
```

- `when` must all match the line. Matchers: `eq` and `in` (string values only), `exists`, and
  `not`. Paths are dot-separated; numeric segments index arrays; `.` is the whole value.
- A matching rule without `iterate` wins immediately and emits one event.
- With `iterate`, the named array is walked and each element that passes `when_each` emits an
  event built from that element. Matching `iterate` rules all contribute, ordered by element index.
- `emit.fields` build the event. A field is a literal, `{ "path": ..., "default": ... }`,
  `{ "value": ... }`, `{ "concat": [...], "sep": ... }`, or
  `{ "map_path": { "from": ..., "pluck": ... } }`. A field that evaluates to nothing is omitted.
- Rules are checked when the config loads: unknown events, unknown fields, and missing required
  fields are errors.
