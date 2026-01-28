use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "snapvault")]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new backup repository
    Init {
        /// Path where the repository will be created
        #[arg(long)]
        repo: PathBuf,
    },
    /// Create a backup snapshot (basic: full copy + manifest)
    Backup {
        /// Source directory to backup
        #[arg(long)]
        source: PathBuf,
        /// Repository path
        #[arg(long)]
        repo: PathBuf,
    },
    /// List all snapshots in the repository
    List {
        /// Repository path
        #[arg(long)]
        repo: PathBuf,
    },
    /// Delete a snapshot or all snapshots from the repository
    Delete {
        /// Repository path
        #[arg(long)]
        repo: PathBuf,
        /// Snapshot ID to delete
        #[arg(long)]
        snapshot: Option<String>,
        /// Delete all snapshots
        #[arg(long)]
        all: bool,
    },
    /// Restore a snapshot to a directory
    Restore {
        /// Destination directory to restore to
        #[arg(long)]
        dest: PathBuf,
        /// Snapshot ID to restore (latest if not provided)
        #[arg(long)]
        snapshot: Option<String>,
        /// Repository path
        #[arg(long)]
        repo: PathBuf,
    },
}
