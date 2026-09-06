use zakhar::cli::{chat, clean, daemon, mobile, models, shout};

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

#[derive(Parser)]
#[command(name = "zakhar", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// free-text command, e.g. `zakhar search here`, `zakhar delete the logs`
    words: Vec<String>,
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
    Mobile {
        message: String,
        #[arg(long)]
        auto: bool,
        #[arg(long)]
        mock: bool,
    },
    Completion {
        #[arg(value_enum)]
        shell: Shell,
    },
    Daemon,
    /// Run as an MCP server over stdio, exposing read-only and knowledge tools.
    Mcp,
    /// Show every path zakhar writes to (its single ~/.zakhar home).
    Paths,
    /// Permanently delete everything zakhar stores in ~/.zakhar.
    Clean,
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

    if !matches!(
        cli.command,
        Some(Command::Paths)
            | Some(Command::Clean)
            | Some(Command::Completion { .. })
            | Some(Command::Mcp)
    ) {
        zakhar::migrate::run();
    }

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
            chat(provider, model, agent, invoke, auto, plan, simple, String::new())
                .await?
        }
        (Some(Command::Models { provider }), _) => models(provider).await?,
        (Some(Command::Mobile { message, auto, mock }), _) => {
            mobile(message, auto, mock).await?
        }
        (Some(Command::Completion { shell }), _) => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "zakhar", &mut std::io::stdout());
        }
        (Some(Command::Daemon), _) => daemon::run().await?,
        (Some(Command::Mcp), _) => zakhar::mcp::server::run()?,
        (Some(Command::Paths), _) => clean::paths_cmd(),
        (Some(Command::Clean), _) => clean::clean_cmd()?,
        (None, words) if !words.is_empty() => {
            shout(words.join(" ")).await?;
        }
        (None, _) => {
            let _ = Cli::command().print_help();
            println!();
        }
    }
    Ok(())
}
