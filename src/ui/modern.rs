use std::io::Write;
use std::time::Instant;

use colored::Colorize;

use super::markdown;
use super::palette::Palette;

pub struct Modern<'a> {
    md: markdown::Stream<'a>,
    pal: &'a Palette,
    has_status: bool,
    reason: String,
    reason_dirty: bool,
    mark_printed: bool,
    preview: String,
    preview_at: Option<Instant>,
    cols: usize,
}

const PREVIEW_TICK: std::time::Duration = std::time::Duration::from_millis(30);

impl<'a> Modern<'a> {
    pub fn new(pal: &'a Palette) -> Self {
        Self {
            md: markdown::Stream::new(pal),
            pal,
            has_status: false,
            reason: String::new(),
            reason_dirty: false,
            mark_printed: false,
            preview: String::new(),
            preview_at: None,
            cols: term_width(),
        }
    }

    pub fn status(&mut self, msg: &str) {
        self.clear_status();
        print!("\r\x1b[2K{}", self.pal.status.on(&format!("· {msg}")));
        self.has_status = true;
        flush();
    }

    pub fn ok(&mut self, msg: &str) {
        self.clear_status();
        println!("{} {msg}", self.pal.ok.on("✓"));
        flush();
    }

    pub fn err(&mut self, msg: &str) {
        self.clear_status();
        println!("{} {msg}", self.pal.err.on("✗"));
        flush();
    }

    pub fn note(&mut self, msg: &str) {
        self.clear_status();
        println!("{}", self.pal.note.on(msg));
        flush();
    }

    pub fn summary(&mut self, msg: &str) {
        self.clear_status();
        println!("{}", self.pal.summary.on(msg));
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

    pub fn tool_call(&mut self, calls_summary: &str) {
        self.clear_status();
        self.clear_preview();
        self.end_line();
        println!(
            "{} {}",
            self.pal.tool_call.on("▸"),
            self.pal.tool_result.on(calls_summary)
        );
        flush();
    }

    pub fn tool_result(&mut self, name: &str, preview: &str, byte_len: usize) {
        self.clear_status();
        self.clear_preview();
        let arrow = self.pal.tool_result.on("▾");
        let name_s = self.pal.tool_result.on(name);
        let preview_s = self.pal.tool_result.on(preview);
        if byte_len > 500 {
            println!("{} {} ({} B): {} …", arrow, name_s, byte_len, preview_s);
        } else {
            println!("{} {}: {}", arrow, name_s, preview_s);
        }
        flush();
    }

    pub fn text(&mut self, text: &str) {
        self.clear_status();
        if self.reason_dirty {
            self.flush_reason();
            self.mark_printed = false;
        }
        let out = self.md.feed(text);
        if !out.is_empty() {
            self.clear_preview();
            if !self.mark_printed {
                self.mark_printed = true;
            }
            print!("{out}");
            flush();
        }
        if self.md.has_pending() {
            self.paint_preview();
        } else {
            self.clear_preview();
        }
    }

    pub fn end(&mut self) {
        self.clear_status();
        if self.reason_dirty {
            self.flush_reason();
        }
        self.clear_preview();
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

    pub fn confirm(&mut self, msg: &str) -> char {
        self.clear_status();
        print!("\r\x1b[2K· {msg} [y/n/a] ");
        flush();
        let ch = crate::term::read_key();
        let label = match ch {
            'y' | 'Y' => "yes",
            'n' | 'N' => "no",
            'a' | 'A' => "always",
            '?' => "?",
            _ => "?",
        };
        print!("\r\x1b[2K· {msg} [{label}]");
        flush();
        ch.to_ascii_lowercase()
    }

    fn end_line(&mut self) {
        if self.mark_printed {
            println!();
            self.mark_printed = false;
            flush();
        }
    }

    fn paint_preview(&mut self) {
        let pending = self.md.pending_raw().to_string();
        if pending.trim().is_empty() {
            self.clear_preview();
            return;
        }
        let due = match self.preview_at {
            Some(t) => t.elapsed() >= PREVIEW_TICK,
            None => true,
        };
        let grew = pending.len() >= self.preview.len() && !self.preview.is_empty();
        if !due && !grew {
            return;
        }
        let rendered = markdown::preview(&trunc_to_cols(&pending, self.cols - 1), self.pal);
        self.clear_preview();
        print!("{}", self.pal.preview.on(&rendered));
        self.preview = pending;
        self.preview_at = Some(Instant::now());
        flush();
    }

    fn clear_preview(&mut self) {
        if !self.preview.is_empty() || self.preview_at.is_some() {
            print!("\r\x1b[2K");
            self.preview.clear();
            self.preview_at = None;
            flush();
        }
    }

    fn flush_reason(&mut self) {
        if !self.reason.is_empty() {
            let label = self.pal.thought.on("Thought:").italic();
            let body = self.pal.thought.on(&self.reason).italic();
            println!("{} {}", label, body);
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

fn term_width() -> usize {
    let mut ws = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ok = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) };
    if ok == 0 && ws.ws_col > 0 {
        ws.ws_col as usize
    } else {
        80
    }
}

fn trunc_to_cols(s: &str, cols: usize) -> String {
    let mut used = 0usize;
    let mut out = String::new();
    let mut truncated = false;
    for c in s.chars() {
        let w = if c.is_ascii() { 1 } else { 2 };
        if used + w > cols {
            truncated = true;
            break;
        }
        out.push(c);
        used += w;
    }
    if truncated && used < cols {
        out.push('…');
    }
    out
}
