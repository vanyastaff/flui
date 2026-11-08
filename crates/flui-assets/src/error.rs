//! Error types for decoded-image operations.

use thiserror::Error;

/// Result type for decoded-image operations.
pub type Result<T> = std::result::Result<T, AssetError>;

/// Errors produced while loading, fetching or decoding an image.
#[derive(Error, Debug, Clone)]
pub enum AssetError {
    /// The requested source was not found.
    #[error("Asset not found: {path}")]
    NotFound {
        /// The path or identifier that was not found.
        path: String,
    },

    /// The source could not be read or its image could not be decoded.
    #[error("Failed to load asset '{path}': {reason}")]
    LoadFailed {
        /// The asset path or identifier.
        path: String,
        /// The reason for the failure.
        reason: String,
    },

    /// The decoder failed after the source was read.
    #[error("Failed to decode asset '{path}': {reason}")]
    DecodeFailed {
        /// The asset path or identifier.
        path: String,
        /// The reason for the decode failure.
        reason: String,
    },

    /// A network image was requested without an HTTP client.
    #[error("No HTTP client to fetch '{url}'")]
    HttpUnavailable {
        /// The URL that was not fetched.
        url: String,
    },

    /// The HTTP request for a network image failed.
    #[error("Fetching '{url}' failed: {source}")]
    Http {
        /// The URL that failed.
        url: String,
        /// Why the request failed.
        source: flui_platform::http::HttpError,
    },

    /// The server answered with a status other than 2xx.
    #[error("Fetching '{url}' answered HTTP {status}")]
    HttpStatus {
        /// The URL that was fetched.
        url: String,
        /// The status the server answered.
        status: flui_platform::http::StatusCode,
    },

    /// The background lanes refused a load because they are shutting down.
    #[error("The background lanes refused to load '{path}': {reason}")]
    LanesRefused {
        /// The asset path or URL.
        path: String,
        /// Why the lanes refused it.
        reason: flui_platform::executor::SpawnError,
    },
}
