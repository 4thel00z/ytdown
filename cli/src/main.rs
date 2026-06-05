//! ytdown command-line interface.

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

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

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "ytdown", &mut std::io::stdout());
        }
    }
}
