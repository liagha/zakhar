use serde_json::Value;

use crate::types::Tool;

pub trait Handler: Send + Sync {
    fn spec(&self) -> Tool;
    fn run(&self, args: &Value) -> anyhow::Result<String>;
}
