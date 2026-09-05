use std::io::Write;

use colored::Colorize;

use super::palette::Palette;

pub struct Simple<'a> {
    pal: &'a Palette,
}

impl<'a> Simple<'a> {
    pub fn new(pal: &'a Palette) -> Self {
        Self { pal }
    }

    pub fn status(&mut self, msg: &str) {
        if !msg.is_empty() {
            println!("· {msg}");
        }
        flush();
    }

    pub fn ok(&mut self, msg: &str) {
        println!("{} {msg}", self.pal.ok.on("✓"));
        flush();
    }

    pub fn err(&mut self, msg: &str) {
        println!("{} {msg}", self.pal.err.on("✗"));
        flush();
    }

    pub fn note(&mut self, msg: &str) {
        if !msg.is_empty() {
            println!("{msg}");
        }
        flush();
    }

    pub fn summary(&mut self, msg: &str) {
        println!("{msg}");
        flush();
    }

    pub fn reasoning(&mut self, text: &str) {
        print!("{}", self.pal.thought.on(text).italic());
        flush();
    }

    pub fn tool_call(&mut self, calls_summary: &str) {
        println!("{} {}", self.pal.tool_call.on("▸"), calls_summary);
        flush();
    }

    pub fn tool_result(&mut self, name: &str, preview: &str, byte_len: usize) {
        let arrow = self.pal.tool_result.on("▾");
        let name_s = self.pal.tool_result.on(name);
        if byte_len > 500 {
            println!("{} {} ({} B): {} …", arrow, name_s, byte_len, preview);
        } else {
            println!("{} {}: {}", arrow, name_s, preview);
        }
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

    pub fn confirm(&mut self, msg: &str) -> char {
        print!("· {msg} [y/n/a] ");
        flush();
        let ch = crate::term::read_key();
        let label = match ch {
            'y' | 'Y' => "yes",
            'n' | 'N' => "no",
            'a' | 'A' => "always",
            '?' => "?",
            _ => "?",
        };
        print!("\r\x1b[2K· {msg} [{label}]\n");
        flush();
        ch.to_ascii_lowercase()
    }
}

fn flush() {
    std::io::stdout().flush().ok();
}
