use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct Time;

impl Handler for Time {
    fn spec(&self) -> Tool {
        Tool::function(
            "time",
            "Get the current time in the user's local timezone plus the UTC time. Storing the \
             user's phrase at the right local moment requires the local offset: compute the due \
             timestamp from 'local' and keep the offset, e.g. '11AM' => due_at '07:30:00+03:30'.",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        )
    }

    fn run(&self, _args: &Value) -> anyhow::Result<String> {
        let utc = chrono::Utc::now();
        let local = chrono::Local::now();
        Ok(format!(
            "local: {}\nutc: {}\noffset: {}\nzone: {}",
            local.to_rfc3339(),
            utc.to_rfc3339(),
            local.format("%:z"),
            local.format("%Z")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field<'a>(out: &'a str, name: &str) -> &'a str {
        out.lines()
            .find(|l| l.starts_with(&format!("{name}:")))
            .and_then(|l| l.split_once(':').map(|(_, rest)| rest.trim()))
            .unwrap_or("")
    }

    #[test]
    fn run_reports_local_offset_and_utc() {
        let out = Time.run(&json!({})).unwrap();
        for name in ["local", "utc"] {
            let ts = field(&out, name);
            assert!(
                chrono::DateTime::parse_from_rfc3339(ts).is_ok(),
                "{name} timestamp unparseable: {ts:?}"
            );
        }
        let offset = field(&out, "offset");
        assert!(
            offset.starts_with('+') || offset.starts_with('-'),
            "bad offset {offset:?}"
        );
        assert!(!field(&out, "zone").is_empty());
    }

    #[test]
    fn local_matches_utc_line() {
        let out = Time.run(&json!({})).unwrap();
        let local = chrono::DateTime::parse_from_rfc3339(field(&out, "local")).unwrap();
        let utc = chrono::DateTime::parse_from_rfc3339(field(&out, "utc")).unwrap();
        let delta = local.with_timezone(&chrono::Utc)
            .signed_duration_since(utc)
            .num_seconds()
            .abs();
        assert!(
            delta <= 2,
            "local {local} does not map onto utc {utc} (off by {delta}s)"
        );
    }
}
