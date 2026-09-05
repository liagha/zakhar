mod ask;
pub(crate) use ask::load_persisted_todos;
pub(crate) mod context;
mod exec;
pub(crate) use exec::Task;
mod fetch;
mod fs;
mod remind;
mod search;
mod session;
mod system;

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
        Box::new(fetch::Fetch),
        Box::new(search::Search),
        Box::new(remind::Remind),
        Box::new(session::SessionTool),
        Box::new(system::Skill),
        Box::new(system::Control),
    ]
}

pub fn context_index() -> String {
    context::index()
}

pub fn context_keys() -> String {
    context::context_keys()
}
