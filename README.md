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

## Config

Everything zakhar writes lives in one place — `~/.zakhar`:

```
~/.zakhar/
  config/config.toml   # providers + agents
  config/profile.md    # who you are (fed to the AI)
  sessions/            # conversation history
  reminders.json       # reminders
```

Per-project files (`memory`, skills) live in `.zakhar/` inside each repo.

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