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
    let pal = cfg.palette();
    let mut ui = Ui::new(false, &pal);

    let cap = crate::capabilities::detect(&cfg, &phrase);
    let chosen = crate::capabilities::resolve(&cfg, &cap, "heavy");
    let provider_id = chosen.provider;

    let model = (!chosen.model.is_empty())
        .then_some(chosen.model.clone())
        .or_else(|| {
            registry
                .get(&provider_id)
                .and_then(|p| p.list_models().first().cloned())
        })
        .unwrap_or_default();

    let primary = crate::levels::Resolved {
        provider: provider_id.clone(),
        model: model.clone(),
    };
    let explicit = cfg
        .capabilities
        .get(&cap)
        .map(|c| c.fallback.clone())
        .unwrap_or_default();
    let routes = crate::fallback::chain(&cfg, primary, &explicit);
    let provider_box = crate::fallback::build(&registry, &routes, crate::fallback::Decide::Ask)?;
    let p: &dyn crate::provider::Provider = provider_box.as_ref();
    crate::invoke::seed_models(p.list_models());

    let mut inv = Invoke::new();
    let mounted = inv.mount_servers(&cfg);
    if !mounted.is_empty() {
        ui.note(&format!("mcp: {}", mounted.join(", ")));
    }
    let mut runner = Runner::new(p, model.clone(), None);

    runner.push(crate::types::Message::system(
         "You are zakhar, a mate who does quick file/terminal chores from a single short phrase. \
          Short, dry, direct. Do exactly what the phrase asks. Read-only tools run freely. \
          Mutating tools (write/edit/bash) are confirmed by default, but if the user's \
          phrase grants permission (e.g. 'you have my permission', 'go ahead', 'don't ask'), call \
          control with action=allow first and then run mutating tools freely. To list models call \
          control with action=models. To start an interactive chat call control with action=chat. \
          Finish with a one-line mate-style summary of what you did."
            .to_string(),
    ));

    for (label, text) in crate::memory::load_blocks() {
        runner.push(crate::types::Message::system(format!(
            "{label}:\n{text}"
        )));
    }

    let mut tools = inv.definitions();
    tools.push(delegate::tool_def(&cfg));
    tools.push(delegate::handoff_tool_def(&cfg));
    tools.push(slash::tool_def());
    runner.set_tools(tools);

    if phrase_grants_permission(&phrase) {
        crate::invoke::grant();
    }

    runner.push(crate::types::Message::user(phrase));
    ui.status("…");

    let mut session = Session::new();
    let turn_start = std::time::Instant::now();
    let mut tool_count = 0usize;
    let text =
        run_tool_loop(&mut ui, &mut runner, &cfg, &inv, p, &mut session, &mut tool_count).await?;
    let secs = turn_start.elapsed().as_secs_f64();
    ui.summary(&format!("done · {secs:.1}s · {tool_count} tool(s) · {provider_id}/{model}"));

    if let Err(e) = crate::memory::episodic::append("phrase", &text) {
        println!("[memory] failed to log event: {e}");
    }
    let _ = crate::memory::mind::dispatch(&std::env::current_dir().unwrap_or_default());

    if let Some(seed) = crate::invoke::chat_message() {
        super::chat(None, None, None, true, false, false, false, seed).await?;
        return Ok(());
    }

    Ok(())
}

async fn run_tool_loop(
    ui: &mut Ui<'_>,
    runner: &mut Runner<'_>,
    cfg: &Config,
    inv: &Invoke,
    provider: &dyn crate::provider::Provider,
    session: &mut Session,
    tool_count: &mut usize,
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
                    ui.text(&t);
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
        *tool_count += tool_calls.len();

        if tool_calls.is_empty() {
            runner.push(crate::types::Message::assistant(full.clone(), None));
            ui.end();
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

        let mut allow_all = false;
        for tc in &tool_calls {
            let approved = if !is_mutating(&tc.name) || allow_all || crate::invoke::permitted() {
                true
            } else {
                let ch = ui.confirm(&format!("{}?", tc.name));
                match ch {
                    'a' => {
                        crate::invoke::grant();
                        allow_all = true;
                        true
                    }
                    'n' => false,
                    _ => true,
                }
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
            } else if tc.name == "ask" {
                ui.end();
                let out = inv.exec("ask", &tc.arguments);
                hooks::run_post(&tc.name, &tc.arguments, &out);
                outputs.insert(tc.id.clone(), out);
            } else {
                ui.status(format!("↷ {}", tc.name).as_str());
                let out = inv.exec(&tc.name, &tc.arguments);
                hooks::run_post(&tc.name, &tc.arguments, &out);
                let skill_msg = if tc.name == "skill"
                    && let Some(name) = tc.arguments.get("name").and_then(|v| v.as_str())
                    && !name.is_empty()
                    && !out.starts_with("error:")
                    && !out.contains("available skills")
                {
                    Some(format!(
                        "You have loaded the skill '{name}'. Apply its instructions:\n\n{out}"
                    ))
                } else {
                    None
                };
                outputs.insert(tc.id.clone(), out);
                if let Some(msg) = skill_msg {
                    runner.push(crate::types::Message::system(msg));
                }
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
    !crate::invoke::READONLY.contains(&name)
}

fn phrase_grants_permission(phrase: &str) -> bool {
    let p = phrase.to_lowercase();
    [
        "you have my permission",
        "go ahead",
        "don't ask",
        "no confirmation",
        "do it freely",
        "permission granted",
        "i authorize",
    ]
    .iter()
    .any(|marker| p.contains(marker))
}

#[derive(Default)]
struct ToolCallPartAccum {
    id: String,
    name: String,
    arguments: String,
}
