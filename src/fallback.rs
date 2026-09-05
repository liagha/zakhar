//! Fallback chains. A *candidate* is a (provider, model) pair. Given a primary
//! route, `chain` builds an ordered list of candidates: the primary first, then
//! any explicit `fallback` entries from the config, then every other configured
//! provider (with its own default model) as a last resort. So a config with a
//! single provider behaves exactly as before, and a config with several gets
//! automatic fail-over for free.
//!
//! `Fallback` is a provider that walks the chain. When a candidate fails at
//! request time (overloaded, unauthorized, unreachable), the next candidate is
//! tried. How the switch is decided is a `Decide` policy: `Ask` prompts the
//! user at the terminal, `Auto` switches silently, `Off` stops at the first
//! error. The choice is only made once per fallback chain; after it succeeds a
//! later hard failure falls back again.

use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::config::Config;
use crate::levels::Resolved;
use crate::provider::{DeltaStream, Provider};
use crate::types::ChatRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decide {
    Ask,
    Auto,
    Off,
}

pub fn chain(cfg: &Config, primary: Resolved, explicit: &[String]) -> Vec<Resolved> {
    let mut out: Vec<Resolved> = vec![primary];
    for raw in explicit {
        if let Some(route) = parse_route(cfg, raw) {
            push_unique(&mut out, route);
        }
    }
    let mut rest: Vec<String> = cfg.providers.keys().cloned().collect();
    rest.sort();
    for pid in rest {
        if out.iter().any(|r| r.provider == pid) {
            continue;
        }
        let model = cfg
            .providers
            .get(&pid)
            .and_then(|p| {
                if !p.default_model.is_empty() {
                    Some(p.default_model.clone())
                } else {
                    p.models.first().cloned()
                }
            })
            .unwrap_or_default();
        push_unique(&mut out, Resolved { provider: pid, model });
    }
    out
}

fn parse_route(cfg: &Config, raw: &str) -> Option<Resolved> {
    let (pid, model) = match raw.split_once('/') {
        Some((p, m)) => (p.to_string(), m.to_string()),
        None => (raw.to_string(), String::new()),
    };
    if !cfg.providers.contains_key(&pid) {
        return None;
    }
    let model = if model.is_empty() {
        cfg.providers
            .get(&pid)
            .and_then(|p| {
                if !p.default_model.is_empty() {
                    Some(p.default_model.clone())
                } else {
                    p.models.first().cloned()
                }
            })
            .unwrap_or_default()
    } else {
        model
    };
    Some(Resolved { provider: pid, model })
}

fn push_unique(out: &mut Vec<Resolved>, route: Resolved) {
    if !out
        .iter()
        .any(|r| r.provider == route.provider && r.model == route.model)
    {
        out.push(route);
    }
}

pub fn build(
    registry: &crate::provider::Registry,
    routes: &[Resolved],
    decide: Decide,
) -> anyhow::Result<Box<dyn Provider>> {
    let mut candidates: Vec<(Arc<dyn Provider>, String)> = Vec::new();
    for route in routes {
        if let Some(p) = registry.arc(&route.provider) {
            let model = if route.model.is_empty() {
                p.list_models().first().cloned().unwrap_or_default()
            } else {
                route.model.clone()
            };
            candidates.push((p, model));
        }
    }
    anyhow::ensure!(
        !candidates.is_empty(),
        "no usable provider candidates in fallback chain"
    );
    Ok(Box::new(Fallback {
        candidates,
        decide,
        chosen: Mutex::new(0),
    }))
}

struct Fallback {
    candidates: Vec<(Arc<dyn Provider>, String)>,
    decide: Decide,
    chosen: Mutex<usize>,
}

#[async_trait]
impl Provider for Fallback {
    fn id(&self) -> &str {
        self.candidates[0].0.id()
    }

    fn list_models(&self) -> Vec<String> {
        self.candidates[0].0.list_models()
    }

    async fn chat_stream(&self, request: ChatRequest) -> anyhow::Result<DeltaStream> {
        let mut idx = self.current();
        let mut last_err = None;
        while idx < self.candidates.len() {
            let (provider, model) = &self.candidates[idx];
            let mut req = request.clone();
            req.model = model.clone();
            match provider.chat_stream(req).await {
                Ok(stream) => {
                    self.set_current(idx);
                    return Ok(stream);
                }
                Err(e) => {
                    last_err = Some(e);
                    if idx + 1 >= self.candidates.len() {
                        break;
                    }
                    let next = &self.candidates[idx + 1];
                    let next_label = format!("{}/{}", next.0.id(), next.1);
                    match self.decide {
                        Decide::Off => break,
                        Decide::Auto => {
                            let msg = last_err.as_ref().unwrap().to_string();
                            tracing::warn!("{msg} — falling back to {next_label}");
                            idx += 1;
                            self.set_current(idx);
                        }
                        Decide::Ask => {
                            let msg = last_err.as_ref().unwrap().to_string();
                            if ask(&format!("{msg} — fall back to {next_label}?")) {
                                idx += 1;
                                self.set_current(idx);
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("fallback chain exhausted")))
    }
}

impl Fallback {
    fn current(&self) -> usize {
        *self.chosen.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_current(&self, idx: usize) {
        *self.chosen.lock().unwrap_or_else(|e| e.into_inner()) = idx;
    }
}

fn ask(question: &str) -> bool {
    print!("\r\x1b[2K· {question} [y/N] ");
    use std::io::Write;
    std::io::stdout().flush().ok();
    let ch = crate::term::read_key();
    let yes = matches!(ch, 'y' | 'Y');
    println!("{}", if yes { "yes" } else { "no" });
    yes
}