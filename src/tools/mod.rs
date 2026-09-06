mod ask;
pub(crate) use ask::load_persisted_todos;
mod calc;
mod clipboard;
mod compact;
pub(crate) mod context;
mod env;
mod exec;
pub(crate) use exec::Task;
mod fetch;
mod fs;
mod json;
mod process;
mod regex;
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
        Box::new(calc::Calc),
        Box::new(clipboard::Clipboard),
        Box::new(env::Env),
        Box::new(json::Json),
        Box::new(process::Ps),
        Box::new(process::Kill),
        Box::new(regex::Regex),
    ]
}

pub fn context_keys() -> String {
    let store = crate::memory::knowledge::load();
    let summaries: Vec<&str> = store.iter().map(|i| i.summary.as_str()).collect();
    serde_json::to_string(&summaries).unwrap_or_else(|_| "[]".to_string())
}
