use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

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
    /// Create a backup snapshot (currently: scan + report)
    Backup {
        /// Source directory to backup
        source_path: PathBuf,
        /// Repository path
        #[arg(long)]
        repo: PathBuf,
    },
}

/// Repository configuration stored in config file
#[derive(Serialize, Deserialize, Debug)]
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
    // Initialize logger (RUST_LOG=info cargo run -- ...)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { repo_path } => init_repository(&repo_path),
        Commands::Backup { source_path, repo } => backup_scan(&source_path, &repo),
    }
}

/// Initialize a new backup repository
fn init_repository(repo_path: &Path) -> Result<()> {
    info!("Initializing repository at: {}", repo_path.display());

    // Safety: prevent accidental overwrite
    if repo_path.exists() {
        anyhow::bail!(
            "Repository path already exists: {}\nUse a new path or remove the existing directory.",
            repo_path.display()
        );
    }

    // Create repo directory (and parents)
    fs::create_dir_all(repo_path).context("Failed to create repository directory")?;

    // Restrict permissions on Unix (owner only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(repo_path)?.permissions();
        perms.set_mode(0o700); // rwx------ (owner only)
        fs::set_permissions(repo_path, perms).context("Failed to set repository permissions")?;
        info!("Set repository permissions to 0700 (owner-only access)");
    }

    // Create subdirectories
    let snapshots_dir = repo_path.join("snapshots");
    let data_dir = repo_path.join("data");

    fs::create_dir_all(&snapshots_dir).context("Failed to create snapshots directory")?;
    fs::create_dir_all(&data_dir).context("Failed to create data directory")?;

    // Write configuration file
    let config = RepoConfig::new();
    let config_path = repo_path.join("config.json");
    let config_json =
        serde_json::to_string_pretty(&config).context("Failed to serialize configuration")?;
    fs::write(&config_path, config_json).context("Failed to write configuration file")?;

    info!("Repository initialized successfully");
    println!("✓ Repository created at: {}", repo_path.display());
    println!("✓ Configuration written to: {}", config_path.display());
    println!("\nNext:");
    println!("  snapvault backup <source> --repo {}", repo_path.display());

    Ok(())
}

/// Validate repo exists AND looks like a SnapVault repo
fn load_repo_config(repo_path: &Path) -> Result<RepoConfig> {
    if !repo_path.exists() {
        anyhow::bail!(
            "Repository path does not exist: {}\nRun: snapvault init <new_repo_path>",
            repo_path.display()
        );
    }

    let config_path = repo_path.join("config.json");
    if !config_path.is_file() {
        anyhow::bail!(
            "Not a SnapVault repository: missing {}\nRun: snapvault init <new_repo_path>",
            config_path.display()
        );
    }

    let raw = fs::read_to_string(&config_path).context("Failed to read repo config")?;
    let cfg: RepoConfig = serde_json::from_str(&raw).context("Invalid repo config JSON")?;

    if cfg.version != 1 {
        anyhow::bail!(
            "Unsupported repo version {} (expected 1).",
            cfg.version
        );
    }

    Ok(cfg)
}

/// File metadata collected during scanning
#[derive(Debug)]
#[allow(dead_code)]
struct FileEntry {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

/// Scan source directory and collect file metadata (currently: scan + report only)
fn backup_scan(source_path: &Path, repo_path: &Path) -> Result<()> {
    info!("Starting backup scan of: {}", source_path.display());

    // Validate source exists
    if !source_path.exists() {
        anyhow::bail!("Source path does not exist: {}", source_path.display());
    }
    if !source_path.is_dir() {
        anyhow::bail!("Source path is not a directory: {}", source_path.display());
    }

    // Validate repo properly (not just "exists")
    let cfg = load_repo_config(repo_path)?;
    info!(
        "Using repository at {} (version {}, created_at {})",
        repo_path.display(),
        cfg.version,
        cfg.created_at
    );

    let mut files: Vec<FileEntry> = Vec::new();
    let mut total_size: u64 = 0;
    let mut dir_count: u64 = 0;
    let mut symlink_count: u64 = 0;

    for entry in WalkDir::new(source_path).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!("Error accessing path: {}", e);
                continue;
            }
        };

        let path = entry.path();
        let ft = entry.file_type(); // IMPORTANT: does not follow symlinks

        if ft.is_dir() {
            dir_count += 1;
            continue;
        }

        if ft.is_symlink() {
            symlink_count += 1;
            info!("Found symlink: {}", path.display());
            continue;
        }

        if ft.is_file() {
            // Only now read metadata (this follows symlinks, but we're already in is_file() branch)
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    warn!("Failed to read metadata for {}: {}", path.display(), e);
                    continue;
                }
            };

            let modified = match metadata.modified() {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to read modified time for {}: {}", path.display(), e);
                    SystemTime::UNIX_EPOCH
                }
            };

            let size = metadata.len();
            total_size = total_size.saturating_add(size);

            files.push(FileEntry {
                path: path.to_path_buf(),
                size,
                modified,
            });
        }
    }

    println!("\n✓ Scan complete");
    println!("  Files found:       {}", files.len());
    println!("  Directories:       {}", dir_count);
    println!("  Symlinks:          {}", symlink_count);
    println!(
        "  Total size:        {} bytes ({:.2} MB)",
        total_size,
        total_size as f64 / 1_048_576.0
    );

    info!(
        "Scanned {} files, total size: {} bytes",
        files.len(),
        total_size
    );

    Ok(())
}
