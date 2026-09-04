use std::path::PathBuf;

pub fn home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".zakhar")
}

pub fn config_dir() -> PathBuf {
    home().join("config")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn profile_path() -> PathBuf {
    config_dir().join("profile.md")
}

pub fn sessions_dir() -> PathBuf {
    home().join("sessions")
}

pub fn reminders_path() -> PathBuf {
    home().join("reminders.json")
}

pub fn ensure_home() -> anyhow::Result<()> {
    std::fs::create_dir_all(home())?;
    Ok(())
}

pub fn project_dir() -> PathBuf {
    PathBuf::from(".zakhar")
}

pub fn list() -> Vec<PathBuf> {
    let mut v = Vec::new();
    v.push(home());
    let old = old_locations();
    for p in old {
        v.push(p);
    }
    v
}

pub fn old_locations() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(c) = dirs::config_dir() {
        v.push(c.join("zakhar"));
    }
    if let Some(d) = dirs::data_local_dir() {
        v.push(d.join("zakhar"));
    }
    if let Some(s) = dirs::state_dir() {
        v.push(s.join("zakhar"));
    }
    v
}
