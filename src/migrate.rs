use std::path::{Path, PathBuf};

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
    if migrate_from(&old)? {
        println!("[zakhar] migrated data into ~/.zakhar");
    }
    std::fs::write(paths::home().join("migrated"), "")?;
    Ok(())
}

fn migrate_from(old: &[PathBuf]) -> anyhow::Result<bool> {
    let mut moved_any = false;

    if let Some(c) = old.first() {
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

    Ok(moved_any)
}

fn move_file(from: &PathBuf, to: &PathBuf) -> anyhow::Result<()> {
    if to.exists() {
        let _ = std::fs::remove_file(from);
        return Ok(());
    }
    std::fs::rename(from, to)?;
    Ok(())
}

fn move_dir_contents(from: &Path, to: &Path) -> anyhow::Result<()> {
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

fn is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut r| r.next().is_none())
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::lock;
    use crate::paths::set_home;
    use std::fs;

    fn tmp() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let dir = tempfile::tempdir().unwrap();
        let g = lock();
        set_home(dir.path().join(".zakhar"));
        (dir, g)
    }

    fn make_old(root: &std::path::Path) -> Vec<PathBuf> {
        vec![
            root.join("config/zakhar"),
            root.join("data/zakhar"),
            root.join("state/zakhar"),
        ]
    }

    #[test]
    fn migrate_moves_known_files_and_drops_empty_dirs() {
        let (dir, _g) = tmp();
        let old = make_old(dir.path());
        for d in &old {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(old[0].join("config.toml"), "cfg").unwrap();
        fs::write(old[0].join("profile.md"), "prof").unwrap();
        fs::create_dir_all(old[0].join("leftover")).unwrap();
        fs::write(old[0].join("leftover/x.txt"), "x").unwrap();
        fs::create_dir_all(old[1].join("sessions")).unwrap();
        fs::write(old[1].join("sessions/a.json"), "{}").unwrap();
        fs::write(old[1].join("completion.bash"), "comp").unwrap();
        fs::write(old[2].join("reminders.json"), "[]").unwrap();

        assert!(migrate_from(&old).unwrap());

        assert_eq!(fs::read_to_string(paths::config_path()).unwrap(), "cfg");
        assert_eq!(fs::read_to_string(paths::profile_path()).unwrap(), "prof");
        assert_eq!(
            fs::read_to_string(paths::sessions_dir().join("a.json")).unwrap(),
            "{}"
        );
        assert_eq!(
            fs::read_to_string(paths::home().join("completion.bash")).unwrap(),
            "comp"
        );
        assert_eq!(
            fs::read_to_string(paths::reminders_path()).unwrap(),
            "[]"
        );

        assert!(old[0].exists());
        assert!(!old[1].exists());
        assert!(!old[2].exists());
    }

    #[test]
    fn migrate_second_call_moves_nothing() {
        let (dir, _g) = tmp();
        let old = make_old(dir.path());
        fs::create_dir_all(old[1].join("sessions")).unwrap();
        assert!(migrate_from(&old).unwrap());
        assert!(!migrate_from(&old).unwrap());
    }

    #[test]
    fn migrate_collision_keeps_existing_target() {
        let (dir, _g) = tmp();
        let old = make_old(dir.path());
        fs::create_dir_all(&old[0]).unwrap();
        fs::write(old[0].join("config.toml"), "new").unwrap();
        fs::create_dir_all(paths::config_dir()).unwrap();
        fs::write(paths::config_path(), "existing").unwrap();

        assert!(migrate_from(&old).unwrap());
        assert_eq!(fs::read_to_string(paths::config_path()).unwrap(), "existing");
        assert!(!old[0].join("config.toml").exists());
    }

    #[test]
    fn migrate_empty_old_dirs_removes_them() {
        let (dir, _g) = tmp();
        let old = make_old(dir.path());
        for d in &old {
            fs::create_dir_all(d).unwrap();
        }
        assert!(!migrate_from(&old).unwrap());
        assert!(old.iter().all(|d| !d.exists()));
    }
}