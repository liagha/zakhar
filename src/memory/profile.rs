use std::path::PathBuf;

pub fn load() -> Option<String> {
    if let Some(home) = dirs::config_dir() {
        let p = home.join("zakhar/profile.md");
        if let Ok(text) = std::fs::read_to_string(&p)
            && !text.trim().is_empty()
        {
            println!("[memory] loaded profile from {}", p.display());
            return Some(text);
        }
    }
    let p = PathBuf::from("profile.md");
    if let Ok(text) = std::fs::read_to_string(&p)
        && !text.trim().is_empty()
    {
        println!("[memory] loaded profile from {}", p.display());
        return Some(text);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_profile_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        assert!(load().is_none());
        std::env::set_current_dir(&orig).unwrap();
    }
}
