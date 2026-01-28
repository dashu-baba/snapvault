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

// Constants for security limits
const MAX_CONFIG_SIZE: u64 = 1024 * 1024; // 1MB
const MAX_MANIFEST_SIZE: u64 = 100 * 1024 * 1024; // 100MB
const SNAPSHOT_UUID_LEN: usize = 8;

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

/// Validate snapshot ID to prevent path traversal
fn validate_snapshot_id(id: &str) -> Result<()> {
    if id.is_empty() {
        anyhow::bail!("Snapshot ID cannot be empty");
    }
    if id.contains('\0') {
        anyhow::bail!("Snapshot ID contains null byte");
    }
    if id.contains('/') || id.contains('\\') {
        anyhow::bail!("Snapshot ID cannot contain path separators");
    }
    if id.starts_with('.') {
        anyhow::bail!("Snapshot ID cannot start with dot");
    }
    Ok(())
}

/// Check if a path is safe (no traversal, no absolute paths, no null bytes)
fn is_safe_path(path_str: &str) -> bool {
    // Check for null bytes (security: null byte injection)
    if path_str.contains('\0') {
        return false;
    }
    
    let path = Path::new(path_str);
    if path.is_absolute() {
        return false;
    }
    
    for comp in path.components() {
        match comp {
            std::path::Component::Normal(_) => {},
            std::path::Component::ParentDir => return false,
            std::path::Component::CurDir => {}, // allow .
            _ => return false,
        }
    }
    true
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    match cli.command {
        Commands::Init { repo } => init_repository(&repo),
        Commands::Backup { source, repo } => backup_basic(&source, &repo),
        Commands::List { repo } => list_snapshots(&repo),
        Commands::Delete { repo, snapshot, all } => delete_snapshot(&repo, snapshot.as_deref(), all),
        Commands::Restore { dest, snapshot, repo } => restore(snapshot.as_deref(), &dest, &repo),
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
    
    // Security: Check file size before reading
    let metadata = fs::metadata(&config_path)
        .context("Failed to read config metadata")?;
    if metadata.len() > MAX_CONFIG_SIZE {
        anyhow::bail!(
            "Config file too large: {} bytes (max: {} bytes)",
            metadata.len(),
            MAX_CONFIG_SIZE
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
        &Uuid::new_v4().to_string()[..SNAPSHOT_UUID_LEN]
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
            warn!("Backup failed, cleaning up partial data at {}", data_root.display());
            if let Err(cleanup_err) = fs::remove_dir_all(&data_root) {
                warn!("Failed to cleanup partial backup: {}", cleanup_err);
            }
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

fn list_snapshots(repo_path: &Path) -> Result<()> {
    info!("Listing snapshots in repository: {}", repo_path.display());

    // Validate repo
    if !repo_path.exists() {
        anyhow::bail!("Repository path does not exist: {}", repo_path.display());
    }
    let _cfg = load_repo_config(repo_path)?;

    let snapshots_dir = repo_path.join("snapshots");
    if !snapshots_dir.exists() {
        println!("No snapshots found in repository.");
        return Ok(());
    }

    let mut snapshots: Vec<SnapshotManifest> = Vec::new();
    for entry in fs::read_dir(&snapshots_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension() == Some(std::ffi::OsStr::new("json")) {
            // Security: Check manifest size before reading
            let metadata = fs::metadata(&path)?;
            if metadata.len() > MAX_MANIFEST_SIZE {
                warn!("Skipping oversized manifest: {} ({} bytes)", path.display(), metadata.len());
                continue;
            }
            
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read manifest: {}", path.display()))?;
            let manifest: SnapshotManifest = serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse manifest: {}", path.display()))?;
            snapshots.push(manifest);
        }
    }

    if snapshots.is_empty() {
        println!("No snapshots found in repository.");
        return Ok(());
    }

    // Sort by created_at descending (latest first)
    snapshots.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    println!("Snapshots in repository {}:", repo_path.display());
    println!("{:<40} {:<25} {:<10} {:<10} {}", "Snapshot ID", "Created At", "Files", "Bytes", "Source Root");
    println!("{}", "-".repeat(100));

    for snap in snapshots {
        println!(
            "{:<40} {:<25} {:<10} {:<10} {}",
            snap.snapshot_id,
            snap.created_at,
            snap.total_files,
            snap.total_bytes,
            snap.source_root
        );
    }

    Ok(())
}

fn delete_single_snapshot(repo_path: &Path, snapshot_id: &str) -> Result<()> {
    // Security: Validate snapshot ID
    validate_snapshot_id(snapshot_id)?;

    let manifest_path = repo_path.join("snapshots").join(format!("{}.json", snapshot_id));
    let data_path = repo_path.join("data").join(snapshot_id);

    // Check if both manifest and data exist
    if !manifest_path.exists() {
        anyhow::bail!("Snapshot manifest not found: {}", manifest_path.display());
    }
    if !data_path.exists() {
        anyhow::bail!("Snapshot data directory not found: {}", data_path.display());
    }

    // Load manifest to verify it's a valid snapshot
    let raw = fs::read_to_string(&manifest_path)?;
    let manifest: SnapshotManifest = serde_json::from_str(&raw)?;
    if manifest.snapshot_id != snapshot_id {
        anyhow::bail!("Manifest snapshot ID mismatch");
    }

    // Delete data directory first
    info!("Removing snapshot data directory: {}", data_path.display());
    fs::remove_dir_all(&data_path).with_context(|| {
        format!("Failed to remove snapshot data directory: {}", data_path.display())
    })?;

    // Delete manifest file
    info!("Removing snapshot manifest: {}", manifest_path.display());
    fs::remove_file(&manifest_path).with_context(|| {
        format!("Failed to remove snapshot manifest: {}", manifest_path.display())
    })?;

    Ok(())
}

fn delete_snapshot(repo_path: &Path, snapshot_id_opt: Option<&str>, all: bool) -> Result<()> {
    // Validate arguments
    match (snapshot_id_opt, all) {
        (Some(_), true) => anyhow::bail!("Cannot specify both --snapshot and --all"),
        (None, false) => anyhow::bail!("Must specify either --snapshot or --all"),
        _ => {}
    }

    // Validate repo
    if !repo_path.exists() {
        anyhow::bail!("Repository path does not exist: {}", repo_path.display());
    }
    let _cfg = load_repo_config(repo_path)?;

    if let Some(snapshot_id) = snapshot_id_opt {
        info!("Deleting snapshot {} from repository {}", snapshot_id, repo_path.display());
        delete_single_snapshot(repo_path, snapshot_id)?;
        println!("✓ Snapshot {} deleted successfully", snapshot_id);
    } else {
        // all is true
        info!("Deleting all snapshots from repository {}", repo_path.display());

        let snapshots_dir = repo_path.join("snapshots");
        if !snapshots_dir.exists() {
            println!("No snapshots found in repository.");
            return Ok(());
        }

        let mut snapshot_ids: Vec<String> = Vec::new();
        for entry in fs::read_dir(&snapshots_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension() == Some(std::ffi::OsStr::new("json")) {
                if let Some(stem) = path.file_stem() {
                    snapshot_ids.push(stem.to_string_lossy().to_string());
                }
            }
        }

        if snapshot_ids.is_empty() {
            println!("No snapshots found in repository.");
            return Ok(());
        }

        let total_snapshots = snapshot_ids.len();
        let mut deleted_count = 0;
        for id in snapshot_ids {
            match delete_single_snapshot(repo_path, &id) {
                Ok(()) => {
                    println!("✓ Snapshot {} deleted successfully", id);
                    deleted_count += 1;
                }
                Err(e) => {
                    warn!("Failed to delete snapshot {}: {}", id, e);
                }
            }
        }

        println!("✓ Deleted {} out of {} snapshots", deleted_count, total_snapshots);
    }

    Ok(())
}

fn restore(snapshot_id_opt: Option<&str>, dest_path: &Path, repo_path: &Path) -> Result<()> {
    // Validate repo
    if !repo_path.exists() {
        anyhow::bail!("Repository path does not exist: {}", repo_path.display());
    }
    let _cfg = load_repo_config(repo_path)?;

    // Determine snapshot ID
    let snapshot_id = if let Some(id) = snapshot_id_opt {
        // Security: Validate snapshot ID
        validate_snapshot_id(id)?;
        
        if !repo_path.join("snapshots").join(format!("{}.json", id)).exists() {
            anyhow::bail!("Snapshot {} not found", id);
        }
        id.to_string()
    } else {
        // Find latest snapshot
        let snapshots_dir = repo_path.join("snapshots");
        if !snapshots_dir.exists() {
            anyhow::bail!("No snapshots found in repository");
        }
        let mut snapshots: Vec<String> = fs::read_dir(&snapshots_dir)?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension() == Some(std::ffi::OsStr::new("json")) {
                    Some(path.file_stem()?.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();
        if snapshots.is_empty() {
            anyhow::bail!("No snapshots found in repository");
        }
        snapshots.sort_by(|a, b| b.cmp(a)); // descending order, since newer snapshots have larger IDs
        snapshots[0].clone()
    };

    info!("Restoring snapshot {} to {}", snapshot_id, dest_path.display());

    // Validate dest
    if dest_path.exists() {
        if !dest_path.is_dir() {
            anyhow::bail!("Destination path is not a directory: {}", dest_path.display());
        }
        if fs::read_dir(dest_path)?.next().is_some() {
            anyhow::bail!("Destination directory is not empty: {}", dest_path.display());
        }
    } else {
        fs::create_dir_all(dest_path).context("Failed to create destination directory")?;
    }

    // Load manifest
    let manifest_path = repo_path.join("snapshots").join(format!("{}.json", snapshot_id));
    if !manifest_path.is_file() {
        anyhow::bail!("Snapshot manifest not found: {}", manifest_path.display());
    }
    
    // Security: Check manifest size before reading
    let metadata = fs::metadata(&manifest_path)
        .context("Failed to read manifest metadata")?;
    if metadata.len() > MAX_MANIFEST_SIZE {
        anyhow::bail!(
            "Manifest file too large: {} bytes (max: {} bytes)",
            metadata.len(),
            MAX_MANIFEST_SIZE
        );
    }
    
    let raw = fs::read_to_string(&manifest_path).context("Failed to read snapshot manifest")?;
    let manifest: SnapshotManifest = serde_json::from_str(&raw).context("Failed to parse snapshot manifest")?;
    if manifest.snapshot_id != snapshot_id {
        anyhow::bail!("Manifest snapshot ID mismatch");
    }

    // Check data root exists
    let data_root = repo_path.join("data").join(&snapshot_id);
    if !data_root.exists() {
        anyhow::bail!("Snapshot data directory not found: {}", data_root.display());
    }

    // Restore files
    let mut restored_count = 0;
    let total_files = manifest.files.len();
    
    for (idx, file) in manifest.files.iter().enumerate() {
        // Security: Validate path safety
        if !is_safe_path(&file.rel_path) {
            warn!("Skipping unsafe path: {}", file.rel_path);
            continue;
        }

        let src_path = data_root.join(&file.rel_path);
        let dst_path = dest_path.join(&file.rel_path);

        // Create parent directories
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory for {}", dst_path.display())
            })?;
        }

        // Copy file
        fs::copy(&src_path, &dst_path).with_context(|| {
            format!("Failed to copy {} to {}", src_path.display(), dst_path.display())
        })?;

        restored_count += 1;
        
        // Log progress every 100 files
        if restored_count % 100 == 0 {
            info!("Restored {}/{} files", restored_count, total_files);
        }
    }

    println!("✓ Restore complete");
    println!("  Snapshot:     {}", snapshot_id);
    println!("  Files:        {}", restored_count);
    println!("  Destination:  {}", dest_path.display());

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
