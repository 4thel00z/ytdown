//! ytdown command-line interface.

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

mod app;

mod get;

mod info;

mod selector;

mod template;

mod table;

mod formats;

mod progress;

mod search;

mod picker;

mod tui;

#[cfg(feature = "serve")]
mod serve;

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
    /// Search and list results.
    Search {
        /// Search query.
        query: String,
        /// Maximum number of results.
        #[arg(short = 'n', long = "limit", default_value_t = 10)]
        limit: usize,
        /// Emit JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Download a video, playlist, channel, or search result set.
    Get(get::GetArgs),
    /// Serve the browser (WASM) demo with a local CORS proxy.
    #[cfg(feature = "serve")]
    Serve {
        /// Port to bind on 127.0.0.1.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Open the demo in the default browser.
        #[arg(long)]
        open: bool,
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

async fn run(cli: Cli, mp: &indicatif::MultiProgress) -> anyhow::Result<()> {
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
        Command::Search { query, limit, json } => {
            let yt = app::build_ytdown(ua.as_deref())?;
            search::run(&yt, &query, limit, json).await
        }
        Command::Get(args) => {
            let yt = app::build_ytdown(ua.as_deref())?;
            get::run(&yt, mp, &args).await
        }
        #[cfg(feature = "serve")]
        Command::Serve { port, open } => serve::run(port, open).await,
    }
}
