mod ask;
mod context;
mod exec;
mod fs;
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
        Box::new(system::Skill),
        Box::new(system::Control),
    ]
}

pub fn context_index() -> String {
    context::index()
}
