pub fn load() -> Option<String> {
    let p = crate::paths::profile_path();
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
        let prev = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", dir.path()) };
        assert!(
            load().is_none(),
            "should not find profile under empty HOME"
        );
        match prev {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}
