mod agent;
mod cli;
mod config;
mod delegate;
mod invoke;
mod provider;
mod registry;
mod session;
mod types;

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
    },
    Models {
        provider: Option<String>,
    },
    Serve {
        #[arg(long, short, default_value = "8787")]
        port: u16,
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
        } => cli::chat(provider, model, agent, invoke, auto).await?,
        Command::Models { provider } => cli::models(provider).await?,
        Command::Serve { port } => cli::serve(port).await?,
    }
    Ok(())
}
