use clap::Parser;
use snapvault::cli::{Cli, Commands};
use snapvault::commands;
use snapvault::error::Result;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { repo } => commands::init(&repo),
        Commands::Backup { source, repo } => commands::backup(&source, &repo),
        Commands::List { repo } => commands::list(&repo),
        Commands::Delete {
            repo,
            snapshot,
            all,
        } => commands::delete(&repo, snapshot.as_deref(), all),
        Commands::Restore {
            dest,
            snapshot,
            repo,
        } => commands::restore(snapshot.as_deref(), &dest, &repo),
    }
}
