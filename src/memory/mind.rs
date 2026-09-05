use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::episodic;
use super::knowledge::{self, Item};
use crate::provider::Provider;

const WINDOW: usize = 200;
const SOURCE: &str = "mind";
const STALE_MINS: i64 = 30;
const MAX_SUMMARY: usize = 200;
const KIND_FALLBACK: &str = "fact";
const LOOP_KIND: &str = "open_loop";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Draft {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    refs: Vec<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct Proposal {
    #[serde(default)]
    candidates: Vec<Draft>,
    #[serde(default)]
    loops: Vec<Draft>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Plan {
    #[serde(default)]
    add: Vec<Draft>,
    #[serde(default)]
    bump: Vec<Bump>,
    #[serde(default)]
    drop: Vec<String>,
    #[serde(default)]
    loops: Vec<Draft>,
    #[serde(default)]
    journal: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Bump {
    id: String,
    salience: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Marker {
    #[serde(default)]
    last_ts: String,
    #[serde(default)]
    in_flight: bool,
    #[serde(default)]
    run_at: String,
}

pub fn dispatch(root: &Path) -> anyhow::Result<()> {
    if cfg!(test) || std::env::var("ZAKHAR_NO_MIND").is_ok() {
        return Ok(());
    }
    crate::memory::jobs::enqueue("mind", root, None)
}

pub async fn run(root: &Path, provider: &dyn Provider, model: &str) -> anyhow::Result<()> {
    let mut marker = load_marker(root);
    let (events, last) = new_events(root, &marker.last_ts);
    if events.is_empty() {
        marker.in_flight = false;
        marker.run_at = now();
        let _ = save_marker(root, &marker);
        let _ = log(root, "mind: nothing new to distill");
        return Ok(());
    }
    if marker.in_flight && !stale(&marker.run_at) {
        let _ = log(root, "mind: already running");
        return Ok(());
    }
    marker.in_flight = true;
    marker.run_at = now();
    save_marker(root, &marker)?;

    let outcome = pipeline(root, provider, model, &events, &knowledge::load_root(root)).await;

    match outcome {
        Ok(stats) => {
            marker.last_ts = last;
            marker.in_flight = false;
            save_marker(root, &marker)?;
            let _ = log(
                root,
                &format!(
                    "mind: distilled {} events → {} added, {} reinforced, {} dropped",
                    events.len(),
                    stats.added,
                    stats.bumped,
                    stats.dropped
                ),
            );
        }
        Err(e) => {
            marker.in_flight = false;
            let _ = save_marker(root, &marker);
            let _ = log(root, &format!("mind: failed: {e}"));
        }
    }
    Ok(())
}

struct Stats {
    added: usize,
    bumped: usize,
    dropped: usize,
}

async fn pipeline(
    root: &Path,
    provider: &dyn Provider,
    model: &str,
    events: &[episodic::Event],
    store: &[Item],
) -> anyhow::Result<Stats> {
    let proposal = archivist(provider, model, events, store).await?;
    let plan = critic(provider, model, &proposal, store).await?;
    let plan = validator(provider, model, &plan, store).await?;
    apply(root, events, store, &plan)
}

async fn archivist(provider: &dyn Provider, model: &str, events: &[episodic::Event], store: &[Item]) -> anyhow::Result<Proposal> {
    let event_text = events
        .iter()
        .enumerate()
        .map(|(i, e)| format!("[{i}] [{}] {}: {}", e.ts, e.kind, e.text))
        .collect::<Vec<_>>()
        .join("\n");
    let store_text = summaries(store);
    let system = "You are a memory archivist. From a batch of chronological work events you \
                  extract durable knowledge: stable facts, decisions, preferences, skills, or \
                  domain notes. Skip noise, greetings, and transient state. Emit each as a \
                  candidate with kind (fact|decision|preference|skill), a one-line summary \
                  under 90 characters, an optional detail, up to 4 short tags, and refs = the \
                  0-based event indices it came from. Also emit loops: open threads the events \
                  started but did not finish. Respond with ONLY a JSON object like \
                  {\"candidates\":[{\"kind\":\"fact\",\"summary\":\"...\",\"detail\":\"...\",\"tags\":[\"a\"],\"refs\":[0]}],\"loops\":[{\"summary\":\"...\"}]}. \
                  Empty arrays are allowed.";
    let user = format!(
        "Current knowledge: {store_text}\n\nEvents:\n{event_text}\n\nExtract candidate memories now."
    );
    let value = call(provider, model, system, user, 1024).await?;
    Ok(serde_json::from_value(value)?)
}

async fn critic(provider: &dyn Provider, model: &str, proposal: &Proposal, store: &[Item]) -> anyhow::Result<Plan> {
    let candidates = serde_json::to_string(&proposal.candidates)?;
    let loops = serde_json::to_string(
        &proposal
            .loops
            .iter()
            .map(|l| serde_json::json!({"summary": l.summary}))
            .collect::<Vec<_>>(),
    )?;
    let system = "You are a memory critic. Review the archivist's candidate memories against \
                  the current knowledge store (id, kind, summary, salience). Accept durable \
                  ones, merge duplicates with existing items, drop noise. Decide which existing \
                  items to delete (drop, by id) and which to reinforce (bump, id + salience \
                  0-1). Return the final plan as ONLY JSON: {\"add\":[{\"kind\",\"summary\",\
                  \"detail\",\"tags\",\"refs\"}],\"bump\":[{\"id\",\"salience\"}],\"drop\":[\"id\"],\
                  \"loops\":[{\"summary\"}],\"journal\":\"one present-tense sentence\"}. Keep \
                  summaries under 90 characters; only include items that changed.";
    let user = format!(
        "Store:\n{store_lines}\n\nArchivist candidates: {candidates}\n\nArchivist loops: {loops}\n\nProduce the plan.",
        store_lines = {
            store.iter()
                .map(|i| format!("{} | {} | {}", i.id, i.kind, i.summary))
                .collect::<Vec<_>>()
                .join("\n")
        }
    );
    let value = call(provider, model, system, user, 1024).await?;
    Ok(serde_json::from_value(value)?)
}

async fn validator(provider: &dyn Provider, model: &str, plan: &Plan, store: &[Item]) -> anyhow::Result<Plan> {
    let system = "You are a memory validator, the final check before memories are written. The \
                  critic produced a plan against the current store. Veto any add whose summary \
                  is vague or duplicates an existing item under different wording, any bump of a \
                  nonexistent or irrelevant id, any drop that loses still-valuable knowledge. \
                  Re-emit the corrected plan with EXACTLY the same JSON shape as the input, even \
                  if unchanged. Prefer fewer, stronger items.";
    let plan_text = serde_json::to_string(plan)?;
    let user = format!(
        "Store:\n{store_lines}\n\nCritic plan:\n{plan_text}\n\nEmit the final validated plan.",
        store_lines = summaries(store)
    );
    let value = call(provider, model, system, user, 1024).await?;
    Ok(serde_json::from_value(value)?)
}

fn apply(root: &Path, events: &[episodic::Event], store: &[Item], plan: &Plan) -> anyhow::Result<Stats> {
    let mut list = store.to_vec();
    let dropped = plan
        .drop
        .iter()
        .filter(|id| {
            let before = list.len();
            list.retain(|i| &i.id != *id);
            list.len() < before
        })
        .count();
    let bumped = plan
        .bump
        .iter()
        .filter(|b| match list.iter_mut().find(|i| i.id == b.id) {
            Some(item) => {
                item.salience = (item.salience * 0.5 + b.salience * 0.5).min(1.0);
                item.updated = now();
                true
            }
            None => false,
        })
        .count();
    let mut added = 0;
    for draft in &plan.add {
        let summary = draft.summary.trim();
        if summary.is_empty() || summary.chars().count() > MAX_SUMMARY || duplicate(&summary, &list, false) {
            continue;
        }
        let refs = draft
            .refs
            .iter()
            .filter_map(|&i| events.get(i).map(|e| e.ts.clone()))
            .collect();
        let kind = if draft.kind.is_empty() { KIND_FALLBACK } else { &draft.kind };
        let mut tags = draft.tags.clone();
        tags.sort();
        tags.dedup();
        list.push(Item::brand(kind, &summary, draft.detail.clone(), tags, refs, SOURCE, false));
        added += 1;
    }
    let mut loops = 0;
    for draft in &plan.loops {
        let summary = draft.summary.trim();
        if summary.is_empty() || duplicate(&summary, &list, true) {
            continue;
        }
        list.push(Item::brand(LOOP_KIND, &summary, None, Vec::new(), Vec::new(), SOURCE, true));
        loops += 1;
    }
    knowledge::decay(&mut list);
    knowledge::save_root(root, &list)?;

    let stats = Stats { added: added + loops, bumped, dropped };
    let stamp = Utc::now().format("%Y-%m-%d %H:%M");
    let entry = format!(
        "\n## Mind @ {stamp}\n{}\nnew {stats_added} · reinforced {bumped} · dropped {dropped}\n",
        plan.journal.trim(),
        stats_added = stats.added
    );
    let notes = root.join(".zakhar").join("NOTES.md");
    if let Some(dir) = notes.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut notes_file = std::fs::OpenOptions::new().create(true).append(true).open(&notes)?;
    writeln!(notes_file, "{entry}")?;
    Ok(stats)
}

fn duplicate(summary: &str, list: &[Item], loops: bool) -> bool {
    let lower = summary.to_lowercase();
    list.iter().any(|i| {
        if loops {
            i.open && i.summary.to_lowercase() == lower
        } else {
            i.summary.to_lowercase() == lower || i.summary.to_lowercase().contains(&lower)
        }
    })
}

fn summaries(store: &[Item]) -> String {
    if store.is_empty() {
        return "(empty)".to_string();
    }
    store
        .iter()
        .map(|i| format!("{} | {} | {}", i.id, i.kind, i.summary))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn call(provider: &dyn Provider, model: &str, system: &str, user: String, budget: u32) -> anyhow::Result<serde_json::Value> {
    let mut attempt = 0;
    let mut subject = user;
    loop {
        let request = crate::types::ChatRequest {
            model: model.to_string(),
            messages: vec![
                crate::types::Message::system(system.to_string()),
                crate::types::Message::user(subject.clone()),
            ],
            temperature: Some(0.2),
            max_tokens: Some(budget),
            stream: Some(false),
            tools: None,
        };
        let mut stream = provider.chat_stream(request).await?;
        let mut text = String::new();
        while let Some(event) = futures::StreamExt::next(&mut stream).await {
            match event? {
                crate::provider::ChatStreamEvent::Text(t) => text.push_str(&t),
                crate::provider::ChatStreamEvent::Done => break,
                _ => {}
            }
        }
        if let Ok(value) = parse_json(&text) {
            return Ok(value);
        }
        attempt += 1;
        if attempt >= 2 {
            let preview: String = text.chars().take(120).collect();
            anyhow::bail!("model returned unparseable JSON: {preview}");
        }
        subject = format!("{subject}\n\nReply with ONLY valid JSON. No markdown fences.");
    }
}

fn parse_json(raw: &str) -> anyhow::Result<serde_json::Value> {
    let mut oneline = raw.trim();
    if let Some(stripped) = oneline.strip_prefix("```") {
        match stripped.find('\n') {
            Some(i) => {
                oneline = &stripped[i + 1..];
                if let Some(end) = oneline.rfind("```") {
                    oneline = &oneline[..end];
                }
            }
            None => oneline = &stripped,
        }
    }
    let start = oneline.find('{').ok_or_else(|| anyhow::anyhow!("no object"))?;
    let end = oneline.rfind('}').ok_or_else(|| anyhow::anyhow!("no object end"))?;
    serde_json::from_str(&oneline[start..=end]).map_err(|e| anyhow::anyhow!("{e}"))
}

fn new_events(root: &Path, since: &str) -> (Vec<episodic::Event>, String) {
    let events = episodic::read_events(&root.join(".zakhar").join("memory").join("episodic.jsonl"));
    let mut fresh: Vec<episodic::Event> = if since.is_empty() {
        events
    } else {
        events.into_iter().filter(|e| e.ts.as_str() > since).collect()
    };
    let last = fresh.iter().map(|e| e.ts.as_str()).max().unwrap_or("").to_string();
    if last.is_empty() {
        return (Vec::new(), last);
    }
    if fresh.len() > WINDOW {
        let split = fresh.len() - WINDOW;
        fresh.drain(..split);
    }
    (fresh, last)
}

fn marker_path(root: &Path) -> PathBuf {
    root.join(".zakhar").join("memory").join("mind.json")
}

fn load_marker(root: &Path) -> Marker {
    std::fs::read_to_string(marker_path(root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_marker(root: &Path, marker: &Marker) -> anyhow::Result<()> {
    let path = marker_path(root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(marker)?)?;
    Ok(())
}

fn stale(ts: &str) -> bool {
    match chrono::DateTime::parse_from_rfc3339(ts) {
        Ok(parsed) => {
            let age = Utc::now().signed_duration_since(parsed);
            age.num_minutes() > STALE_MINS
        }
        Err(_) => true,
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn log(root: &Path, line: &str) -> anyhow::Result<()> {
    let path = root.join(".zakhar").join("memory").join("mind.log");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        (dir, guard)
    }

    fn event(i: usize) -> episodic::Event {
        episodic::Event {
            ts: format!("2026-01-0{}T00:00:00Z", i % 9 + 1),
            kind: "chat".to_string(),
            text: format!("event {i}"),
        }
    }

    #[test]
    fn parse_json_handles_fences_and_prose() {
        let raw = "Sure!\n```json\n{\"add\":[{\"summary\":\"x\"}]}\n```\n";
        let value = parse_json(raw).unwrap();
        assert_eq!(value["add"][0]["summary"], "x");
    }

    #[test]
    fn new_events_respects_marker() {
        let (dir, _g) = temp_root();
        let root = dir.keep();
        let p = root.join(".zakhar/memory/episodic.jsonl");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(&p).unwrap();
        for e in [event(1), event(2), event(3)] {
            writeln!(file, "{}", serde_json::to_string(&e).unwrap()).unwrap();
        }
        let (all, last) = new_events(&root, "");
        assert_eq!(all.len(), 3);
        assert_eq!(last, "2026-01-04T00:00:00Z");
        let (after, _) = new_events(&root, &last);
        assert!(after.is_empty());
    }

    #[test]
    fn apply_merges_drops_and_prunes() {
        let (dir, _g) = temp_root();
        let root = dir.keep();
        let store = vec![
            Item::brand("fact", "old plan", Some("x".to_string()), vec!["p".to_string()], Vec::new(), "t", false),
            Item::brand("decision", "keep me forever", None, Vec::new(), vec!["r1".to_string()], "t", false),
        ];
        let events: Vec<episodic::Event> = vec![event(1), event(2)];
        let plan = Plan {
            add: vec![
                Draft { kind: "decision".into(), summary: "switch to rust for the engine".into(), detail: None, tags: vec!["rust".into()], refs: vec![0] },
                Draft { kind: "fact".into(), summary: "old plan".into(), detail: None, tags: vec![], refs: vec![] },
            ],
            bump: vec![Bump { id: store[0].id.clone(), salience: 0.9 }],
            drop: vec![store[1].id.clone()],
            loops: vec![Draft { kind: LOOP_KIND.into(), summary: "finish the rewrite".into(), detail: None, tags: vec![], refs: vec![] }],
            journal: "rewrote the engine plan".into(),
        };
        let stats = apply(&root, &events, &store, &plan).unwrap();
        assert_eq!(stats.added, 2, "one new item + one loop");
        assert_eq!(stats.bumped, 1);
        assert_eq!(stats.dropped, 1);
        let saved = knowledge::load_root(&root);
        assert!(!saved.iter().any(|i| i.summary == "keep me forever"));
        assert!(saved.iter().any(|i| i.summary == "old plan"));
        assert!(saved.iter().any(|i| i.open && i.summary == "finish the rewrite"));
        assert!(saved.iter().any(|i| i.summary == "switch to rust for the engine"));
    }

    #[test]
    fn marker_claim_release() {
        let (dir, _g) = temp_root();
        let root = dir.keep();
        let mut marker = load_marker(&root);
        assert!(!marker.in_flight);
        marker.in_flight = true;
        marker.run_at = now();
        save_marker(&root, &marker).unwrap();
        let reload = load_marker(&root);
        assert!(reload.in_flight);
    }

    #[test]
    fn duplicate_detects_near_matches() {
        let list = vec![Item::brand("fact", "Use Vazir for logos", None, Vec::new(), Vec::new(), "t", false)];
        assert!(duplicate("USE VAZIR FOR LOGOS", &list, false));
        assert!(duplicate("for logos", &list, false));
        assert!(!duplicate("different thing", &list, false));
        assert!(!duplicate("for logos", &list, true), "open-loop check must require flagged items");
    }
}