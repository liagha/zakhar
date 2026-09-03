use std::io::Write;

use colored::Colorize;

use super::markdown;

pub struct Modern {
    md: markdown::Stream,
    has_status: bool,
    reason: String,
    reason_dirty: bool,
    mark_printed: bool,
}

impl Modern {
    pub fn new() -> Self {
        Self {
            md: markdown::Stream::new(),
            has_status: false,
            reason: String::new(),
            reason_dirty: false,
            mark_printed: false,
        }
    }

    pub fn status(&mut self, msg: &str) {
        self.clear_status();
        print!("\r\x1b[2K{}", format!("· {msg}").dimmed());
        self.has_status = true;
        flush();
    }

    pub fn ok(&mut self, msg: &str) {
        self.clear_status();
        println!("{} {msg}", "✓".dimmed());
        flush();
    }

    pub fn err(&mut self, msg: &str) {
        self.clear_status();
        println!("{} {msg}", "✗".dimmed().red());
        flush();
    }

    pub fn note(&mut self, msg: &str) {
        self.clear_status();
        println!("{}", msg.dimmed());
        flush();
    }

    pub fn reasoning(&mut self, text: &str) {
        self.mark_printed = false;
        for c in text.chars() {
            if c == '\n' {
                self.flush_reason();
            } else {
                self.reason.push(c);
                self.reason_dirty = true;
            }
        }
    }

    pub fn text(&mut self, text: &str) {
        self.clear_status();
        if self.reason_dirty {
            self.flush_reason();
            self.mark_printed = false;
        }
        let out = self.md.feed(text);
        if !out.is_empty() {
            if !self.mark_printed {
                self.mark_printed = true;
            }
            print!("{out}");
            flush();
        }
    }

    pub fn end(&mut self) {
        self.clear_status();
        if self.reason_dirty {
            self.flush_reason();
        }
        let tail = self.md.finish();
        if !tail.is_empty() {
            print!("{tail}");
        }
        println!();
        flush();
    }

    pub fn prompt(&mut self) {
        self.clear_status();
        print!("> ");
        flush();
    }

    fn flush_reason(&mut self) {
        if !self.reason.is_empty() {
            println!("{}", self.reason.dimmed().italic());
        }
        self.reason.clear();
        self.reason_dirty = false;
        flush();
    }

    fn clear_status(&mut self) {
        if self.has_status {
            print!("\r\x1b[2K");
            self.has_status = false;
            flush();
        }
    }
}

fn flush() {
    std::io::stdout().flush().ok();
}
