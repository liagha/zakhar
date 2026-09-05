use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};

const FILE: &str = ".zakhar/memory/knowledge.jsonl";
const SALIENCE_HALF_DAYS: f64 = 30.0;
const WARM_HALF_DAYS: f64 = 7.0;
const SALIENCE_MAX: f64 = 1.0;
const SALIENCE_BASE: f64 = 0.6;
const PRUNE_SALIENCE: f64 = 0.2;
const PRUNE_DAYS: f64 = 90.0;

static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static MIGRATED: OnceLock<()> = OnceLock::new();

#[cfg(test)]
pub(crate) fn set_path(p: PathBuf) {
    let cell = OVERRIDE.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(p);
}

fn path() -> PathBuf {
    if let Some(over) = OVERRIDE.get()
        && let Some(p) = over.lock().unwrap().clone()
    {
        return p;
    }
    PathBuf::from(FILE)
}

pub fn store_path(root: &Path) -> PathBuf {
    root.join(".zakhar").join("memory").join("knowledge.jsonl")
}

pub fn uid() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub refs: Vec<String>,
    pub salience: f64,
    pub updated: String,
    pub accessed: String,
    #[serde(default)]
    pub access_count: u64,
    pub origin: String,
    #[serde(default)]
    pub open: bool,
}

impl Item {
    pub fn brand(kind: &str, summary: &str, detail: Option<String>, tags: Vec<String>, refs: Vec<String>, origin: &str, open: bool) -> Self {
        let stamp = now();
        Self {
            id: uid(),
            kind: kind.to_string(),
            summary: summary.to_string(),
            detail,
            tags,
            refs,
            salience: SALIENCE_BASE,
            updated: stamp.clone(),
            accessed: stamp,
            access_count: 0,
            origin: origin.to_string(),
            open,
        }
    }
}

pub fn load() -> Vec<Item> {
    load_path(&path())
}

