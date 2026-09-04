use crate::reminder;

pub async fn run() -> anyhow::Result<()> {
    println!("zakhar daemon started (pid {})", std::process::id());
    loop {
        for r in reminder::due_and_due() {
            let msg = format!("⏰ {}", r.message);
            super::remind::notify(&msg);
            println!("⏰ fired: {} — {}", r.id, r.message);
            if r.recurring.is_none() {
                reminder::mark_done(&r.id);
            } else if let Some(rr) = r.recurring {
                let next = advance(&r.due_at, &rr);
                if let Some(next) = next {
                    let _ = reminder::drop(&r.id);
                    let _ = reminder::add(r.message.clone(), next, Some(rr));
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

fn advance(due: &str, recurring: &str) -> Option<String> {
    let base = reminder::parse_due(due)?;
    let low = recurring.to_lowercase();
    let next = if low.contains("hour") {
        base + chrono::Duration::hours(1)
    } else if low.contains("minute") {
        base + chrono::Duration::minutes(5)
    } else if low.contains("day") || low.contains("daily") || low.contains("morning") || low.contains("noon") {
        base + chrono::Duration::days(1)
    } else if low.contains("week") {
        base + chrono::Duration::weeks(1)
    } else if low.contains("month") {
        base + chrono::Duration::days(30)
    } else {
        base + chrono::Duration::days(1)
    };
    Some(next.to_rfc3339())
}
