mod ask;
pub(crate) use ask::load_persisted_todos;
mod compact;
pub(crate) mod context;
mod exec;
pub(crate) use exec::Task;
mod fetch;
mod fs;
mod remind;
mod remember;
mod search;
mod session;
mod system;
mod time;

use crate::handler::Handler;

pub fn all() -> Vec<Box<dyn Handler>> {
    vec![
        Box::new(fs::Read),
        Box::new(fs::Write),
        Box::new(fs::Edit),
        Box::new(fs::Glob),
        Box::new(fs::Grep),
        Box::new(exec::Bash),
        Box::new(exec::Task),
        Box::new(exec::Watch),
        Box::new(ask::Ask),
        Box::new(ask::Todo),
        Box::new(context::Context),
        Box::new(remember::Remember),
        Box::new(compact::Compact),
        Box::new(fetch::Fetch),
        Box::new(search::Search),
        Box::new(remind::Remind),
        Box::new(session::SessionTool),
        Box::new(system::Skill),
        Box::new(system::Control),
        Box::new(time::Time),
    ]
}

pub fn context_index() -> String {
    let store = crate::memory::knowledge::load();
    let mut out = String::new();
    let mut ranked = store.clone();
    ranked.sort_by(|a, b| b.salience.partial_cmp(&a.salience).unwrap_or(std::cmp::Ordering::Equal));
    for item in ranked.iter().take(3) {
        out.push_str(&format!("{}: {}\n", item.summary, item.kind));
    }
    if out.is_empty() {
        out.push_str("no saved knowledge");
    }
    out
}

pub fn context_keys() -> String {
    let store = crate::memory::knowledge::load();
    let summaries: Vec<&str> = store.iter().map(|i| i.summary.as_str()).collect();
    serde_json::to_string(&summaries).unwrap_or_else(|_| "[]".to_string())
}
