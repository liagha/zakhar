//! Readline wrapper around rustyline: arrow-key editing, in-line cursor
//! movement, and persisted history, so interactive chat input behaves like a
//! real shell.

use std::sync::{Mutex, OnceLock};

use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use rustyline::Editor;

type RL = Editor<(), FileHistory>;

static EDITOR: OnceLock<Mutex<Option<RL>>> = OnceLock::new();

fn editor() -> &'static Mutex<Option<RL>> {
    EDITOR.get_or_init(|| {
        let mut e = match RL::new() {
            Ok(e) => e,
            Err(_) => return Mutex::new(None),
        };
        let hist = crate::paths::home().join("history.txt");
        if std::fs::create_dir_all(hist.parent().unwrap()).is_ok() {
            let _ = e.load_history(&hist);
        }
        Mutex::new(Some(e))
    })
}

pub fn readline(prompt: &str) -> Option<String> {
    let mut guard = editor().lock().unwrap();
    let e = guard.as_mut()?;
    match e.readline(prompt) {
        Ok(line) => {
            let hist = crate::paths::home().join("history.txt");
            let _ = e.add_history_entry(line.as_str());
            let _ = e.append_history(&hist);
            if line.trim().is_empty() {
                None
            } else {
                Some(line.trim().to_string())
            }
        }
        Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => None,
        Err(_) => {
            guard.take();
            None
        }
    }
}

pub fn available() -> bool {
    editor().lock().unwrap().is_some()
}