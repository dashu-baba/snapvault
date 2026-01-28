use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RepoConfig {
    pub version: u32,
    pub created_at: String,
}

impl RepoConfig {
    pub fn new() -> Self {
        Self {
            version: 1,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self::new()
    }
}
