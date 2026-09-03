mod agent;
mod cli;
mod config;
mod delegate;
mod handler;
mod hooks;
mod invoke;
mod memory;
mod provider;
mod registry;
mod session;
mod slash;
mod tools;
mod types;
mod ui;

use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "zakhar", version, about)]
struct Cli {
    /// free-text command, e.g. `zakhar search here`, `zakhar delete the logs`
    #[arg(trailing_var_arg = true)]
    words: Vec<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
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

    let cli = Cli::parse();
    match (cli.command, cli.words) {
        (Some(Command::Chat {
            provider,
            model,
            agent,
            invoke,
            auto,
            plan,
            simple,
        }), _) => {
            cli::chat(provider, model, agent, invoke, auto, plan, simple, String::new())
                .await?
        }
        (Some(Command::Models { provider }), _) => cli::models(provider).await?,
        (None, words) if !words.is_empty() => {
            cli::shout(words.join(" ")).await?;
        }
        (None, _) => {
            let _ = Cli::command().print_help();
            println!();
        }
    }
    Ok(())
}
