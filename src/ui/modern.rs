use std::io::Write;
use std::time::Instant;

use colored::Colorize;

use super::markdown;
use super::palette::Palette;

pub struct Modern<'a> {
    md: markdown::Stream<'a>,
    pal: &'a Palette,
    has_status: bool,
    mark_printed: bool,
    preview: String,
    preview_at: Option<Instant>,
    cols: usize,
    // Collapsible thinking block: reasoning streams into a buffer while only a
    // one-line collapsed summary is shown. ctrl+t expands the full indented body.
    reasoning_buf: String,
    reasoning_expanded: bool,
    reasoning_shown: bool,
}

const PREVIEW_TICK: std::time::Duration = std::time::Duration::from_millis(30);

impl<'a> Modern<'a> {
    pub fn new(pal: &'a Palette) -> Self {
        Self {
            md: markdown::Stream::new(pal),
            pal,
            has_status: false,
            mark_printed: false,
            preview: String::new(),
            preview_at: None,
            cols: term_width(),
            reasoning_buf: String::new(),
            reasoning_expanded: false,
            reasoning_shown: false,
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

    /// Stream a reasoning chunk. While collapsed, only a one-line summary of
    /// the important info is repainted in place; when the user pressed ctrl+t
    /// the full body streams through, indented under the header.
    pub fn reasoning(&mut self, text: &str) {
        self.clear_status();
        self.clear_preview();
        if self.reasoning_buf.is_empty() && !self.reasoning_shown {
            self.reasoning_shown = true;
        }
        self.reasoning_buf.push_str(text);
        if self.reasoning_expanded {
            let body = indent_stream(text);
            print!("{}", self.pal.thought.on(&body).italic());
        } else {
            self.paint_collapsed();
        }
        flush();
    }

    /// ctrl+t: expand the collapsed thinking summary into the full body.
    pub fn expand_reasoning(&mut self) {
        if self.reasoning_buf.trim().is_empty() || self.reasoning_expanded {
            return;
        }
        self.clear_status();
        self.clear_preview();
        self.reasoning_expanded = true;
        // Erase the in-place collapsed `··· thinking` line before printing the
        // multi-line expanded body, so no stray text is left behind.
        print!("\r\x1b[2K");
        println!(
            "{}",
            self.pal.thought.on("··· thinking (ctrl+t to collapse)").italic()
        );
        for line in self.reasoning_buf.lines() {
            println!("  {}", self.pal.thought.on(line).italic());
        }
        flush();
    }

    /// Start a fresh thinking block for a new stream attempt/turn.
    pub fn reset_reasoning(&mut self) {
        self.close_reasoning();
        self.reasoning_buf.clear();
        self.reasoning_expanded = false;
        self.reasoning_shown = false;
    }

    /// Finalize the thinking block so normal output starts on a fresh line.
    fn close_reasoning(&mut self) {
        if !self.reasoning_shown {
            return;
        }
        println!();
        self.reasoning_shown = false;
        flush();
    }

    fn paint_collapsed(&mut self) {
        let summary = flatten(&self.reasoning_buf, self.cols.saturating_sub(16));
        let line = format!("··· thinking {summary}");
        if self.reasoning_shown {
            print!("\r\x1b[2K{}", self.pal.thought.on(&line));
        } else {
            print!("{}", self.pal.thought.on(&line));
            self.reasoning_shown = true;
        }
        flush();
    }

    pub fn tool_call(&mut self, calls_summary: &str) {
        self.close_reasoning();
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
        self.close_reasoning();
        self.clear_status();
        self.clear_preview();
        let arrow = self.pal.tool_result.on("▾");
        let name_s = self.pal.tool_result.on(name);
        let preview_s = self.pal.tool_result.on(preview);
        if byte_len > 500 {
            println!("  {} {} ({} B): {} …", arrow, name_s, byte_len, preview_s);
        } else {
            println!("  {} {}: {}", arrow, name_s, preview_s);
        }
        flush();
    }

    pub fn action_call(&mut self, name: &str, args: &str) {
        self.close_reasoning();
        self.clear_status();
        self.clear_preview();
        self.end_line();
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
        self.clear_status();
        self.clear_preview();
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
        self.clear_status();
        self.clear_preview();
        // Related info (the diff of what an action changed) nests indented
        // under the action's result line.
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
        self.clear_status();
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
        self.close_reasoning();
        self.clear_status();
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

    pub fn clear_line(&mut self) {
        self.clear_status();
        self.clear_preview();
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

/// Condense a raw thinking buffer into a single-line summary of the important
/// info: the concluding line (or tail) with noise trimmed, truncated to `max`.
fn flatten(raw: &str, max: usize) -> String {
    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    // The last non-empty line is usually the conclusion — the important part.
    let mut pick = lines[lines.len() - 1].to_string();
    if pick.is_empty() {
        pick = lines.join(" ");
    }
    // Drop trailing sentence fragments like "I'll", "So".
    if pick.len() > max {
        let mut cut = max;
        while cut > 0 && !pick.is_char_boundary(cut) {
            cut -= 1;
        }
        let mut s: String = pick[..cut].chars().collect();
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

/// Indent continuation lines of a streamed chunk by two spaces so the expanded
/// thinking body stays nested under its header.
fn indent_stream(text: &str) -> String {
    if text.contains('\n') {
        text.replace('\n', "\n  ")
    } else {
        text.to_string()
    }
}