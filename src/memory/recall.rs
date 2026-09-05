use std::collections::{BTreeSet, HashMap};

use super::knowledge::Item;

pub struct Hit {
    pub item: Item,
    pub score: f64,
    pub loop_open: bool,
}

const SUMMARY_WEIGHT: f64 = 3.0;
const TAG_WEIGHT: f64 = 2.0;
const DETAIL_WEIGHT: f64 = 1.0;
const FRESH_WEIGHT: f64 = 0.6;
const IMPORT_WEIGHT: f64 = 0.3;
const USE_WEIGHT: f64 = 0.2;
const LOOP_BOOST: f64 = 1.4;
const LINK_BOOST: f64 = 0.06;
const LINK_SCOPE: usize = 12;

const SYNONYMS: &[(&str, &[&str])] = &[
    ("edit", &["change", "modify"]),
    ("write", &["save", "create"]),
    ("delete", &["remove", "drop"]),
    ("read", &["load", "view"]),
    ("query", &["search", "lookup"]),
    ("tool", &["command", "tooling"]),
    ("compile", &["build", "bundle"]),
    ("test", &["check", "verify"]),
    ("user", &["author", "owner"]),
    ("memory", &["context", "notes"]),
    ("commit", &["push", "git"]),
    ("slow", &["lag", "slowly"]),
    ("fast", &["quick", "quickly"]),
    ("bug", &["error", "issue"]),
    ("github", &["repo", "repository"]),
];

fn norm(word: &str) -> String {
    let lower: String = word.chars().map(|c| if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c }).collect();
    let base: String = lower.trim_matches(|c: char| !c.is_alphanumeric()).to_string();
    let stem = if base.len() > 3 {
        if base.ends_with("ing") {
            base[..base.len() - 3].to_string()
        } else if base.ends_with("ed") {
            base[..base.len() - 2].to_string()
        } else if base.ends_with("ies") {
            format!("{}y", &base[..base.len() - 3])
        } else if base.ends_with("es") || base.ends_with("s") {
            base[..base.len() - 1].to_string()
        } else {
            base.clone()
        }
    } else {
        base.clone()
    };
    if stem.is_empty() { base } else { stem }
}

fn family(word: &str) -> Vec<String> {
    let stem = norm(word);
    let mut fam: Vec<String> = SYNONYMS
        .iter()
        .filter(|(k, _)| norm(k) == stem || k.eq_ignore_ascii_case(&stem))
        .flat_map(|(_, alts)| alts.iter().map(|a| norm(a)))
        .collect();
    fam.push(stem);
    fam.sort();
    fam.dedup();
    fam
}

fn terms(text: &str) -> BTreeSet<String> {
    text.split_whitespace()
        .filter(|t| t.chars().count() > 1)
        .flat_map(family)
        .collect()
}

fn corpus_idf(store: &[Item]) -> HashMap<String, f64> {
    let n = store.len().max(1);
    let mut df: HashMap<String, usize> = HashMap::new();
    for item in store {
        let mut seen = BTreeSet::new();
        seen.extend(terms(&item.summary));
        seen.extend(item.tags.iter().flat_map(|t| family(t)));
        for term in seen {
            *df.entry(term).or_insert(0) += 1;
        }
    }
    df.into_iter()
        .map(|(term, count)| {
            let weight = ((n - count) as f64 + 0.5) / (count as f64 + 0.5);
            (term, (weight.ln() + 1.0).max(0.05))
        })
        .collect()
}

fn base_score(item: &Item, query_terms: &BTreeSet<String>, idf: &HashMap<String, f64>) -> f64 {
    let mut total = 0.0;
    for (hay, weight) in [(&item.summary, SUMMARY_WEIGHT), (&item.detail.clone().unwrap_or_default(), DETAIL_WEIGHT)] {
        let hay_terms = terms(hay);
        for term in query_terms {
            if hay_terms.contains(term) {
                total += idf.get(term).copied().unwrap_or(1.0) * weight;
            }
        }
    }
    for tag in &item.tags {
        let tag_family = family(tag);
        for term in query_terms {
            if tag_family.contains(term) {
                total += idf.get(term).copied().unwrap_or(1.0) * TAG_WEIGHT;
            }
        }
    }
    total
}

