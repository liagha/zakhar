use colored::Colorize;

use super::palette::Palette;

pub struct Stream<'a> {
    buf: String,
    fence: Option<String>,
    table: Vec<String>,
    pal: &'a Palette,
}

impl<'a> Stream<'a> {
    pub fn new(pal: &'a Palette) -> Self {
        Self {
            buf: String::new(),
            fence: None,
            table: Vec::new(),
            pal,
        }
    }

    pub fn feed(&mut self, text: &str) -> String {
        self.buf.push_str(text);
        let mut lines = Vec::new();
        let mut start = 0;
        for (i, b) in self.buf.bytes().enumerate() {
            if b == b'\n' {
                lines.push(self.buf[start..i].to_string());
                start = i + 1;
            }
        }
        if start > 0 {
            self.buf.drain(0..start);
        }
        let mut out = String::new();
        for line in lines {
            self.line(&line, &mut out);
        }
        out
    }

    pub fn pending_raw(&self) -> &str {
        &self.buf
    }

    pub fn has_pending(&self) -> bool {
        !self.buf.is_empty()
    }

    pub fn finish(&mut self) -> String {
        let mut out = String::new();
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            self.line(&line, &mut out);
        }
        if self.fence.is_some() {
            self.fence = None;
            out.push_str(&format!("{}\n", color_close(self.pal)));
        }
        if !self.table.is_empty() {
            out.push_str(&render_table(&self.table, self.pal));
            self.table.clear();
        }
        out
    }

    fn line(&mut self, line: &str, out: &mut String) {
        if self.fence.is_some() {
            if is_fence(line) {
                self.fence = None;
                out.push_str(&format!("{}\n", color_close(self.pal)));
            } else if is_table(line) {
                self.table.push(line.to_string());
            } else {
                if !self.table.is_empty() {
                    out.push_str(&render_table(&self.table, self.pal));
                    self.table.clear();
                }
                out.push_str(&format!("{}\n", code(line, self.pal)));
            }
            return;
        }

        if is_fence(line) {
            if !self.table.is_empty() {
                out.push_str(&render_table(&self.table, self.pal));
                self.table.clear();
            }
            let lang = fence_lang(line).to_string();
            self.fence = Some(lang.clone());
            out.push_str(&format!("{}\n", color_open(&lang, self.pal)));
            return;
        }

        if is_table(line) {
            self.table.push(line.to_string());
            return;
        }

        if !self.table.is_empty() {
            if line.trim().is_empty() {
                out.push_str(&render_table(&self.table, self.pal));
                self.table.clear();
            } else {
                self.table.push(line.to_string());
                return;
            }
        }

        if line.trim().is_empty() {
            out.push('\n');
            return;
        }

        if let Some(mark) = heading(line) {
            out.push_str(&heading_text(mark, inline(line[mark..].trim(), self.pal), self.pal));
            return;
        }

        if quote(line) {
            out.push_str(&quote_text(inline(line.trim_start().trim_start_matches('>').trim(), self.pal), self.pal));
            return;
        }

        if let Some(off) = item(line) {
            let (glyph, rest) = if line.split('.').next().unwrap_or("").parse::<usize>().is_ok() && off >= 3 {
                (line[..off - 2].trim().to_string(), &line[off..])
            } else {
                ("•".to_string(), &line[off..])
            };
            out.push_str(&item_text(glyph, inline(rest.trim(), self.pal), self.pal));
            return;
        }

        if rule(line) {
            out.push_str(&color_rule(self.pal));
            return;
        }

        out.push_str(&format!("{}\n", inline(line, self.pal)));
    }
}

fn is_fence(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("```") || t.starts_with("~~~")
}

fn fence_lang(line: &str) -> &str {
    line.trim().trim_start_matches('`').trim_start_matches('~').trim()
}

fn is_table(line: &str) -> bool {
    line.trim().starts_with('|') && line.contains('|')
}

