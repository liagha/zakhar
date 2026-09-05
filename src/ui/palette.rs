use std::collections::HashMap;

use colored::{Color as C, Colorize};

#[derive(Clone, Copy)]
pub enum Spec {
    Plain,
    Dim,
    Color(C),
}

impl Spec {
    fn parse(value: &str) -> Spec {
        if value.eq_ignore_ascii_case("plain") || value.eq_ignore_ascii_case("none") {
            return Spec::Plain;
        }
        if value.eq_ignore_ascii_case("dim") {
            return Spec::Dim;
        }
        if let Some(hex) = value.strip_prefix('#')
            && let Some(color) = parse_hex(hex)
        {
            return Spec::Color(color);
        }
        Spec::Color(value.replace('_', " ").parse().unwrap_or(C::White))
    }
}

fn parse_hex(hex: &str) -> Option<C> {
    let clean = hex.trim();
    if clean.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
    let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
    let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
    Some(C::TrueColor { r, g, b })
}

#[derive(Clone, Copy)]
pub struct Paint {
    spec: Spec,
}

impl Paint {
    fn of(spec: Spec) -> Self {
        Self { spec }
    }

    pub fn on(&self, text: &str) -> String {
        match self.spec {
            Spec::Plain => text.to_string(),
            Spec::Dim => text.dimmed().to_string(),
            Spec::Color(c) => text.color(c).to_string(),
        }
    }

    pub fn on_bold(&self, text: &str) -> String {
        match self.spec {
            Spec::Plain => text.bold().to_string(),
            Spec::Dim => text.bold().dimmed().to_string(),
            Spec::Color(c) => text.bold().color(c).to_string(),
        }
    }

    pub fn on_underline(&self, text: &str) -> String {
        match self.spec {
            Spec::Plain => text.underline().to_string(),
            Spec::Dim => text.underline().dimmed().to_string(),
            Spec::Color(c) => text.underline().color(c).to_string(),
        }
    }

    pub fn on_italic(&self, text: &str) -> String {
        match self.spec {
            Spec::Plain => text.italic().to_string(),
            Spec::Dim => text.italic().dimmed().to_string(),
            Spec::Color(c) => text.italic().color(c).to_string(),
        }
    }
}

pub struct Palette {
    pub status: Paint,
    pub ok: Paint,
    pub err: Paint,
    pub note: Paint,
    pub summary: Paint,
    pub thought: Paint,
    pub tool_call: Paint,
    pub tool_result: Paint,
    pub preview: Paint,
    pub code: Paint,
    pub link: Paint,
    pub url: Paint,
    pub headings: [Paint; 4],
    pub list: Paint,
    pub quote: Paint,
    pub rule: Paint,
}

impl Palette {
    fn paint(key: &str, user: &HashMap<String, String>, fallback: Spec) -> Paint {
        user.get(key)
            .map(|v| Paint::of(Spec::parse(v)))
            .unwrap_or(Paint::of(fallback))
    }

    pub fn from(user: &HashMap<String, String>) -> Self {
        Self {
            status: Self::paint("status", user, Spec::Color(C::BrightBlack)),
            ok: Self::paint("ok", user, Spec::Color(C::Green)),
            err: Self::paint("err", user, Spec::Color(C::Red)),
            note: Self::paint("note", user, Spec::Color(C::BrightBlack)),
            summary: Self::paint("summary", user, Spec::Dim),
            thought: Self::paint("thought", user, Spec::Color(C::BrightBlack)),
            tool_call: Self::paint("tool_call", user, Spec::Color(C::BrightCyan)),
            tool_result: Self::paint("tool_result", user, Spec::Color(C::BrightBlack)),
            preview: Self::paint("preview", user, Spec::Dim),
            code: Self::paint("code", user, Spec::Dim),
            link: Self::paint("link", user, Spec::Color(C::Blue)),
            url: Self::paint("url", user, Spec::Dim),
            headings: [
                Self::paint("h1", user, Spec::Color(C::BrightWhite)),
                Self::paint("h2", user, Spec::Color(C::BrightCyan)),
                Self::paint("h3", user, Spec::Color(C::Cyan)),
                Paint::of(Spec::Dim),
            ],
            list: Self::paint("list", user, Spec::Color(C::Cyan)),
            quote: Self::paint("quote", user, Spec::Dim),
            rule: Self::paint("rule", user, Spec::Dim),
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::from(&HashMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ansi(text: &str) -> String {
        format!("{}", text.color(C::Green))
    }

    fn force() {
        colored::control::set_override(true);
    }

    #[test]
    fn name_parses_to_color() {
        force();
        let mut m = HashMap::new();
        m.insert("ok".to_string(), "cyan".to_string());
        let pal = Palette::from(&m);
        let out = pal.ok.on("✓");
        assert_eq!(out, ansi("✓").replace("32", "36"));
    }

    #[test]
    fn hex_parses_to_truecolor() {
        force();
        let mut m = HashMap::new();
        m.insert("ok".to_string(), "#ff0000".to_string());
        let pal = Palette::from(&m);
        let out = pal.ok.on("✓");
        assert!(out.contains("38;2;255;0;0"));
    }

    #[test]
    fn dim_role_emits_dim() {
        force();
        let mut m = HashMap::new();
        m.insert("summary".to_string(), "dim".to_string());
        let pal = Palette::from(&m);
        let out = pal.summary.on("done");
        assert!(out.contains("2m"));
    }

    #[test]
    fn plain_role_emits_no_style() {
        force();
        let mut m = HashMap::new();
        m.insert("note".to_string(), "none".to_string());
        let pal = Palette::from(&m);
        let out = pal.note.on("hi");
        assert_eq!(out, "hi");
    }

    #[test]
    fn unknown_name_falls_back_to_white() {
        force();
        let mut m = HashMap::new();
        m.insert("ok".to_string(), "notacolor".to_string());
        let pal = Palette::from(&m);
        let out = pal.ok.on("✓");
        assert_eq!(out, ansi("✓").replace("32", "37"));
    }

    #[test]
    fn bad_hex_falls_back_to_white() {
        force();
        let mut m = HashMap::new();
        m.insert("ok".to_string(), "#zzzzzz".to_string());
        let pal = Palette::from(&m);
        let out = pal.ok.on("✓");
        assert_eq!(out, ansi("✓").replace("32", "37"));
    }

    #[test]
    fn labels_differ_under_custom_palette() {
        force();
        let mut m = HashMap::new();
        m.insert("ok".to_string(), "green".to_string());
        m.insert("err".to_string(), "red".to_string());
        let pal = Palette::from(&m);
        assert_ne!(pal.ok.on("x"), pal.err.on("x"));
    }
}