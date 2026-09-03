mod agent;
mod cli;
mod config;
mod delegate;
mod hooks;
mod invoke;
mod memory;
mod provider;
mod registry;
mod session;
mod slash;
mod types;
mod ui;

use clap::Parser;

#[derive(Parser)]
#[command(name = "zakhar", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    Chat {
        #[arg(long, short)]
        provider: Option<String>,
        #[arg(long, short)]
        model: Option<String>,
        #[arg(long, short)]
        agent: Option<String>,
        #[arg(long, default_value_t = true, default_missing_value = "true", num_args = 0..=1)]
        invoke: bool,
        #[arg(long)]
        auto: bool,
        #[arg(long)]
        plan: bool,
        #[arg(long)]
        simple: bool,
    },
    Models {
        provider: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    match Cli::parse().command {
        Command::Chat {
            provider,
            model,
            agent,
            invoke,
            auto,
            plan,
            simple,
        } => cli::chat(provider, model, agent, invoke, auto, plan, simple).await?,
        Command::Models { provider } => cli::models(provider).await?,
    }
    Ok(())
}
