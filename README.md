<p align="center">
  <img src="logo.svg" width="128" alt="zakhar logo">
</p>

# zakhar

Multi-agent CLI for working with AI coding models.

## Setup

Build:

```sh
cargo build --release
```

Configure a provider. Copy `zakhar.example.toml` to
`~/.config/zakhar/config.toml` and set your API key. The key can be inlined or
referenced from an environment variable with `{env:NAME}`.

## Usage

```sh
zakhar chat --provider zen   # interactive chat (modern UI)
zakhar chat --model big-pickle
zakhar chat --agent default  # use a configured agent
zakhar chat --simple         # plain text UI (no markdown/colors/status line)
zakhar models                # list providers and models

# natural-language quick commands (no chat session)
zakhar search here           # understand the phrase and do the task
zakhar find "TODO" in src    # bare words = one-shot agent, live preview line
zakhar delete the logs       # mutating tools wait for your [y/n]
zakhar open chat             # AI-controlled: above now via the `control` tool
zakhar show me the models    # AI-controlled: lists models via the `control` tool
zakhar create a file, you have my permission  # phrase grants autonomy, no [y/n]
```

zakhar controls itself through a single `control` tool (actions: `allow`,
`models`, `chat`), so any phrase can steer the whole tool through natural
language. Tools are unified behind a `Handler` trait (`src/handler.rs`),
implemented modularly in `src/tools/`.

## Tools

Every tool is a `Handler` (`fn spec() -> Tool`, `fn run() -> String`), so the
model drives all of them through natural language:

- `read` / `write` / `edit` / `glob` / `grep` — filesystem operations
- `bash` — run a shell command, optionally detached (`detach=true`, inspect
  later via `task`); `task` also lists/reads detached tasks
- `ask` — ask the user a question and get the answer
- `todo` — maintain a task list across turns
- `control` — `allow` (grant mutation permission), `models`, `chat` (hand off
  to an interactive session)
- `context` — per-project memory: `save`/`get`/`list`/`drop` keys in
  `.zakhar/context.json`; the key index is auto-injected at session start and
  values are fetched on demand
- `watch` — run a long-lived process and act as its parent: `start` returns a
  task id, `read` returns output since your last read (capped), `send` writes
  to its stdin, `stop` terminates it. Use for servers, `tail -f`, or long
  tools that span turns.
- `skill` — load a specialized skill's instructions

Mutating tools (`write`, `edit`, `bash`, `watch start`, ...) wait for your
`[y/n]` unless permission was granted in advance (via `control allow` or the
"you have my permission" phrase).

## UI

`zakhar chat` uses a modern terminal UI by default: assistant responses are
rendered as colored markdown (headings, bold, code, tables, links, lists), and
progress/status messages collapse into a single updating line. Pass `--simple`
to fall back to a plain stream of raw text and `[zakhar]` status lines.

## Config

```toml
default_provider = "zen"
default_model = "big-pickle"

[providers.zen]
api_type = "openai"
base_url = "https://opencode.ai/zen/v1"
api_key = "{env:OPENCODE_API_KEY}"
default_model = "big-pickle"
user_agent = "opencode/1.18.25"
models = ["big-pickle", "deepseek-v4-flash-free", "mimo-v2.5-free"]

[agents.default]
model = "big-pickle"
prompt = "You are zakhar, a helpful coding assistant."
```

### User-Agent note

OpenCode Zen rate-limits requests that don't present the OpenCode client
User-Agent (`opencode/<version>`). Without it you get `FreeUsageLimitError`
even when the key is valid. Set `user_agent` on a Zen provider so its requests
are treated as first-party. Other OpenAI-compatible providers can omit it.

Sessions are stored under the platform data directory in
`zakhar/sessions/`.
