pub mod markdown;
pub mod modern;
pub mod palette;
pub mod simple;

use modern::Modern;
use palette::Palette;
use simple::Simple;

pub enum Ui<'a> {
    Simple(Simple<'a>),
    Modern(Modern<'a>),
}

impl<'a> Ui<'a> {
    pub fn new(simple: bool, palette: &'a Palette) -> Self {
        if simple {
            Ui::Simple(Simple::new(palette))
        } else {
            Ui::Modern(Modern::new(palette))
        }
    }

    pub fn status(&mut self, msg: &str) {
        match self {
            Ui::Simple(u) => u.status(msg),
            Ui::Modern(u) => u.status(msg),
        }
    }

    pub fn ok(&mut self, msg: &str) {
        match self {
            Ui::Simple(u) => u.ok(msg),
            Ui::Modern(u) => u.ok(msg),
        }
    }

    pub fn err(&mut self, msg: &str) {
        match self {
            Ui::Simple(u) => u.err(msg),
            Ui::Modern(u) => u.err(msg),
        }
    }

    pub fn note(&mut self, msg: &str) {
        match self {
            Ui::Simple(u) => u.note(msg),
            Ui::Modern(u) => u.note(msg),
        }
    }

    pub fn summary(&mut self, msg: &str) {
        match self {
            Ui::Simple(u) => u.summary(msg),
            Ui::Modern(u) => u.summary(msg),
        }
    }

    pub fn reasoning(&mut self, text: &str) {
        match self {
            Ui::Simple(u) => u.reasoning(text),
            Ui::Modern(u) => u.reasoning(text),
        }
    }

    pub fn expand_reasoning(&mut self) {
        match self {
            Ui::Simple(u) => u.expand_reasoning(),
            Ui::Modern(u) => u.expand_reasoning(),
        }
    }

    pub fn reset_reasoning(&mut self) {
        match self {
            Ui::Simple(u) => u.reset_reasoning(),
            Ui::Modern(u) => u.reset_reasoning(),
        }
    }

    pub fn tool_call(&mut self, calls_summary: &str) {
        match self {
            Ui::Simple(u) => u.tool_call(calls_summary),
            Ui::Modern(u) => u.tool_call(calls_summary),
        }
    }

    pub fn tool_result(&mut self, name: &str, preview: &str, byte_len: usize) {
        match self {
            Ui::Simple(u) => u.tool_result(name, preview, byte_len),
            Ui::Modern(u) => u.tool_result(name, preview, byte_len),
        }
    }

    pub fn action_call(&mut self, name: &str, args: &str) {
        match self {
            Ui::Simple(u) => u.action_call(name, args),
            Ui::Modern(u) => u.action_call(name, args),
        }
    }

    pub fn action_result(&mut self, name: &str, preview: &str, byte_len: usize) {
        match self {
            Ui::Simple(u) => u.action_result(name, preview, byte_len),
            Ui::Modern(u) => u.action_result(name, preview, byte_len),
        }
    }

    pub fn diff_block(&mut self, diff_text: &str) {
        match self {
            Ui::Simple(u) => u.diff_block(diff_text),
            Ui::Modern(u) => u.diff_block(diff_text),
        }
    }

    pub fn text(&mut self, text: &str) {
        match self {
            Ui::Simple(u) => u.text(text),
            Ui::Modern(u) => u.text(text),
        }
    }

    pub fn end(&mut self) {
        match self {
            Ui::Simple(u) => u.end(),
            Ui::Modern(u) => u.end(),
        }
    }

    pub fn prompt(&mut self) {
        match self {
            Ui::Simple(u) => u.prompt(),
            Ui::Modern(u) => u.prompt(),
        }
    }

    pub fn clear_line(&mut self) {
        match self {
            Ui::Simple(u) => u.clear_line(),
            Ui::Modern(u) => u.clear_line(),
        }
    }

    pub fn confirm(&mut self, msg: &str) -> char {
        match self {
            Ui::Simple(u) => u.confirm(msg),
            Ui::Modern(u) => u.confirm(msg),
        }
    }
}
