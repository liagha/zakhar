<p align="center">
  <img src="logo.svg" width="128" alt="zakhar logo">
</p>

# zakhar

Multi-agent CLI for working with AI coding models. Talk to your files, let the AI do the work.

## Install

One line — downloads a prebuilt binary for your OS and arch (no Rust needed):

```sh
curl -fsSL https://raw.githubusercontent.com/liagha/zakhar/master/install.sh | bash
```

Installs the `zakhar` binary to `~/.local/bin/zakhar` (override with `ZAKHAR_INSTALL_DIR=/somewhere`). Make sure that directory is on your `PATH`. Linux (x86_64, aarch64), macOS (x86_64, aarch64), and Termux on Android are supported — Termux gets a static build that doesn't depend on glibc.

## Set up a provider

Copy `zakhar.example.toml` to `~/.zakhar/config/config.toml`, then set your API key:

```sh
mkdir -p ~/.zakhar/config
cp zakhar.example.toml ~/.zakhar/config/config.toml
echo 'export OPENCODE_API_KEY=sk-...' >> ~/.bashrc   # or your shell's rc
source ~/.bashrc
```

Check it works:

```sh
zakhar models               # lists providers and models
zakhar chat                 # interactive session
```

The example uses the free OpenCode Zen tier (`big-pickle` and friends). Any OpenAI-compatible API works — just change `base_url` and `api_key`. A key can be inlined or referenced from an env var with `{env:NAME}`.

## Use it

Everything is natural language:

```sh
zakhar chat                       # interactive chat
zakhar make a todo app            # one-shot: understand the phrase, do the task
zakhar find "TODO" in src         # bare words = agent runs and shows you lines
zakhar delete the logs            # mutating actions wait for your [y/n]
zakhar make sure I don't forget my pills at 11am   # AI schedules a reminder
zakhar models                     # list providers and models
zakhar paths                      # where everything lives
```

Reminders are an AI tool: the model turns a phrase like *"my pills at 11am"* into a stored reminder, and a background daemon (`notify-send` on Linux, `osascript` on macOS) fires it when due.

## Terminal

Markdown streams in, rendered live: headings, bold, code fences (with syntax-style coloring on the fence line), inline code, lists, and tables are colored as each completes. The in-progress line is shown as a dim preview that repaints in place without flicker. Tool steps appear as `▸ call` / `▾ result` lines, and each turn ends with a summary line like `done · 4.2s · 1 tool(s) · provider/model`. Add `--simple` to `zakhar chat` for a plain no-ANSI output.

## Memory

Zakhar keeps a persistent memory of your project — knowledge it learns while working, and an event log of everything that happens. Both live in `.zakhar/memory/` inside the project.

- **Knowledge** (`.zakhar/memory/knowledge.jsonl`) — facts, decisions, preferences, and open loops, each with a salience score that decays as it ages. The `context` tool saves/gets/list/drops items by key; the `remember` tool does semantic recall — stemming, synonyms, and rarity-aware ranking — so a question phrased any way returns the matching entry. Stale entries (unused past a threshold) are listed by `zakhar` and are natural candidates for the mind to drop.
- **Events** (`.zakhar/memory/episodic.jsonl`) — a running log of chats and shouts. When it exceeds 100 entries, `zakhar` archives the oldest into `.zakhar/memory/archive/` and backgrounds a summary.
- **Mind** — periodically, a small multi-agent ensemble (archivist → critic → validator) reads the events, then proposes consolidation: merging duplicates, updating salience, flagging open loops, and dropping stale entries. The journal lands in `NOTES.md`, and the same job also fires on `zakhar chat` exit, shout, and compaction.
- **Agent ledger** (`.zakhar/ledger.jsonl`) — every mutating tool call is recorded with a content digest and a backup of the pre-edit file, so you can undo. `zakhar chat` exposes it as `/undo` and `/audit`.

The daemon (`zakhar daemon`) drains a job queue at `~/.zakhar/jobs` and runs summarization and mind consolidation in the background. Disable pieces with `ZAKHAR_NO_COMPACT`, `ZAKHAR_NO_MIND`, or `ZAKHAR_NO_DAEMON`.

Slash commands in `zakhar chat`:

