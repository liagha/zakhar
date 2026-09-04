use std::path::PathBuf;

use crate::paths;

pub fn run() {
    if let Err(e) = migrate() {
        eprintln!("[zakhar] migration: {e}");
    }
}

fn migrate() -> anyhow::Result<()> {
    if paths::home().join("migrated").exists() {
        return Ok(());
    }
    paths::ensure_home()?;

    let old = paths::old_locations();
    let mut moved_any = false;

    if let Some(c) = old.get(0) {
        if c.join("config.toml").exists() {
            std::fs::create_dir_all(paths::config_dir())?;
            move_file(&c.join("config.toml"), &paths::config_path())?;
            moved_any = true;
        }
        if c.join("profile.md").exists() {
            std::fs::create_dir_all(paths::config_dir())?;
            move_file(&c.join("profile.md"), &paths::profile_path())?;
            moved_any = true;
        }
        if c.exists() && is_empty(c) {
            let _ = std::fs::remove_dir(c);
        }
    }

    if let Some(d) = old.get(1) {
        if d.join("sessions").exists() {
            std::fs::create_dir_all(paths::sessions_dir())?;
            move_dir_contents(&d.join("sessions"), &paths::sessions_dir())?;
            moved_any = true;
        }
        if d.join("completion.bash").exists() && !paths::home().join("completion.bash").exists() {
            move_file(&d.join("completion.bash"), &paths::home().join("completion.bash"))?;
            moved_any = true;
        }
        if d.exists() && is_empty(d) {
            let _ = std::fs::remove_dir(d);
        }
    }

    if let Some(s) = old.get(2) {
        if s.join("reminders.json").exists() {
            move_file(&s.join("reminders.json"), &paths::reminders_path())?;
            moved_any = true;
        }
        if s.exists() && is_empty(s) {
            let _ = std::fs::remove_dir(s);
        }
    }

    if moved_any {
        println!("[zakhar] migrated data into ~/.zakhar");
    }
    std::fs::write(paths::home().join("migrated"), "")?;
    Ok(())
}

fn move_file(from: &PathBuf, to: &PathBuf) -> anyhow::Result<()> {
    if to.exists() {
        let _ = std::fs::remove_file(from);
        return Ok(());
    }
    std::fs::rename(from, to)?;
    Ok(())
}

fn move_dir_contents(from: &PathBuf, to: &PathBuf) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if !dest.exists() {
            std::fs::rename(entry.path(), dest)?;
        } else {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    if is_empty(from) {
        let _ = std::fs::remove_dir(from);
    }
    Ok(())
}

fn is_empty(dir: &PathBuf) -> bool {
    std::fs::read_dir(dir)
        .map(|mut r| r.next().is_none())
        .unwrap_or(true)
}
