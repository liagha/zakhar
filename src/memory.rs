use std::path::Path;

pub fn load() -> Option<String> {
    let mut parts = Vec::new();
    let candidates = [
        "ZAKHAR.md",
        "CLAUDE.md",
        "AGENTS.md",
        ".zakhar/MEMORY.md",
        ".claude/MEMORY.md",
    ];
    for name in candidates {
        let p = Path::new(name);
        if p.exists()
            && let Ok(text) = std::fs::read_to_string(p)
                && !text.trim().is_empty() {
                    parts.push(format!("--- {name} ---\n{text}"));
                    println!("[memory] loaded {name} ({} bytes)", text.len());
                }
    }
    if let Some(home) = dirs::config_dir() {
        let p = home.join("zakhar/memory.md");
        if p.exists()
            && let Ok(text) = std::fs::read_to_string(&p)
                && !text.trim().is_empty() {
                    parts.push(format!("--- config/memory.md ---\n{text}"));
                    println!("[memory] loaded {} ({} bytes)", p.display(), text.len());
                }
    }
    if parts.is_empty() {
        None
    } else {
        let combined = parts.join("\n\n");
        println!("[memory] total {} bytes from {} file(s)", combined.len(), parts.len());
        std::io::Write::flush(&mut std::io::stdout()).ok();
        Some(combined)
    }
}