fn heading(line: &str) -> Option<usize> {
    let mut n = 0;
    for b in line.bytes() {
        if b == b'#' {
            n += 1;
        } else {
            break;
        }
    }
    if n > 0 && n <= 6 && line.as_bytes().get(n) == Some(&b' ') {
        Some(n)
    } else {
        None
    }
}

fn quote(line: &str) -> bool {
    line.trim_start().starts_with('>')
}

fn item(line: &str) -> Option<usize> {
    let t = line;
    let tt = t.trim_start();
    let lead = t.len() - tt.len();
    if tt.starts_with("- ")
        || tt.starts_with("* ")
        || tt.starts_with("+ ")
    {
        Some(lead + 2)
    } else if tt.len() >= 3
        && tt.as_bytes()[0].is_ascii_digit()
        && tt[1..].starts_with(". ")
    {
        let num = tt.split('.').next().unwrap_or("").len();
        Some(lead + num + 2)
    } else {
        None
    }
}

fn rule(line: &str) -> bool {
    let t = line.trim();
    t == "---" || t == "***" || t == "___"
}

fn inline(input: &str, pal: &Palette) -> String {
    let mut out = String::new();
    let mut rest = input;
    while !rest.is_empty() {
        if let Some((pre, content, len)) = code_span(rest)
            && len <= rest.len()
        {
            out.push_str(&inline(&pre, pal));
            out.push_str(&code_text(content, pal));
            rest = &rest[len..];
            continue;
        }
        if let Some((pre, text, url, len)) = link_span(rest)
            && len <= rest.len()
        {
            out.push_str(&inline(&pre, pal));
            out.push_str(&link_text(text, url, pal));
            rest = &rest[len..];
            continue;
        }
        if let Some((pre, bold, len)) = bold_span(rest)
            && len <= rest.len()
        {
            out.push_str(&inline(&pre, pal));
            out.push_str(&bold_text(bold));
            rest = &rest[len..];
            continue;
        }
        if let Some((pre, ital, len)) = italic_span(rest)
            && len <= rest.len()
        {
            out.push_str(&inline(&pre, pal));
            out.push_str(&italic_text(ital));
            rest = &rest[len..];
            continue;
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

fn code_span(s: &str) -> Option<(String, &str, usize)> {
    let open = s.find('`')?;
    let after = &s[open + 1..];
    let close = after.find('`')?;
    let content = &after[..close];
    if content.is_empty() {
        None
    } else {
        Some((s[..open].to_string(), content, open + 1 + close + 1))
    }
}

fn link_span(s: &str) -> Option<(String, &str, &str, usize)> {
    let open = s.find('[')?;
    let after = &s[open + 1..];
    let close = after.find(']')?;
    let text = &after[..close];
    let tail = &after[close + 1..];
    if let Some(rest) = tail.strip_prefix('(')
        && let Some(end) = rest.find(')')
    {
        let url = &rest[..end];
        let len = open + close + end + 4;
        Some((s[..open].to_string(), text, url, len))
    } else {
        None
    }
}

fn bold_span(s: &str) -> Option<(String, &str, usize)> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    for i in 0..chars.len() {
        let (idx, c) = chars[i];
        if c == '*' && chars.get(i + 1).is_some_and(|(_, n)| *n == '*') {
            let after = &s[idx + 2..];
            if let Some(close) = after.find("**") {
                let content = &after[..close];
                let pre = s[..idx].to_string();
                return Some((pre, content, idx + close + 4));
            }
        }
    }
    None
}

fn italic_span(s: &str) -> Option<(String, &str, usize)> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    for i in 0..chars.len() {
        let (idx, c) = chars[i];
        if c == '*' || c == '_' {
            if c == '*' && chars.get(i + 1).is_some_and(|(_, n)| *n == '*') {
                continue;
            }
            let after = &s[idx + c.len_utf8()..];
            if let Some(close) = after.find(c) {
                let content = &after[..close];
                let pre = s[..idx].to_string();
                return Some((pre, content, idx + c.len_utf8() + close + c.len_utf8()));
            }
        }
    }
    None
}

fn code_text(content: &str, pal: &Palette) -> String {
    pal.code.on(content)
}

fn link_text(text: &str, url: &str, pal: &Palette) -> String {
    format!(
        "{}{}",
        pal.link.on_underline(text),
        pal.url.on(&format!(" ({url})"))
    )
}

fn bold_text(s: &str) -> String {
    s.bold().to_string()
}

fn italic_text(s: &str) -> String {
    s.italic().to_string()
}

fn code(line: &str, pal: &Palette) -> String {
    format!("  {}", pal.code.on(line))
}

fn color_open(lang: &str, pal: &Palette) -> String {
    if lang.is_empty() {
        pal.code.on("```")
    } else {
        format!("``` {}", pal.code.on(lang))
    }
}

fn color_close(pal: &Palette) -> String {
    pal.code.on("```")
}

fn heading_text(mark: usize, content: String, pal: &Palette) -> String {
    let paint = pal.headings.get(mark.saturating_sub(1)).unwrap_or(&pal.headings[3]);
    format!("{}\n", paint.on_bold(&content))
}

fn quote_text(content: String, pal: &Palette) -> String {
    format!("{} {}", pal.quote.on("│"), pal.quote.on(&content))
}

fn item_text(mark: String, rest: String, pal: &Palette) -> String {
    format!("{} {}\n", pal.list.on(&mark), rest)
}

fn color_rule(pal: &Palette) -> String {
    format!("{}\n", pal.rule.on(&"───".repeat(6)))
}

fn render_table(rows: &[String], pal: &Palette) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cells: Vec<Vec<&str>> = rows
        .iter()
        .map(|r| {
            r.trim()
                .trim_start_matches('|')
                .trim_end_matches('|')
                .split('|')
                .map(|c| c.trim())
                .collect()
        })
        .collect();
    let cols = cells.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    let mut widths = vec![0usize; cols];
    for row in &cells {
        for (i, c) in row.iter().enumerate() {
            let w = plain_len(c);
            if w > widths[i] {
                widths[i] = w;
            }
        }
    }
    let mut out = String::new();
    out.push_str(&sep(&widths));
    for (ri, row) in cells.iter().enumerate() {
        out.push('|');
        for (ci, w) in widths.iter().enumerate() {
            let val = row.get(ci).copied().unwrap_or("");
            let pad = w.saturating_sub(plain_len(val));
            let styled = if ri == 0 {
                inline(val, pal).bold().to_string()
            } else {
                inline(val, pal)
            };
            out.push_str(&format!(" {}{} |", styled, " ".repeat(pad)));
        }
        out.push('\n');
        if ri == 0 {
            out.push_str(&sep(&widths));
        }
    }
    out.push_str(&sep(&widths));
    out
}

