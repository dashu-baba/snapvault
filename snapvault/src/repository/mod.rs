pub mod config;
pub mod snapshot;

use crate::error::{Result, SnapVaultError};
use crate::utils::MAX_CONFIG_SIZE;
use config::RepoConfig;
use log::info;
use std::fs;
use std::path::{Path, PathBuf};

/// Repository structure representing a SnapVault backup repository
pub struct Repository {
    root: PathBuf,
    config: RepoConfig,
}

impl Repository {
    /// Open an existing repository
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(SnapVaultError::RepoNotFound(path.to_path_buf()));
        }

        let config = Self::load_config(path)?;

        Ok(Self {
            root: path.to_path_buf(),
            config,
        })
    }

    /// Initialize a new repository
    pub fn init(path: &Path) -> Result<Self> {
        info!("Initializing repository at: {}", path.display());

        if path.exists() {
            return Err(SnapVaultError::RepoAlreadyExists(path.to_path_buf()));
        }

        fs::create_dir_all(path).map_err(|e| {
            SnapVaultError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to create repository directory: {}", e),
            ))
        })?;

        // Set permissions on Unix (owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path)?.permissions();
            perms.set_mode(0o700);
            fs::set_permissions(path, perms)?;
        }

        fs::create_dir_all(path.join("snapshots"))?;
        fs::create_dir_all(path.join("data"))?;

        let config = RepoConfig::new();
        let cfg_path = path.join("config.json");
        fs::write(&cfg_path, serde_json::to_string_pretty(&config)?)?;

        println!("✓ Repo initialized at {}", path.display());

        Ok(Self {
            root: path.to_path_buf(),
            config,
        })
    }

    /// Load repository configuration
    fn load_config(repo_path: &Path) -> Result<RepoConfig> {
        let config_path = repo_path.join("config.json");
        if !config_path.is_file() {
            return Err(SnapVaultError::InvalidRepo(config_path));
        }

        // Security: Check file size before reading
        let metadata = fs::metadata(&config_path)?;
        if metadata.len() > MAX_CONFIG_SIZE {
            return Err(SnapVaultError::FileTooLarge {
                size: metadata.len(),
                max: MAX_CONFIG_SIZE,
            });
        }

        let raw = fs::read_to_string(&config_path)?;
        let cfg: RepoConfig = serde_json::from_str(&raw)?;

        if cfg.version != 1 {
            return Err(SnapVaultError::UnsupportedVersion {
                version: cfg.version,
                expected: 1,
            });
        }

        Ok(cfg)
    }

    /// Get the root path of the repository
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get the snapshots directory path
    pub fn snapshots_dir(&self) -> PathBuf {
        self.root.join("snapshots")
    }

    /// Get the data directory path
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// Get repository configuration
    pub fn config(&self) -> &RepoConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_repository() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");

        let repo = Repository::init(&repo_path).unwrap();
        assert_eq!(repo.root(), repo_path);
        assert!(repo.snapshots_dir().exists());
        assert!(repo.data_dir().exists());
        assert!(repo_path.join("config.json").exists());
    }

    #[test]
    fn test_init_existing_fails() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");

        Repository::init(&repo_path).unwrap();
        let result = Repository::init(&repo_path);
        assert!(matches!(result, Err(SnapVaultError::RepoAlreadyExists(_))));
    }

    #[test]
    fn test_open_repository() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");

        Repository::init(&repo_path).unwrap();
        let repo = Repository::open(&repo_path).unwrap();
        assert_eq!(repo.root(), repo_path);
    }

    #[test]
    fn test_open_nonexistent_fails() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("nonexistent");

        let result = Repository::open(&repo_path);
        assert!(matches!(result, Err(SnapVaultError::RepoNotFound(_))));
    }
}
