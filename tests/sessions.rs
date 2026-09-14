use std::process::Command;

fn zakhar() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("zakhar"))
}

fn home() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".zakhar/sessions")).unwrap();
    dir
}

fn write_session(home: &std::path::Path, id: &str, created: &str, ask: &str) {
    let body = serde_json::json!({
        "id": id,
        "created_at": created,
        "messages": [
            { "role": "user", "content": ask }
        ]
    });
    std::fs::write(
        home.join(".zakhar/sessions").join(format!("{id}.json")),
        serde_json::to_string_pretty(&body).unwrap(),
    )
    .unwrap();
}

#[test]
fn sessions_lists_saved_sessions() {
    let dir = home();
    write_session(
        dir.path(),
        "aaaaaaaa-0000-0000-0000-000000000000",
        "2026-01-01T00:00:00Z",
        "refactor the parser",
    );
    write_session(
        dir.path(),
        "bbbbbbbb-0000-0000-0000-000000000000",
        "2026-06-01T00:00:00Z",
        "add dark mode",
    );

    let out = zakhar()
        .env("HOME", dir.path())
        .arg("sessions")
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("saved sessions:"), "got: {stdout}");
    assert!(stdout.contains("bbbbbbbb"), "got: {stdout}");
    assert!(stdout.contains("add dark mode"), "got: {stdout}");
    assert!(stdout.contains("aaaaaaaa"), "got: {stdout}");
    assert!(stdout.contains("refactor the parser"), "got: {stdout}");
}

#[test]
fn sessions_prints_empty_note_when_none() {
    let dir = home();
    let out = zakhar()
        .env("HOME", dir.path())
        .arg("sessions")
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.trim(), "no saved sessions");
}