fn sep(widths: &[usize]) -> String {
    let mut s = String::new();
    s.push('+');
    for w in widths {
        s.push_str(&"-".repeat(w + 2));
        s.push('+');
    }
    s.push('\n');
    s
}

fn plain_len(s: &str) -> usize {
    let mut n = 0;
    let mut rest = s;
    while !rest.is_empty() {
        if rest.starts_with("\x1b[")
            && let Some(end) = rest.find('m')
        {
            rest = &rest[end + 1..];
            continue;
        }
        let c = rest.chars().next().unwrap();
        n += 1;
        rest = &rest[c.len_utf8()..];
    }
    n
}

pub fn preview(input: &str, pal: &Palette) -> String {
    inline(input, pal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(input: &str) -> String {
        let pal = Palette::default();
        let mut s = Stream::new(&pal);
        let mut out = s.feed(input);
        out.push_str(&s.finish());
        out
    }

    fn plain(input: &str) -> String {
        let mut out = String::new();
        let mut rest = input;
        while let Some(i) = rest.find("\u{1b}")
            && let Some(end) = rest[i..].find('m')
        {
            out.push_str(&rest[..i]);
            rest = &rest[i + end + 1..];
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn plain_text_passes_through() {
        let out = render("hello world");
        assert!(out.contains("hello world"));
    }

    #[test]
    fn heading_gets_color() {
        let out = render("# Title\n");
        assert!(!out.contains('#'));
        assert!(out.contains("Title"));
    }

    #[test]
    fn bold_is_styled() {
        let out = render("some **bold** text");
        assert!(!out.contains("**"));
        assert!(out.contains("bold"));
    }

    #[test]
    fn bold_consumes_all_delimiters() {
        for s in [
            "**Entry point** (src/main.rs) - Clap",
            "**Config** (src/config.rs) - defaults",
            "**Provider layer** (src/provider/) - A Provider trait",
            "Plain **bold** and `code` together",
            "`code` then **bold**",
        ] {
            let out = render(s);
            assert!(!out.contains('*'), "stray asterisk in {s:?} -> {out:?}");
        }
    }

    #[test]
    fn code_span_preserves_prefix() {
        let out = render("Module: `config.rs` and `types.rs`");
        assert!(out.contains("Module:"));
        assert!(out.contains("config.rs"));
        assert!(out.contains("types.rs"));
        let plain = plain(&out);
        let a = plain.find("config.rs").unwrap();
        let b = plain.find("types.rs").unwrap();
        assert!(a < b);
    }

    #[test]
    fn code_span_is_dimmed() {
        let out = render("run `cargo build` now");
        assert!(!out.contains('`'));
        assert!(out.contains("cargo build"));
    }

    #[test]
    fn fenced_code_kept() {
        let out = render("```rust\nlet x = 1;\n```\n");
        assert!(out.contains("let x = 1;"));
    }

    #[test]
    fn list_items() {
        let out = render("- one\n- two\n");
        assert!(out.contains("one"));
        assert!(out.contains("two"));
    }

    #[test]
    fn list_items_each_on_own_line() {
        let out = render("- one\n- two\n- three\n");
        let one = out.find("one").unwrap();
        let two = out.find("two").unwrap();
        let three = out.find("three").unwrap();
        assert!(one < two && two < three);
        assert!(out[one..two].contains('\n'));
    }

    #[test]
    fn table_rendered() {
        let out = render("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(out.contains('+'));
        assert!(out.contains('a'));
        assert!(out.contains('1'));
    }

    #[test]
    fn quote_styled() {
        let out = render("> a note\n");
        assert!(!out.contains('>'));
        assert!(out.contains("a note"));
    }

    #[test]
    fn split_across_feed() {
        let pal = Palette::default();
        let mut s = Stream::new(&pal);
        let o1 = s.feed("# Ti");
        let o2 = s.feed("tle\n");
        assert!(o1.is_empty());
        assert!(o2.contains("Title") || o2.contains("Ti"));
    }

    #[test]
    fn multiple_bold_does_not_overflow() {
        let out = render("**x** and **y**\n");
        assert!(out.contains("x"));
        assert!(out.contains("y"));
        assert!(!out.contains("**"));
    }

    #[test]
    fn inline_never_panics_on_partial() {
        // a lone opening marker at end of a streamed chunk must not panic
        let out = render("some **partial\n");
        assert!(out.contains("some"));
    }

    #[test]
    fn link_consumed_including_paren() {
        let out = render("a [x](u) b\n");
        assert!(out.contains("x"));
        assert!(out.contains("b"));
        // url is rendered once via formatting, not left as raw trailing text
        assert_eq!(out.matches('x').count(), 1);
    }

    #[test]
    fn pending_holds_incomplete_line() {
        let pal = Palette::default();
        let mut s = Stream::new(&pal);
        let out = s.feed("# Ti");
        assert!(out.is_empty());
        assert_eq!(s.pending_raw(), "# Ti");
        let _ = s.feed("tle\n");
        assert!(s.pending_raw().is_empty());
    }

    #[test]
    fn preview_inlines_bold_and_code() {
        let pal = Palette::default();
        let p = preview("see **bold** and `code`", &pal);
        assert!(!p.contains("**"));
        assert!(!p.contains('`'));
        assert!(plain(&p).contains("bold"));
        assert!(plain(&p).contains("code"));
    }
}
