use std::io::Write;

use colored::Colorize;

use super::palette::Palette;

pub struct Simple<'a> {
    pal: &'a Palette,
    md: super::markdown::Stream<'a>,
    // Collapsible thinking block: reasoning is buffered, only a one-line
    // summary is printed; ctrl+t reprints the full indented body.
    reasoning_buf: String,
    reasoning_expanded: bool,
    reasoning_shown: bool,
}

impl<'a> Simple<'a> {
    pub fn new(pal: &'a Palette) -> Self {
        Self {
            pal,
            reasoning_buf: String::new(),
            reasoning_expanded: false,
            reasoning_shown: false,
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
        self.reasoning_buf.push_str(text);
        if self.reasoning_expanded {
            // Full body streams, indented to stay nested under the header.
            for line in text.lines() {
                println!("  {}", self.pal.thought.on(line).italic());
            }
            self.reasoning_shown = true;
        } else if !self.reasoning_shown {
            // One-line summary of the important info only.
            println!(
                "{} {}",
                self.pal.thought.on("··· thinking"),
                self.pal.thought.on(&flatten(&self.reasoning_buf))
            );
            self.reasoning_shown = true;
        }
        flush();
    }

    /// ctrl+t: expand the thinking summary into the full indented body.
    pub fn expand_reasoning(&mut self) {
        if self.reasoning_buf.trim().is_empty() || self.reasoning_expanded {
            return;
        }
        self.reasoning_expanded = true;
        self.reasoning_shown = false;
        println!(
            "{}",
            self.pal.thought.on("··· thinking (ctrl+t to collapse)").italic()
        );
        for line in self.reasoning_buf.lines() {
            println!("  {}", self.pal.thought.on(line).italic());
        }
        self.reasoning_shown = true;
        flush();
    }

    /// Start a fresh thinking block for a new stream attempt/turn.
    pub fn reset_reasoning(&mut self) {
        self.close_reasoning();
        self.reasoning_buf.clear();
        self.reasoning_expanded = false;
        self.reasoning_shown = false;
    }

    pub fn tool_call(&mut self, calls_summary: &str) {
        self.close_reasoning();
        println!("{} {}", self.pal.tool_call.on("▸"), calls_summary);
        flush();
    }

    pub fn tool_result(&mut self, name: &str, preview: &str, byte_len: usize) {
        self.close_reasoning();
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
        self.close_reasoning();
        println!(
            "{} {} {}",
            self.pal.tool_call.on("▸"),
            self.pal.action.on_bold(name),
            self.pal.action.on(args)
        );
        flush();
    }

    pub fn action_result(&mut self, name: &str, preview: &str, byte_len: usize) {
        self.close_reasoning();
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
        self.close_reasoning();
        // The diff of an action's change nests indented under the result line.
        for line in diff_text.lines() {
            if let Some(rest) = line.strip_prefix('+') {
                println!("  {}", self.pal.add.on(&format!("+{rest}")));
            } else if let Some(rest) = line.strip_prefix('-') {
                println!("  {}", self.pal.del.on(&format!("-{rest}")));
            } else if line.starts_with("@@") {
                println!("  {}", self.pal.note.on(line));
            } else {
                println!("  {}", self.pal.code.on(line));
            }
        }
        flush();
    }

    pub fn text(&mut self, text: &str) {
        self.close_reasoning();
        let out = self.md.feed(text);
        if !out.is_empty() {
            print!("{out}");
            flush();
        }
    }

    pub fn end(&mut self) {
        self.close_reasoning();
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

    fn close_reasoning(&mut self) {
        if !self.reasoning_shown {
            return;
        }
        self.reasoning_shown = false;
        flush();
    }
}

fn flush() {
    std::io::stdout().flush().ok();
}

/// Condense a raw thinking buffer into a single-line summary of the important
/// info: the concluding (last) non-empty line, with sentence fragments trimmed.
fn flatten(raw: &str) -> String {
    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let pick = lines[lines.len() - 1].to_string();
    if pick.len() > 160 {
        let cut = pick
            .char_indices()
            .take_while(|(i, _)| *i < 160)
            .map(|(i, c)| i + c.len_utf8())
            .last()
            .unwrap_or(160);
        let mut s: String = pick[..cut].to_string();
        if let Some(idx) = s.rfind(['.', ':', '!']) {
            let keep = idx + 1;
            s.truncate(keep);
        }
        s.push_str(" …");
        s
    } else {
        pick
    }
}