fn overlap(a: &Item, b: &Item) -> usize {
    let mut set: BTreeSet<String> = terms(&a.summary);
    set.extend(a.tags.iter().flat_map(|t| family(t)));
    let mut count = 0;
    for term in terms(&b.summary) {
        if set.contains(&term) {
            count += 1;
        }
    }
    for tag in &b.tags {
        if family(tag).iter().any(|t| set.contains(t)) {
            count += 1;
        }
    }
    count
}

fn assoc_score(item: &Item, top: &[(f64, Item)]) -> f64 {
    top.iter()
        .filter(|(_, other)| other.id != item.id)
        .filter(|(_, other)| overlap(item, other) >= 2)
        .map(|(score, _)| score)
        .sum::<f64>()
        * LINK_BOOST
}

pub fn remember(query: &str, store: &[Item], k: usize) -> Vec<Hit> {
    let query_terms = terms(query);
    if query_terms.is_empty() || store.is_empty() {
        return Vec::new();
    }
    let idf = corpus_idf(store);
    let mut scored: Vec<(f64, Item)> = store
        .iter()
        .filter_map(|item| {
            let base = base_score(item, &query_terms, &idf);
            if base <= 0.0 {
                return None;
            }
            let days = age_days(&item.accessed).unwrap_or(0.0);
            let fresh = 0.5f64.powf(days / 7.0);
            let import = item.salience;
            let use_count = (item.access_count as f64).ln_1p();
            let boost = if item.open { LOOP_BOOST } else { 1.0 };
            let score = base * (1.0 + FRESH_WEIGHT * fresh + IMPORT_WEIGHT * import + USE_WEIGHT * use_count) * boost;
            Some((score, item.clone()))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let scope = scored.iter().take(LINK_SCOPE).cloned().collect::<Vec<_>>();
    for (score, item) in scored.iter_mut().take(LINK_SCOPE) {
        *score += assoc_score(item, &scope);
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .take(k.max(1))
        .map(|(score, item)| Hit {
            loop_open: item.open,
            score,
            item,
        })
        .collect()
}

fn age_days(ts: &str) -> Option<f64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
    let age = chrono::Utc::now().signed_duration_since(parsed);
    Some(age.num_seconds() as f64 / 86400.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(summary: &str, tags: &[&str], detail: &str) -> Item {
        let mut frag = Item::brand("fact", summary, Some(detail.to_string()), tags.iter().map(|t| t.to_string()).collect(), Vec::new(), "t", false);
        frag.accessed = chrono::Utc::now().to_rfc3339();
        frag
    }

    #[test]
    fn matches_stems_and_synonyms() {
        let store = vec![item("build the watch tool", &["rust"], "long detail"), item("eat pizza", &["food"], "none")];
        let hits = remember("building watch", &store, 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item.summary, "build the watch tool");
    }

    #[test]
    fn rare_term_ranks_higher() {
        let store = vec![
            item("scan every word again", &["c1"], "x"),
            item("rare fossil term now", &["r1"], "x"),
            item("go spread the word out", &["c2"], "x"),
        ];
        for _ in 0..2 {
            let hits = remember("word fossil", &store, 5);
            assert_eq!(hits[0].item.summary, "rare fossil term now", "rarer term must dominate");
        }
    }

    #[test]
    fn open_loop_floats_on_topic() {
        let mut loop_item = item("finish the auth refactor", &["auth"], "pending");
        loop_item.open = true;
        let store = vec![item("auth basics recorded", &["auth"], "done"), loop_item];
        let hits = remember("auth refactor", &store, 5);
        assert!(hits[0].loop_open, "open loop must lead on its topic");
    }

    #[test]
    fn no_match_empty() {
        let store = vec![item("gardening tips", &["plants"], "x")];
        assert!(remember("quantum physics", &store, 5).is_empty());
    }
}