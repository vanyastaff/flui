//! Error types for asset operations.

use thiserror::Error;

/// Result type for asset operations.
pub type Result<T> = std::result::Result<T, AssetError>;

/// Errors that can occur during asset operations.
#[derive(Error, Debug, Clone)]
pub enum AssetError {
    /// Asset was not found at the specified location.
    #[error("Asset not found: {path}")]
    NotFound {
        /// The path or identifier that was not found.
        path: String,
    },

    /// Failed to load the asset from its source.
    #[error("Failed to load asset '{path}': {reason}")]
    LoadFailed {
        /// The asset path or identifier.
        path: String,
        /// The reason for the failure.
        reason: String,
    },

    /// Failed to decode the asset data.
    #[error("Failed to decode asset '{path}': {reason}")]
    DecodeFailed {
        /// The asset path or identifier.
        path: String,
        /// The reason for the decode failure.
        reason: String,
    },

    /// The asset format is not supported.
    #[error("Unsupported asset format for '{path}': {format}")]
    UnsupportedFormat {
        /// The asset path or identifier.
        path: String,
        /// The unsupported format.
        format: String,
    },

    /// Invalid asset data.
    #[error("Invalid asset data for '{path}': {reason}")]
    InvalidData {
        /// The asset path or identifier.
        path: String,
        /// The reason the data is invalid.
        reason: String,
    },

    /// Network error occurred while fetching the asset.
    #[error("Network error for '{url}': {reason}")]
    NetworkError {
        /// The URL that failed.
        url: String,
        /// The reason for the network error.
        reason: String,
    },

    /// I/O error occurred.
    #[error("I/O error: {0}")]
    Io(String),

    /// Cache error.
    #[error("Cache error: {0}")]
    Cache(String),

    /// Bundle error.
    #[error("Bundle error: {0}")]
    Bundle(String),
}

impl From<std::io::Error> for AssetError {
    fn from(err: std::io::Error) -> Self {
        AssetError::Io(err.to_string())
    }
}

#[cfg(feature = "images")]
impl From<image::ImageError> for AssetError {
    fn from(err: image::ImageError) -> Self {
        AssetError::DecodeFailed {
            path: "unknown".to_string(),
            reason: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AssetError::NotFound {
            path: "test.png".to_string(),
        };
        assert_eq!(err.to_string(), "Asset not found: test.png");

        let err = AssetError::LoadFailed {
            path: "test.png".to_string(),
            reason: "file too large".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Failed to load asset 'test.png': file too large"
        );
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let asset_err: AssetError = io_err.into();
        assert!(matches!(asset_err, AssetError::Io(_)));
    }
}
