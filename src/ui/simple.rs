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
        println!("{} {msg}", "✓".green());
        flush();
    }

    pub fn err(&mut self, msg: &str) {
        println!("{} {msg}", "✗".red());
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
        print!("{}", text.italic().dimmed());
        flush();
    }

    pub fn tool_call(&mut self, calls_summary: &str) {
        println!("{} {}", "▸".cyan(), calls_summary);
        flush();
    }

    pub fn tool_result(&mut self, name: &str, preview: &str, byte_len: usize) {
        if byte_len > 500 {
            println!("{} {} ({} B): {} …", "▾".bright_black(), name.bright_black(), byte_len, preview);
        } else {
            println!("{} {}: {}", "▾".bright_black(), name.bright_black(), preview);
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