```
/clear    /compact  /init     /help     /agents   /skills
/memory   /undo     /audit    /sessions /resume   /kill
```

`/memory` browses knowledge and recent events, and has subcommands: `/memory drop <key>` forgets an entry, `/memory search <text>` recalls, `/memory stale [days]` lists and prunes decayed items, `/memory compact` archives events, and `/memory mind` triggers a background consolidation.

## Config

Everything zakhar writes lives in one place — `~/.zakhar`:

```
~/.zakhar/
  config/config.toml   # providers + agents
  config/profile.md    # who you are (fed to the AI)
  sessions/            # conversation history
  reminders.json       # reminders
  jobs/                # background job queue (daemon)
```

Per-project files live in `.zakhar/` inside each repo:

```
.zakhar/
  memory/knowledge.jsonl   # knowledge store (context + remember)
  memory/episodic.jsonl    # event log
  memory/archive/          # archived + summarized events
  memory/mind.log          # consolidation runs
  NOTES.md                 # mind journal
  ledger.jsonl             # agent ledger (undo/audit)
```

```toml
# ~/.zakhar/config/config.toml
default_provider = "zen"
default_model = "big-pickle"

[providers.zen]
api_type = "openai"
base_url = "https://opencode.ai/zen/v1"
api_key = "{env:OPENCODE_API_KEY}"
default_model = "big-pickle"
user_agent = "opencode/1.18.25"
models = ["big-pickle", "deepseek-v4-flash-free", "mimo-v2.5-free"]
```

## Colors

UI colors are configured per-role in the `[ui]` section. Each key accepts a
color name (`green`, `bright_black`, `cyan`) or a hex value (`#ff8800`).
`dim` renders the dimmed style, and `plain`/`none` disables color for that
role. Unset roles keep their defaults.

```toml
[ui]
ok = "green"
err = "#ff3333"
code = "#6b7280"
link = "bright_blue"
summary = "dim"
quote = "none"
```

## Model routing

Different jobs want different models. Two config sections control which
provider+model a task goes to:

- **`[levels.*]`** — weight tiers. `light` is used by background/cheap work
  (compaction, memory summarization), `heavy` by interactive sessions
  (`zakhar chat`, `zakhar mobile`).
- **`[capabilities.*]`** — task-type routing. Each capability names a
  (provider, model) plus keyword `hints`. When a task is phrased in natural
  language, the hints pick a capability; e.g. anything mentioning an image,
  photo or screenshot lands on the vision model, coding keywords land on the
  code model.

```toml
[capabilities.code]
provider = "zai"
model = "glm-4.7-flash"
hints = ["refactor", "compile", "debug", "function", "class", "module"]
fallback = ["opencode/big-pickle"]

[capabilities.vision]
provider = "zai"
model = "glm-4.6v-flash"
hints = ["image", "photo", "screenshot", "picture", "diagram"]
fallback = ["opencode/mimo-v2.5-free"]
```

Resolution order: capability `provider`/`model` if set, otherwise the matching
`[levels.*]` entry, otherwise `default_provider`/`default_model`. Fields not
configured fall through to `default_model`, and agents can still pin their own
`model` in `[agents.*]`. See `zakhar.models` for a live view of both tables.

### Fallback chains

Every route is a fallback chain. The resolved `provider/model` is tried first;
if it fails at request time (overloaded, unauthorized, unreachable) the next
candidate is tried:

1. the capability's own `fallback = ["provider/model", ...]` entries (or the
   level's `fallback` list when the task came through a level),
2. then every other configured provider, in provider-name order, using its own
   `default_model`.

So a config with a single provider behaves exactly as before, and one with
several gets automatic fail-over for free — no per-route config required. Each
entry is `"provider"` or `"provider/model"` (`"provider"` uses that provider's
default model).

The switch policy is per-call-site: interactive sessions (`chat`, a phrase in
`shout`) ask `fall back to opencode/big-pickle? [y/N]` before switching;
background work (daemon compaction, mobile turns) switches silently. The choice
is remembered for the rest of the turn, so a later failure on the same
candidate falls back again.

## Uninstall

```sh
zakhar clean        # wipes ~/.zakhar (asks first)
rm ~/.local/bin/zakhar
```

## Build from source

```sh
cargo build --release
```

The result is at `target/release/zakhar`.