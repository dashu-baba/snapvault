use crate::error::Result;
use crate::repository::Repository;
use std::path::Path;

pub fn init(repo_path: &Path) -> Result<()> {
    Repository::init(repo_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_command() {
        let temp = TempDir::new().unwrap();
        let repo_path = temp.path().join("repo");

        init(&repo_path).unwrap();
        assert!(repo_path.exists());
        assert!(repo_path.join("config.json").exists());
        assert!(repo_path.join("snapshots").is_dir());
        assert!(repo_path.join("data").is_dir());
    }
}
