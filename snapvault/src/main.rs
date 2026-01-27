use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::{info};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// SnapVault - Encrypted, incremental backup tool
#[derive(Parser)]
#[command(name = "snapvault")]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new backup repository
    Init {
        /// Path where the repository will be created
        repo_path: PathBuf,
    },
}

/// Repository configuration stored in config file
#[derive(Serialize, Deserialize)]
struct RepoConfig {
    version: u32,
    created_at: String,
}

impl RepoConfig {
    fn new() -> Self {
        Self {
            version: 1,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

fn main() -> Result<()> {
    // Initialize logger (RUST_LOG=info cargo run for verbose output)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { repo_path } => init_repository(repo_path)?,
    }

    Ok(())
}

/// Initialize a new backup repository
fn init_repository(repo_path: PathBuf) -> Result<()> {
    info!("Initializing repository at: {}", repo_path.display());

    // Security: Check if repository already exists to prevent accidental overwrite
    if repo_path.exists() {
        anyhow::bail!(
            "Repository path already exists: {}\nUse a different path or remove the existing directory.",
            repo_path.display()
        );
    }

    // Create main repository directory with restricted permissions (owner only)
    fs::create_dir(&repo_path)
        .context("Failed to create repository directory")?;
    
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&repo_path)?.permissions();
        perms.set_mode(0o700); // rwx------ (owner only)
        fs::set_permissions(&repo_path, perms)
            .context("Failed to set repository permissions")?;
        info!("Set repository permissions to 0700 (owner-only access)");
    }

    // Create subdirectories
    let snapshots_dir = repo_path.join("snapshots");
    let data_dir = repo_path.join("data");

    fs::create_dir(&snapshots_dir)
        .context("Failed to create snapshots directory")?;
    fs::create_dir(&data_dir)
        .context("Failed to create data directory")?;

    info!("Created directory structure");

    // Create configuration file
    let config = RepoConfig::new();
    let config_path = repo_path.join("config");
    let config_json = serde_json::to_string_pretty(&config)
        .context("Failed to serialize configuration")?;
    
    fs::write(&config_path, config_json)
        .context("Failed to write configuration file")?;

    info!("Repository initialized successfully");
    println!("✓ Repository created at: {}", repo_path.display());
    println!("✓ Configuration written");
    println!("\nNext steps:");
    println!("  snapvault backup <source> --repo {}", repo_path.display());

    Ok(())
}
