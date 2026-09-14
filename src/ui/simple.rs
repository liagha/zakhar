use std::io::Write;

use colored::Colorize;

use super::palette::Palette;

pub struct Simple<'a> {
    pal: &'a Palette,
    reason_printed: bool,
    md: super::markdown::Stream<'a>,
}

impl<'a> Simple<'a> {
    pub fn new(pal: &'a Palette) -> Self {
        Self {
            pal,
            reason_printed: false,
            md: super::markdown::Stream::new(pal),
        }
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
        if !self.reason_printed {
            self.reason_printed = true;
            print!("{}", self.pal.thought.on("Thought: ").italic());
        }
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
            println!("  {} {} ({} B): {} …", arrow, name_s, byte_len, preview);
        } else {
            println!("  {} {}: {}", arrow, name_s, preview);
        }
        flush();
    }

    pub fn action_call(&mut self, name: &str, args: &str) {
        println!(
            "{} {} {}",
            self.pal.action.on("⚡"),
            self.pal.action.on_bold(name),
            self.pal.action.on(args)
        );
        flush();
    }

    pub fn action_result(&mut self, name: &str, preview: &str, byte_len: usize) {
        println!(
            "  {} {}: {}",
            self.pal.action.on("↳"),
            self.pal.action.on_bold(name),
            self.pal.action.on(preview)
        );
        let _ = byte_len;
        flush();
    }

    pub fn diff_block(&mut self, diff_text: &str) {
        for line in diff_text.lines() {
            if let Some(rest) = line.strip_prefix('+') {
                println!("{}", self.pal.add.on(&format!("+{rest}")));
            } else if let Some(rest) = line.strip_prefix('-') {
                println!("{}", self.pal.del.on(&format!("-{rest}")));
            } else if line.starts_with("@@") {
                println!("{}", self.pal.note.on(line));
            } else {
                println!("{}", self.pal.code.on(line));
            }
        }
        flush();
    }

    pub fn text(&mut self, text: &str) {
        if self.reason_printed {
            self.reason_printed = false;
            println!();
        }
        let out = self.md.feed(text);
        if !out.is_empty() {
            print!("{out}");
            flush();
        }
    }

    pub fn end(&mut self) {
        self.reason_printed = false;
        let tail = self.md.finish();
        if !tail.is_empty() {
            print!("{tail}");
        }
        println!();
        flush();
    }

    pub fn prompt(&mut self) {
        print!("> ");
        flush();
    }

    pub fn clear_line(&mut self) {}

    pub fn confirm(&mut self, msg: &str) -> char {
        print!("· {msg} [y/n/a] ");
        flush();
        let mut ch = crate::term::read_key();
        if ch == '\x1b' {
            ch = 'n';
        }
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

