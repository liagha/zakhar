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
        println!("edit ~/.config/zakhar/config.toml");
    }
    Ok(())
}
