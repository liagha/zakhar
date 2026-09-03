use std::io::Write;

use colored::Colorize;

pub struct Simple;

impl Simple {
    pub fn new() -> Self {
        Self
    }

    pub fn status(&mut self, msg: &str) {
        if !msg.is_empty() {
            println!("· {msg}");
        }
        flush();
    }

    pub fn ok(&mut self, msg: &str) {
        println!("✓ {msg}");
        flush();
    }

    pub fn err(&mut self, msg: &str) {
        println!("✗ {msg}");
        flush();
    }

    pub fn note(&mut self, msg: &str) {
        if !msg.is_empty() {
            println!("{msg}");
        }
        flush();
    }

    pub fn reasoning(&mut self, text: &str) {
        print!("{}", text.italic().dimmed());
        flush();
    }

    pub fn text(&mut self, text: &str) {
        print!("{text}");
        flush();
    }

    pub fn end(&mut self) {
        println!();
        flush();
    }

    pub fn prompt(&mut self) {
        print!("> ");
        flush();
    }
}

fn flush() {
    std::io::stdout().flush().ok();
}
