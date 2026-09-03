# zakhar — roadmap

Direction for zakhar ("a person who remembers"): multi-agent CLI for working with
AI coding models, with persistent identity, layered memory, continuity across the
stateless `zakhar <phrase>` one-shots, skills, and (soon) a mobile companion app.

Grounded in current code. Completed work is struck through and kept only as a
reference to the commit that landed it.

---

## ✅ Phase 1 — Identity, continuity & foundational memory — DONE

Landing commit: `d2275e0` (trait-based tool unification + context + watch tools)
and the memory work across the recent commits.

- ~~1.1 `src/memory/` module (`load_blocks()` layered blocks)~~ — DONE
- ~~1.2 Episodic logging (`append`/`recent`/`compact`, CAP 500 / roll 100, auto-compact)~~ — DONE
- ~~1.3 Inject profile + recent events + static memory + context into one-shots and chat~~ — DONE
- ~~1.4 Continuity via `.zakhar/NOTES.md` (append-only)~~ — DONE
- ~~1.5 Skill loading + inject instructions as a follow-up system message~~ — DONE (chat.rs)
- ~~1.6 Tests for memory + skills~~ — DONE

---

## 🧠 Phase 2 — Semantic recall, compaction, persistent tasks, memory UX

### 2.1 Semantic memory (partially done)
- ~~`context::recall` keyword/BM25 ranker~~ — DONE (src/tools/context.rs)
- ⬜ **Importance / `accessed_at` scoring** — track access frequency per key; rank
  `recall` by recency + importance, not just keyword count. Stale/low-value keys
  surface for pruning. (Entry currently has only `value` + `updated`.)

### 2.2 Compaction / archive (partially done)
- ~~Raw-archive compaction (roll old events to `.zakhar/memory/archive/*.jsonl` + NOTES.md stub)~~ — DONE
- ⬜ **LLM-based summarisation** — when the log exceeds CAP, call the model to
  distill the oldest chunk into a tight *human-readable* prose summary appended to
  `profile.md` / `NOTES.md` (not just raw JSONL). This is the missing piece of
  real "never forgets" memory.

### 2.3 Persistent background tasks
- ⬜ **`watch`/`task` survive restart** — persist running task state + output to
  `.zakhar/tasks/` so long builds / `tail -f` survive zakhar restarts; `/tasks resume` path.

### 2.4 Memory UX / provenance
- ~~`/memory` command (browse / search / drop / compact)~~ — DONE (src/slash.rs)
- ⬜ **Provenance** — every `context` fact records where it came from (turn, file,
  timestamp); `/memory` shows it. "Why do I believe this?" for trust.

### 2.5 Optional power-ups
- ⬜ **MCP server support** — host external MCP tools behind the existing `Handler` harness.
- ⬜ **Session resume** — `zakhar sessions` list + `zakhar chat --continue <id>`/`--resume`.
  (sessions already stored; just no resume path yet.)

---

## 🆕 Phase 3 — New capabilities & integration

### A. Memory & continuity
- ⬜ **A1 `context` namespacing / scopes** — optional `--scope` so project-specific
  vs global user facts stay separate, or a shared cross-project global memory.

### B. Session & workflow
- ⬜ **B1 Session resume** (moved to 2.5 above — dedupe).
- ⬜ **B2 Persistent named tasks** (moved to 2.3 above — dedupe).
- ⬜ **B3 `--diff` review of changes** — auto `git diff` summary after mutating turns
  so the user can review AI changes before they become permanent.
- ⬜ **B4 Multi-question `ask`** — accept a list of questions, render in one pass;
  plus a `confirm` variant returning yes/no (no free text).

### C. Tools & capabilities
- ⬜ **C1 Web `fetch` tool** — HTTP GET a URL, strip HTML, return text (uses existing
  `reqwest`). Optional `search` helper for research.
- ⬜ **C2 File tree / project map tool** — compact project structure honoring
  `.gitignore`, so the model orients in unfamiliar repos without spraying `glob`s.
- ⬜ **C3 `todo` persistence** — persist to `.zakhar/todo.json` and surface incomplete
  high-priority items at session start.
- ⬜ **C4 Apply-patch / line-anchored `edit`** — structured diffs over exact-string matches.
- ⬜ **C5 More runner hooks** — `SessionStart`, `SessionEnd`, `CommandStart` (logging,
  desktop notify on long commands).

### D. UI & UX
- ⬜ **D1 Configurable themes / color scheme** — `[ui]` theme block; respect
  `NO_COLOR` / `CLICOLOR`.
- ⬜ **D2 Streaming markdown + links/images/code copy** — incremental rendering,
  better links/images and code-fence handling.
- ⬜ **D3 Status line widgets** — token count, elapsed time, provider/model, tool spinner.
- ⬜ **D4 `--headless` / `--json` mode** — structured machine-readable output for scripting.

### E. Integration & platform
- ⬜ **E1 MCP client** (moved to 2.5 above — dedupe).
- ⬜ **E2 Git integration tool** — `git` handler (status/diff/log) with safe mutation gating.
- ⬜ **E3 Project auto-init / `--init` wizard** — scaffold `.zakhar/`, detect language/framework.
- ⬜ **E4 Multi-provider failover / health check** — auto-retry with another provider on
  rate-limit/error.

### F. Robustness & engineering
- ⬜ **F1 Shell completions** — `zakhar completion bash|zsh|fish` (clap_complete already a dep).
- ⬜ **F2 Structured tool results** — return tool results as JSON for predictable parsing.
- ⬜ **F3 Atomic `context` writes** — write temp + rename so a killed process can't corrupt
  `.zakhar/context.json`.
- ⬜ **F4 Tracing spans** — wrap tool calls + LLM requests so `RUST_LOG=debug` shows the tour.

---

## 📱 Phase 4 — Mobile companion app

Goal: a mobile app to talk to / operate zakhar from a phone.

- ⬜ **4.0 Architecture + transport decision** — how the phone talks to the host
  (SSH, a small daemon/HTTP/WebSocket server in zakhar, or a hosted relay). Pick
  auth + protocol (JSON-RPC / SSE / WebSocket streaming).
- ⬜ **4.1 Backend bridge in zakhar** — a `serve`-style daemon exposing chat + tool
  invocation over the wire (note: `serve` was removed in `c7cb1df`; re-architect as
  a thin RPC layer over the existing `Runner`/`Invoke`).
- ⬜ **4.2 Mobile UI** — chat screen, markdown rendering, tool-approval prompts
  (mirror of the CLI `ask`/confirm), session list, memory browser.
- ⬜ **4.3 Notifications / hooks** — long-task completion push via hooks.
- ⬜ **4.4 Auth + pairing** — device pairing / token / keychain.

---

## Suggested sequence

Phase 2 (finish the memory vision) → Phase 3 (capability + polish) → Phase 4 (mobile).

Top picks by ROI:
1. **2.2 LLM-based compaction** — the missing piece of real long-term memory.
2. **C1 Web `fetch` tool** — big capability jump, cheap with existing `reqwest`.
3. **2.5 Session resume** — makes chat usable as a real workflow.
4. **F3 Atomic context writes** — quick reliability win.
5. **F1 Shell completions** — trivial to wire (clap_complete already imported).
6. Then **Phase 4 mobile app** (my next task with Ali).
