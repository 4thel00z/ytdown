//! ytdown command-line interface.

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

mod app;

mod info;

mod selector;

#[allow(dead_code)] // wired into main in the `get` task
mod template;

mod table;

mod formats;

mod progress;

#[derive(Parser)]
#[command(
    name = "ytdown",
    version,
    about = "Resolve and download media via the ytdown library"
)]
struct Cli {
    /// Increase log verbosity (-v info, -vv debug).
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,

    /// Silence all logs.
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    /// Override the HTTP User-Agent.
    #[arg(long, global = true)]
    user_agent: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate shell completions.
    Completions {
        /// Target shell.
        shell: Shell,
    },
    /// Print resolved metadata as JSON.
    Info {
        /// Video, playlist, channel, or ytsearch: URL.
        url: String,
        /// Pretty-print the JSON.
        #[arg(long)]
        pretty: bool,
        /// Maximum number of collection entries to include.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// List available formats for a video.
    Formats {
        /// Video URL.
        url: String,
        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let mp = indicatif::MultiProgress::new();
    progress::init_tracing(cli.verbose, cli.quiet, &mp);
    if let Err(e) = run(cli, &mp).await {
        eprintln!("error: {e:#}");
        let code = if e.downcast_ref::<selector::UsageError>().is_some() {
            2
        } else {
            1
        };
        std::process::exit(code);
    }
}

async fn run(cli: Cli, _mp: &indicatif::MultiProgress) -> anyhow::Result<()> {
    let ua = cli.user_agent.clone();
    match cli.command {
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "ytdown", &mut std::io::stdout());
            Ok(())
        }
        Command::Info { url, pretty, limit } => {
            let yt = app::build_ytdown(ua.as_deref())?;
            info::run(&yt, &url, pretty, limit).await
        }
        Command::Formats { url, json } => {
            let yt = app::build_ytdown(ua.as_deref())?;
            formats::run(&yt, &url, json).await
        }
    }
}
