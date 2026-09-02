# Graph Report - zakhar  (2026-09-02)

## Corpus Check
- 18 files · ~5,593 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 198 nodes · 374 edges · 14 communities (13 shown, 1 thin omitted)
- Extraction: 100% EXTRACTED · 0% INFERRED · 0% AMBIGUOUS
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- openai.rs
- Message
- chat.rs
- Provider
- invoke.rs
- Registry
- config.rs
- provider/types.rs
- Session
- delegate.rs
- Command
- zakhar
- zakhar

## God Nodes (most connected - your core abstractions)
1. `Message` - 16 edges
2. `Provider` - 12 edges
3. `Registry` - 11 edges
4. `SseStream` - 11 edges
5. `ToolDef` - 10 edges
6. `OpenAI` - 10 edges
7. `ToolCall` - 10 edges
8. `Tool` - 10 edges
9. `parse()` - 9 edges
10. `ChatRequest` - 9 edges

## Surprising Connections (you probably didn't know these)
- `Runner` --references--> `Message`  [EXTRACTED]
  src/agent.rs → src/types.rs
- `Runner` --references--> `Tool`  [EXTRACTED]
  src/agent.rs → src/types.rs
- `tool_def()` --references--> `Tool`  [EXTRACTED]
  src/delegate.rs → src/types.rs
- `run()` --references--> `Provider`  [EXTRACTED]
  src/delegate.rs → src/provider/mod.rs
- `ToolDef` --references--> `Tool`  [EXTRACTED]
  src/invoke.rs → src/types.rs

## Import Cycles
- None detected.

## Communities (14 total, 1 thin omitted)

### Community 0 - "openai.rs"
Cohesion: 0.11
Nodes (28): Client, Context, Item, Pin, Poll, ChatStreamEvent, String, ToolCallPart (+20 more)

### Community 1 - "Message"
Cohesion: 0.17
Nodes (18): D, Deserialize, Error, Into, Ok, S, Serialize, ChatRequest (+10 more)

### Community 2 - "chat.rs"
Cohesion: 0.13
Nodes (16): chat(), compact_args(), compact_args_object(), compact_args_truncates(), Option, Result, String, Value (+8 more)

### Community 3 - "Provider"
Cohesion: 0.16
Nodes (12): AgentCfg, DeltaStream, Option, Result, Self, String, Vec, Runner (+4 more)

### Community 4 - "invoke.rs"
Cohesion: 0.28
Nodes (14): Executor, Invoke, register_bash(), register_edit(), register_glob(), register_grep(), register_read(), register_write() (+6 more)

### Community 5 - "Registry"
Cohesion: 0.17
Nodes (11): Registry, Box, Option, Self, Vec, build(), default_provider(), resolve_key() (+3 more)

### Community 6 - "config.rs"
Cohesion: 0.24
Nodes (11): Agent, Config, config_path(), HashMap, Option, PathBuf, ProviderConfig, Result (+3 more)

### Community 7 - "provider/types.rs"
Cohesion: 0.36
Nodes (12): Chunk, ChunkChoice, ChunkDelta, ChunkFunction, ChunkToolCall, Config, default_api_type(), default_user_agent() (+4 more)

### Community 8 - "Session"
Cohesion: 0.25
Nodes (7): dir(), PathBuf, Result, Self, String, Vec, Session

### Community 9 - "delegate.rs"
Cohesion: 0.44
Nodes (8): compact_args(), Config, String, Value, run(), tool_def(), ToolCallPartAccum, truncate()

### Community 10 - "Command"
Cohesion: 0.33
Nodes (6): Cli, Command, main(), Option, Result, String

### Community 11 - "zakhar"
Cohesion: 0.33
Nodes (5): Config, Setup, Usage, User-Agent note, zakhar

## Knowledge Gaps
- **4 isolated node(s):** `zakhar`, `Setup`, `Usage`, `User-Agent note`
  These have ≤1 connection - possible missing edges or undocumented components.
- **1 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Message` connect `Message` to `openai.rs`, `Session`, `Provider`, `provider/types.rs`?**
  _High betweenness centrality (0.209) - this node is a cross-community bridge._
- **Why does `Registry` connect `Registry` to `openai.rs`, `chat.rs`, `Provider`?**
  _High betweenness centrality (0.159) - this node is a cross-community bridge._
- **Why does `ToolCall` connect `Message` to `chat.rs`?**
  _High betweenness centrality (0.143) - this node is a cross-community bridge._
- **What connects `zakhar`, `Setup`, `Usage` to the rest of the system?**
  _4 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `openai.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.10796221322537113 - nodes in this community are weakly interconnected._
- **Should `chat.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.12631578947368421 - nodes in this community are weakly interconnected._