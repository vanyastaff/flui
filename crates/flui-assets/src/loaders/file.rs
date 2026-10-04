//! File system asset loader.

use std::path::{Path, PathBuf};
use tokio::fs;

use crate::core::AssetMetadata;
use crate::error::{AssetError, Result};

/// Loads raw bytes from the file system.
///
/// This is a convenience loader for when you just need the raw file bytes.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::loaders::BytesFileLoader;
///
/// let loader = BytesFileLoader::new("assets");
/// let bytes = loader.load_bytes("config.json").await?;
/// ```
#[derive(Debug, Clone)]
pub struct BytesFileLoader {
    base_path: PathBuf,
}

impl BytesFileLoader {
    /// Creates a new bytes file loader.
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Loads raw bytes from a file.
    pub async fn load_bytes(&self, path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let full_path = self.base_path.join(path.as_ref());

        fs::read(&full_path)
            .await
            .map_err(|e| AssetError::LoadFailed {
                path: full_path.display().to_string(),
                reason: e.to_string(),
            })
    }

    /// Loads a UTF-8 string from a file.
    pub async fn load_string(&self, path: impl AsRef<Path>) -> Result<String> {
        let bytes = self.load_bytes(path).await?;
        String::from_utf8(bytes).map_err(|e| AssetError::LoadFailed {
            path: "string conversion".to_string(),
            reason: e.to_string(),
        })
    }

    /// Checks if a file exists.
    pub fn exists(&self, path: impl AsRef<Path>) -> bool {
        self.base_path.join(path.as_ref()).exists()
    }

    /// Gets file metadata.
    pub async fn metadata(&self, path: impl AsRef<Path>) -> Result<AssetMetadata> {
        let full_path = self.base_path.join(path.as_ref());

        let file_metadata = fs::metadata(&full_path)
            .await
            .map_err(|e| AssetError::LoadFailed {
                path: full_path.display().to_string(),
                reason: e.to_string(),
            })?;

        Ok(AssetMetadata {
            size_bytes: Some(file_metadata.len() as usize),
            format: full_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_uppercase),
            ..Default::default()
        })
    }
}