pub fn load_path(p: &Path) -> Vec<Item> {
    std::fs::read_to_string(p)
        .ok()
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_path(p: &Path, store: &[Item]) -> anyhow::Result<()> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = format!(".{}.tmp", p.file_name().map(|f| f.to_string_lossy()).unwrap_or_default());
    let tmp = p.with_file_name(tmp);
    let mut out = String::new();
    for item in store {
        out.push_str(&serde_json::to_string(item)?);
        out.push('\n');
    }
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

fn save(store: &[Item]) -> anyhow::Result<()> {
    save_path(&path(), store)
}

pub fn load_root(root: &Path) -> Vec<Item> {
    load_path(&store_path(root))
}

pub fn save_root(root: &Path, store: &[Item]) -> anyhow::Result<()> {
    save_path(&store_path(root), store)
}

pub fn get(id: &str) -> Option<Item> {
    load().into_iter().find(|i| i.id == id)
}

pub fn find(key: &str) -> Option<Item> {
    let store = load();
    let lower = key.to_lowercase();
    store
        .iter()
        .find(|i| i.id == key || i.summary.eq_ignore_ascii_case(key))
        .cloned()
        .or_else(|| store.iter().find(|i| i.summary.to_lowercase().contains(&lower)).cloned())
}

pub fn touch(id: &str) -> anyhow::Result<bool> {
    let mut store = load();
    match store.iter_mut().find(|i| i.id == id) {
        Some(item) => {
            item.accessed = now();
            item.access_count += 1;
            save(&store)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

pub fn save_pair(key: &str, value: &str, source: &str) -> anyhow::Result<(String, bool)> {
    let mut store = load();
    if let Some(item) = store.iter_mut().find(|i| i.summary.eq_ignore_ascii_case(key)) {
        let keep = if source.is_empty() { !item.origin.is_empty() } else { false };
        item.detail = Some(value.to_string());
        item.updated = now();
        item.salience = (item.salience + 0.2).min(SALIENCE_MAX);
        if !keep && !source.is_empty() {
            item.origin = source.to_string();
        }
        let id = item.id.clone();
        save(&store)?;
        return Ok((id, false));
    }
    let mut tags: Vec<String> = key
        .split_whitespace()
        .map(|t| t.to_lowercase())
        .filter(|t| t.chars().count() > 2)
        .collect();
    tags.sort();
    tags.dedup();
    let item = Item::brand("conversation", key, Some(value.to_string()), tags, Vec::new(), if source.is_empty() { "conversation" } else { source }, false);
    let id = item.id.clone();
    store.push(item);
    save(&store)?;
    Ok((id, true))
}

pub fn remove(key: &str) -> anyhow::Result<Option<Item>> {
    let mut store = load();
    let index = store.iter().position(|i| i.id == key || i.summary.eq_ignore_ascii_case(key));
    let removed = match index {
        Some(i) => store.remove(i),
        None => return Ok(None),
    };
    save(&store)?;
    Ok(Some(removed))
}

pub fn decay(store: &mut [Item]) {
    for item in store {
        let days = age_days(&item.updated).unwrap_or(0.0);
        item.salience = item.salience.mul_add(0.5f64.powf(days / SALIENCE_HALF_DAYS), 0.0);
        item.salience = item.salience.max(0.01);
    }
}

pub fn prune_run() -> Vec<Item> {
    let mut store = load();
    decay(&mut store);
    let (removed, kept): (Vec<Item>, Vec<Item>) = store
        .into_iter()
        .partition(|i| {
            i.salience < PRUNE_SALIENCE
                && age_days(&i.accessed).is_some_and(|d| d > PRUNE_DAYS)
        });
    let _ = save(&kept);
    removed
}

pub fn top(n: usize) -> Vec<Item> {
    let store = load();
    if store.is_empty() {
        return store;
    }
    let score = |item: &Item| {
        let warm = age_days(&item.accessed).map_or(1.0, |d| 0.5f64.powf(d / WARM_HALF_DAYS));
        let fresh = age_days(&item.updated).map_or(1.0, |d| 0.5f64.powf(d / 14.0));
        item.salience * warm + fresh
    };
    let mut ranked: Vec<Item> = store;
    ranked.sort_by(|a, b| score(b).partial_cmp(&score(a)).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(n.max(1));
    ranked
}

pub fn keys() -> Vec<String> {
    let mut keys: Vec<String> = load().into_iter().map(|i| i.summary).collect();
    keys.sort();
    keys
}

pub fn stale(days: u64) -> Vec<String> {
    let mut stale: Vec<String> = load()
        .into_iter()
        .filter(|i| age_days(&i.accessed).is_some_and(|d| d > days as f64))
        .map(|i| i.summary)
        .collect();
    stale.sort();
    stale
}

pub fn block(n: usize) -> String {
    let store = load();
    if store.is_empty() {
        return "no saved knowledge".to_string();
    }
    let mut out = String::new();
    let mut ranked = store.clone();
    ranked.sort_by(|a, b| b.salience.partial_cmp(&a.salience).unwrap_or(std::cmp::Ordering::Equal));
    for item in ranked.iter().take(n.max(1)) {
        let detail = item
            .detail
            .as_ref()
            .map(|d| format!(" — {}", preview(d, 60)))
            .unwrap_or_default();
        let stamp: String = item.updated.chars().take(10).collect();
        out.push_str(&format!("- {} ({}){detail} (since {stamp})\n", item.summary, item.kind));
    }
    for item in &ranked {
        if item.open {
            out.push_str(&format!("OPEN: {}\n", item.summary));
        }
    }
    out
}

fn age_days(ts: &str) -> Option<f64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
    let age = Utc::now().signed_duration_since(parsed);
    Some(age.num_seconds() as f64 / 86400.0)
}

fn preview(s: &str, cap: usize) -> String {
    let ellipsis = if s.chars().count() > cap { "…" } else { "" };
    let mut out: String = s.chars().take(cap).collect();
    out.push_str(ellipsis);
    out
}

pub fn migrate_once() -> usize {
    let _ = MIGRATED.get_or_init(|| {});
    let raw = match std::fs::read_to_string(".zakhar/context.json") {
        Ok(t) => t,
        Err(_) => return 0,
    };
    if !load().is_empty() {
        return 0;
    }
    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let mut store = Vec::new();
    if let Some(entries) = parsed.get("entries").and_then(|e| e.as_object()) {
        for (key, meta) in entries {
            let value = meta.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let updated = meta.get("updated").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let accessed = meta.get("accessed_at").and_then(|v| v.as_str()).unwrap_or(&updated).to_string();
            let count = meta.get("access_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let origin = meta.get("source").and_then(|v| v.as_str()).unwrap_or("migrated-context").to_string();
            let mut tags: Vec<String> = key.to_lowercase().split_whitespace().map(|t| t.to_string()).filter(|t| t.chars().count() > 2).collect();
            tags.sort();
            tags.dedup();
            let mut item = Item::brand("fact", key, (!value.is_empty()).then_some(value), tags, Vec::new(), &origin, false);
            item.updated = updated;
            item.accessed = accessed;
            item.access_count = count;
            item.salience = SALIENCE_BASE;
            store.push(item);
        }
    }
    if store.is_empty() {
        return 0;
    }
    if save(&store).is_err() {
        return 0;
    }
    let _ = std::fs::remove_file(".zakhar/context.json");
    store.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.keep().join("knowledge.jsonl");
        set_path(path.clone());
        let _ = std::fs::remove_file(&path);
        guard
    }

    #[test]
    fn roundtrip_and_find() {
        let _g = tmp_store();
        let (id, is_new) = save_pair("plan", "build watch tool", "file:todo.md").unwrap();
        assert!(is_new);
        assert_eq!(find("plan").unwrap().detail.as_deref(), Some("build watch tool"));
        assert_eq!(get(&id).unwrap().origin, "file:todo.md");
    }

    #[test]
    fn save_pair_updates_in_place() {
        let _g = tmp_store();
        let (id, first) = save_pair("plan", "v1", "").unwrap();
        assert!(first);
        let (same, second) = save_pair("plan", "v2", "").unwrap();
        assert_eq!(id, same);
        assert!(!second);
        assert_eq!(load().len(), 1);
    }

    #[test]
    fn touch_bumps_access() {
        let _g = tmp_store();
        let (id, _) = save_pair("k", "v", "").unwrap();
        assert!(touch(&id).unwrap());
        assert_eq!(get(&id).unwrap().access_count, 1);
    }

    #[test]
    fn remove_drops_item() {
        let _g = tmp_store();
        save_pair("k", "v", "").unwrap();
        assert!(remove("k").unwrap().is_some());
        assert!(find("k").is_none());
        assert!(remove("k").unwrap().is_none());
    }

    #[test]
    fn decay_then_prune() {
        let _g = tmp_store();
        let (id, _) = save_pair("old", "x", "").unwrap();
        let mut item = get(&id).unwrap();
        item.updated = (Utc::now() - chrono::Duration::days(300)).to_rfc3339();
        item.accessed = item.updated.clone();
        save(&[item.clone()]).unwrap();
        let pruned = prune_run();
        assert!(pruned.iter().any(|i| i.id == id), "old weak item must be pruned");
    }

    #[test]
    fn migrate_converts_context_and_removes_file() {
        let _g = tmp_store();
        let cwd = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        std::fs::create_dir_all(".zakhar").unwrap();
        std::fs::write(
            ".zakhar/context.json",
            r#"{"entries":{"plan":{"value":"build tool","updated":"2026-01-01T00:00:00Z","accessed_at":"2026-01-01T00:00:00Z","access_count":3,"source":"session-1"}}}"#,
        )
        .unwrap();
        let migrated = {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| migrate_once()));
            std::env::set_current_dir(&cwd).unwrap();
            result.unwrap()
        };
        assert_eq!(migrated, 1);
        assert!(!dir.path().join(".zakhar/context.json").exists());
    }

    #[test]
    fn keys_sorted_and_block_lists_open() {
        let _g = tmp_store();
        save_pair("zeta", "a", "m").unwrap();
        save_pair("alpha", "b", "m").unwrap();
        assert_eq!(keys(), vec!["alpha".to_string(), "zeta".to_string()]);
        let text = block(5);
        assert!(text.contains("alpha"));
    }
}