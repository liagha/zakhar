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
```

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
