use std::collections::HashMap;

use futures::StreamExt;

use crate::agent::Runner;
use crate::config::Config;
use crate::delegate;
use crate::hooks;
use crate::invoke::Invoke;
use crate::registry;
use crate::session::Session;
use crate::slash;
use crate::types::ToolCall;
use crate::ui::Ui;

pub async fn shout(phrase: String) -> anyhow::Result<()> {
    let cfg = Config::load()?;
    let registry = registry::build(&cfg);
    let mut ui = Ui::new(false);

    let provider_id = registry::default_provider(&cfg);
    let p = registry
        .get(&provider_id)
        .ok_or_else(|| anyhow::anyhow!("unknown provider: {provider_id}"))?;

    let model = p.list_models().first().cloned().unwrap_or_default();
    crate::invoke::seed_model_list(p.list_models());

    let inv = Invoke::new();
    let mut runner = Runner::new(p, model, None);

    runner.push(crate::types::Message::system(
        "You are zakhar, a mate who does quick file/terminal chores from a single short phrase. \
         Short, dry, direct. Do exactly what the phrase asks. Read-only tools run freely. \
         Mutating tools (write/edit/bash/delete) are confirmed by default, but if the user's \
         phrase grants permission (e.g. 'you have my permission', 'go ahead', 'don't ask'), call \
         grant_permission first and then run mutating tools freely. To list models call \
         list_models. To start an interactive chat call open_chat. Finish with a one-line \
         mate-style summary of what you did."
            .to_string(),
    ));

    if let Some(mem) = crate::memory::load() {
        runner.push(crate::types::Message::system(format!(
            "Project memory:\n{mem}"
        )));
    }

    let mut tools = inv.definitions();
    tools.push(delegate::tool_def(&cfg));
    tools.push(delegate::handoff_tool_def(&cfg));
    tools.push(slash::tool_def());
    runner.set_tools(tools);

    runner.push(crate::types::Message::user(phrase));
    ui.status("…");

    let mut session = Session::new();
    let text = run_tool_loop(&mut ui, &mut runner, &cfg, &inv, p, &mut session).await?;

    if let Some(seed) = crate::invoke::take_oneshot_chat() {
        super::chat(None, None, None, true, false, false, false, seed).await?;
        return Ok(());
    }

    ui.ok(&text);
    Ok(())
}

