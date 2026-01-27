use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use uuid::Uuid;
use walkdir::WalkDir;

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
    /// Create a backup snapshot (basic: full copy + manifest)
    Backup {
        /// Source directory to backup
        source_path: PathBuf,
        /// Repository path
        #[arg(long)]
        repo: PathBuf,
    },
}

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

#[derive(Serialize, Deserialize, Debug)]
struct SnapshotManifest {
    snapshot_id: String,
    created_at: String,
    source_root: String,
    total_files: u64,
    total_bytes: u64,
    files: Vec<FileRecord>,
}

#[derive(Serialize, Deserialize, Debug)]
struct FileRecord {
    rel_path: String,
    size: u64,
    modified: Option<String>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Init { repo_path } => init_repository(&repo_path),
        Commands::Backup { source_path, repo } => backup_basic(&source_path, &repo),
    }
}

fn init_repository(repo_path: &Path) -> Result<()> {
    info!("Initializing repository at: {}", repo_path.display());

    if repo_path.exists() {
        anyhow::bail!(
            "Repository path already exists: {}\nUse a new path or remove the existing directory.",
            repo_path.display()
        );
    }

    fs::create_dir_all(repo_path).context("Failed to create repository directory")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(repo_path)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(repo_path, perms).context("Failed to set repository permissions")?;
    }

    fs::create_dir_all(repo_path.join("snapshots")).context("Failed to create snapshots dir")?;
    fs::create_dir_all(repo_path.join("data")).context("Failed to create data dir")?;

    let cfg = RepoConfig::new();
    let cfg_path = repo_path.join("config.json");
    fs::write(&cfg_path, serde_json::to_string_pretty(&cfg)?).context("Failed to write config")?;

    println!("✓ Repo initialized at {}", repo_path.display());
    Ok(())
}

fn load_repo_config(repo_path: &Path) -> Result<RepoConfig> {
    let config_path = repo_path.join("config.json");
    if !config_path.is_file() {
        anyhow::bail!(
            "Not a SnapVault repo: missing {}\nRun: snapvault init <new_repo_path>",
            config_path.display()
        );
    }
    let raw = fs::read_to_string(&config_path).context("Failed to read repo config")?;
    let cfg: RepoConfig = serde_json::from_str(&raw).context("Invalid repo config JSON")?;
    if cfg.version != 1 {
        anyhow::bail!("Unsupported repo version {} (expected 1).", cfg.version);
    }
    Ok(cfg)
}

/// BASIC BACKUP: full copy into repo/data/<snapshot_id>/... + manifest to repo/snapshots/<snapshot_id>.json
fn backup_basic(source_path: &Path, repo_path: &Path) -> Result<()> {
    // Validate source
    if !source_path.exists() {
        anyhow::bail!("Source path does not exist: {}", source_path.display());
    }
    if !source_path.is_dir() {
        anyhow::bail!("Source path is not a directory: {}", source_path.display());
    }

    // Validate repo
    if !repo_path.exists() {
        anyhow::bail!("Repo path does not exist: {}", repo_path.display());
    }
    let _cfg = load_repo_config(repo_path)?;

    let snapshot_id = format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        &Uuid::new_v4().to_string()[..8]
    );
    let data_root = repo_path.join("data").join(&snapshot_id);
    let snapshot_manifest_path = repo_path.join("snapshots").join(format!("{snapshot_id}.json"));

    fs::create_dir_all(&data_root).context("Failed to create snapshot data directory")?;

    info!(
        "Starting basic backup: source={}, repo={}, snapshot_id={}",
        source_path.display(),
        repo_path.display(),
        snapshot_id
    );

    let backup_result = (|| -> Result<(u64, u64, Vec<FileRecord>)> {
        let mut files: Vec<FileRecord> = Vec::new();
        let mut total_files: u64 = 0;
        let mut total_bytes: u64 = 0;

        for entry in WalkDir::new(source_path).follow_links(false) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!("Walk error: {}", e);
                    continue;
                }
            };

            let path = entry.path();
            let ft = entry.file_type();

            if ft.is_dir() {
                continue;
            }
            if ft.is_symlink() {
                warn!("Skipping symlink: {}", path.display());
                continue;
            }
            if !ft.is_file() {
                continue;
            }

            
            let md = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    warn!("Metadata error for {}: {}", path.display(), e);
                    continue;
                }
            };

            let modified = md
                .modified()
                .ok()
                .and_then(|t| systemtime_to_rfc3339(t).ok());

            // Build relative path
            let rel = match path.strip_prefix(source_path) {
                Ok(r) => r,
                Err(_) => {
                    warn!("Failed to compute relative path for {}", path.display());
                    continue;
                }
            };

            let rel_str = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");

            let dest_path = data_root.join(rel);

            // Ensure parent directories exist
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent).context("Failed to create destination parent dir")?;
            }

            
            let bytes_copied = copy_file(path, &dest_path).with_context(|| {
                format!(
                    "Failed to copy {} -> {}",
                    path.display(),
                    dest_path.display()
                )
            })?;

            total_files += 1;
            total_bytes = total_bytes.saturating_add(bytes_copied);

            files.push(FileRecord {
                rel_path: rel_str,
                size: bytes_copied,
                modified,
            });
        }

        Ok((total_files, total_bytes, files))
    })();

    
    let (total_files, total_bytes, files) = match backup_result {
        Ok(result) => result,
        Err(e) => {
            warn!("Backup failed, cleaning up partial data");
            let _ = fs::remove_dir_all(&data_root);
            return Err(e);
        }
    };

    let manifest = SnapshotManifest {
        snapshot_id: snapshot_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source_root: source_path.to_string_lossy().to_string(),
        total_files,
        total_bytes,
        files,
    };

    fs::write(
        &snapshot_manifest_path,
        serde_json::to_string_pretty(&manifest).context("Failed to serialize manifest")?,
    )
    .context("Failed to write snapshot manifest")?;

    println!("✓ Backup complete");
    println!("  Snapshot:   {}", snapshot_id);
    println!("  Files:      {}", total_files);
    println!("  Bytes:      {}", total_bytes);
    println!("  Manifest:   {}", snapshot_manifest_path.display());
    println!("  Data root:  {}", data_root.display());

    Ok(())
}

fn copy_file(src: &Path, dst: &Path) -> io::Result<u64> {
    // fs::copy is fine for MVP; later we can do buffered streaming + atomic temp writes
    fs::copy(src, dst)
}

fn systemtime_to_rfc3339(t: SystemTime) -> Result<String> {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    Ok(dt.to_rfc3339())
}
