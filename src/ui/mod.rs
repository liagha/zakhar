pub mod markdown;
pub mod modern;
pub mod simple;

use modern::Modern;
use simple::Simple;

pub enum Ui {
    Simple(Simple),
    Modern(Modern),
}

impl Ui {
    pub fn new(simple: bool) -> Self {
        if simple {
            Ui::Simple(Simple::new())
        } else {
            Ui::Modern(Modern::new())
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

    pub fn reasoning(&mut self, text: &str) {
        match self {
            Ui::Simple(u) => u.reasoning(text),
            Ui::Modern(u) => u.reasoning(text),
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
}
