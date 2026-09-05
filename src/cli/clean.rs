use std::io::Write;

use crate::paths;

pub fn paths_cmd() {
    let home = paths::home();
    println!("zakhar home: {}", home.display());
    println!("  config      {}", paths::config_path().display());
    println!("  profile     {}", paths::profile_path().display());
    println!("  sessions    {}", paths::sessions_dir().display());
    println!("  reminders   {}", paths::reminders_path().display());
    if let Ok(b) = std::env::current_exe() {
        println!("  binary      {}", b.display());
    }
    println!();
    println!("per-project: {} (config, memory, skills — one per repo)",
        paths::project_dir().display());

    let old = paths::old_locations();
    let old_exists: Vec<_> = old.into_iter().filter(|p| p.exists()).collect();
    if !old_exists.is_empty() {
        println!();
        println!("legacy XDG dirs still present (can be removed):");
        for p in old_exists {
            println!("  {}", p.display());
        }
    }
}

pub fn clean_cmd() -> anyhow::Result<()> {
    let home = paths::home();
    if !home.exists() {
        println!("nothing to clean — no {}", home.display());
        return Ok(());
    }
    print!(
        "this permanently deletes everything in {} (config, sessions, reminders, memory) [y/N] ",
        home.display()
    );
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let a = line.trim().to_lowercase();
    if a != "y" && a != "yes" {
        println!("aborted");
        return Ok(());
    }
    std::fs::remove_dir_all(&home)?;
    println!("removed {}", home.display());
    for p in paths::old_locations() {
        if p.exists() {
            let _ = std::fs::remove_dir_all(&p);
            println!("removed legacy {}", p.display());
        }
    }
    Ok(())
}
