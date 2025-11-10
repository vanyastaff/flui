//! File system asset loader.

use std::path::{Path, PathBuf};
use tokio::fs;

use crate::core::{Asset, AssetLoader, AssetMetadata};
use crate::error::{AssetError, Result};

/// Loads assets from the file system.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::loaders::FileLoader;
///
/// let loader = FileLoader::new("assets");
///
/// // Load an image
/// let data = loader.load(&AssetKey::new("images/logo.png")).await?;
/// ```
#[derive(Debug, Clone)]
pub struct FileLoader {
    /// Base directory for asset loading.
    base_path: PathBuf,
}

impl FileLoader {
    /// Creates a new file loader with the given base path.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let loader = FileLoader::new("assets");
    /// ```
    pub fn new(base_path: impl Into<PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }

    /// Resolves a key to a full file path.
    fn resolve_path(&self, path: &str) -> PathBuf {
        self.base_path.join(path)
    }
}

impl<T> AssetLoader<T> for FileLoader
where
    T: Asset<Error = AssetError>,
    T::Key: AsRef<str>,
{
    async fn load(&self, key: &T::Key) -> std::result::Result<T::Data, T::Error> {
        let path = self.resolve_path(key.as_ref());

        // Read file contents
        let _bytes = fs::read(&path).await.map_err(|e| AssetError::LoadFailed {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

        // For now, we can't generically construct T::Data from bytes
        // This will be handled by the concrete Asset implementations
        // For a generic loader, we'd need T::Data to implement a trait like FromBytes

        Err(AssetError::LoadFailed {
            path: path.display().to_string(),
            reason: "Generic file loading not supported - use concrete Asset types".to_string(),
        })
    }

    async fn exists(&self, key: &T::Key) -> std::result::Result<bool, T::Error> {
        let path = self.resolve_path(key.as_ref());
        Ok(path.exists())
    }

    async fn metadata(&self, key: &T::Key) -> std::result::Result<Option<AssetMetadata>, T::Error> {
        let path = self.resolve_path(key.as_ref());

        if !path.exists() {
            return Ok(None);
        }

        let file_metadata = fs::metadata(&path).await.map_err(|e| {
            AssetError::LoadFailed {
                path: path.display().to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok(Some(AssetMetadata {
            size_bytes: Some(file_metadata.len() as usize),
            // File extension as format hint
            format: path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_uppercase()),
            ..Default::default()
        }))
    }
}

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

        let file_metadata = fs::metadata(&full_path).await.map_err(|e| {
            AssetError::LoadFailed {
                path: full_path.display().to_string(),
                reason: e.to_string(),
            }
        })?;

        Ok(AssetMetadata {
            size_bytes: Some(file_metadata.len() as usize),
            format: full_path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_uppercase()),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_bytes_file_loader() {
        // Create a temporary directory
        let temp_dir = std::env::temp_dir().join("flui_assets_test");
        let _ = fs::create_dir_all(&temp_dir).await;

        // Create a test file
        let test_file = temp_dir.join("test.txt");
        let mut file = File::create(&test_file).await.unwrap();
        file.write_all(b"Hello, World!").await.unwrap();
        file.flush().await.unwrap();

        // Test loading
        let loader = BytesFileLoader::new(&temp_dir);
        let bytes = loader.load_bytes("test.txt").await.unwrap();
        assert_eq!(bytes, b"Hello, World!");

        // Test string loading
        let string = loader.load_string("test.txt").await.unwrap();
        assert_eq!(string, "Hello, World!");

        // Test exists
        assert!(loader.exists("test.txt"));
        assert!(!loader.exists("nonexistent.txt"));

        // Test metadata
        let metadata = loader.metadata("test.txt").await.unwrap();
        assert_eq!(metadata.size_bytes, Some(13));
        assert_eq!(metadata.format, Some("TXT".to_string()));

        // Cleanup
        let _ = fs::remove_file(test_file).await;
        let _ = fs::remove_dir(temp_dir).await;
    }
}
