//! ytdown command-line interface.

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

#[allow(dead_code)] // wired into main in the info task
mod app;

mod selector;

#[allow(dead_code)] // wired into main in the `get` task
mod template;

#[allow(dead_code)] // wired into main in the `formats` task
mod table;

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
    match cli.command {
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "ytdown", &mut std::io::stdout());
            Ok(())
        }
    }
}
