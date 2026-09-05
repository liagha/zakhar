use crate::config::Config;
use crate::registry;

pub async fn models(provider: Option<String>) -> anyhow::Result<()> {
    let cfg = Config::load()?;
    let registry = registry::build(&cfg);

    for id in registry.ids() {
        if let Some(filter) = &provider
            && &id != filter {
                continue;
            }
        let p = registry.get(&id).unwrap();
        println!("{id}");
        for model in p.list_models() {
            println!("  {model}");
        }
    }

    if cfg.providers.is_empty() {
        println!("no providers configured");
        println!("edit ~/.zakhar/config/config.toml");
    }

    if !cfg.capabilities.is_empty() {
        println!();
        let mut names: Vec<&String> = cfg.capabilities.keys().collect();
        names.sort();
        println!("capabilities:");
        for name in names {
            let cap = &cfg.capabilities[name];
            let provider = cap
                .provider
                .clone()
                .or_else(|| cfg.default_provider.clone())
                .unwrap_or_default();
            let model = cap
                .model
                .clone()
                .or_else(|| cfg.default_model.clone())
                .unwrap_or_default();
            let hints = cap.hints.join(", ");
            let hint_note = if hints.is_empty() { String::new() } else { format!("  (hints: {hints})") };
            let fb = if cap.fallback.is_empty() {
                String::new()
            } else {
                format!("  fallback: {}", cap.fallback.join(", "))
            };
            println!("  {name} → {provider}/{model}{hint_note}{fb}");
        }
    }
    Ok(())
}