async fn run_tool_loop(
    ui: &mut Ui,
    runner: &mut Runner<'_>,
    cfg: &Config,
    inv: &Invoke,
    provider: &dyn crate::provider::Provider,
    session: &mut Session,
) -> anyhow::Result<String> {
    loop {
        let mut stream = match runner.stream().await {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        let mut full = String::new();
        let mut tool_parts: HashMap<usize, ToolCallPartAccum> = HashMap::new();

        while let Some(event) = stream.next().await {
            let event = event?;
            match event {
                crate::provider::ChatStreamEvent::Reasoning(_) => {}
                crate::provider::ChatStreamEvent::Text(t) => {
                    full.push_str(&t);
                    ui.status(&preview(&full));
                }
                crate::provider::ChatStreamEvent::ToolCall(part) => {
                    let entry = tool_parts.entry(part.index).or_default();
                    if let Some(id) = part.id {
                        entry.id = id;
                    }
                    if let Some(name) = part.name {
                        entry.name = name;
                    }
                    if let Some(args) = part.arguments {
                        entry.arguments.push_str(&args);
                    }
                }
                _ => {}
            }
        }

        let tool_calls: Vec<ToolCall> = tool_parts
            .into_iter()
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|(_, acc)| {
                let args: serde_json::Value = serde_json::from_str(&acc.arguments)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                if acc.name.is_empty() {
                    return None;
                }
                Some(ToolCall {
                    id: acc.id,
                    name: acc.name,
                    arguments: args,
                })
            })
            .collect();

        if tool_calls.is_empty() {
            runner.push(crate::types::Message::assistant(full.clone(), None));
            return Ok(full);
        }

        runner.push(crate::types::Message::assistant(
            full,
            Some(tool_calls.clone()),
        ));

        let mut outputs: HashMap<String, String> = HashMap::new();
        let mut delegate_futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = String>>>> =
            Vec::new();
        let mut delegate_ids: Vec<String> = Vec::new();
        let mut delegate_kinds: Vec<String> = Vec::new();

        for tc in &tool_calls {
            let approved = if !is_mutating(&tc.name) || crate::invoke::allow_mutations() {
                true
            } else {
                ui.status(format!("confirm {}? [y/n]", tc.name).as_str());
                let mut resp = String::new();
                std::io::stdin().read_line(&mut resp)?;
                !matches!(resp.trim().to_lowercase().as_str(), "n" | "no")
            };

            if !approved {
                runner.push(crate::types::Message::tool(
                    tc.id.clone(),
                    "canceled by user".to_string(),
                ));
                continue;
            }

            if let Err(e) = hooks::run_pre(&tc.name, &tc.arguments) {
                ui.status(format!("blocked {}({})", tc.name, e).as_str());
                outputs.insert(tc.id.clone(), format!("blocked by pre-hook: {e}"));
                continue;
            }

            if tc.name == "delegate" || tc.name == "handoff" {
                let agent = tc
                    .arguments
                    .get("agent")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let task = tc
                    .arguments
                    .get("task")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if agent.is_empty() || task.is_empty() {
                    outputs.insert(tc.id.clone(), format!("error: {0} needs agent+task", tc.name));
                } else {
                    let cfg_clone = cfg.clone();
                    let prov_copy: &dyn crate::provider::Provider = provider;
                    let agent_c = agent.clone();
                    let task_c = task.clone();
                    let kind = tc.name.clone();
                    delegate_ids.push(tc.id.clone());
                    delegate_kinds.push(kind.clone());
                    delegate_futures.push(Box::pin(async move {
                        let res =
                            delegate::run(prov_copy, &cfg_clone, &agent_c, &task_c, 0, false).await;
                        hooks::run_post(
                            &kind,
                            &serde_json::json!({ "agent": agent_c, "task": task_c }),
                            &res,
                        );
                        res
                    }));
                }
            } else if tc.name == "slash" {
                let cmd = tc
                    .arguments
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let args = tc
                    .arguments
                    .get("args")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let out = slash::handle_ai(cmd, args, session, runner);
                hooks::run_post(&tc.name, &tc.arguments, &out);
                outputs.insert(tc.id.clone(), out);
            } else if tc.name == "ask_user" {
                ui.end();
                let out = inv.exec("ask_user", &tc.arguments);
                hooks::run_post(&tc.name, &tc.arguments, &out);
                outputs.insert(tc.id.clone(), out);
            } else {
                ui.status(format!("↷ {}", tc.name).as_str());
                let out = inv.exec(&tc.name, &tc.arguments);
                hooks::run_post(&tc.name, &tc.arguments, &out);
                outputs.insert(tc.id.clone(), out);
            }
        }

        if !delegate_futures.is_empty() {
            ui.status(format!("↻ {} sub-agent(s) …", delegate_futures.len()).as_str());
            let results = futures::future::join_all(delegate_futures).await;
            for ((id, kind), res) in delegate_ids.into_iter().zip(delegate_kinds).zip(results) {
                outputs.insert(id, res);
                ui.status(format!("↻ {kind} done").as_str());
            }
        }

        for tc in &tool_calls {
            let out = outputs
                .remove(&tc.id)
                .unwrap_or_else(|| "error: missing output".to_string());
            runner.push(crate::types::Message::tool(tc.id.clone(), out));
        }

        ui.status("↻ feeding results back …");
    }
}

fn is_mutating(name: &str) -> bool {
    !crate::invoke::READONLY_TOOLS.contains(&name)
}

fn preview(s: &str) -> String {
    let one = s.split('\n').next().unwrap_or("").trim();
    let mut out: String = one.chars().take(80).collect();
    if one.chars().count() > 80 {
        out.push('…');
    }
    out
}

#[derive(Default)]
struct ToolCallPartAccum {
    id: String,
    name: String,
    arguments: String,
}
