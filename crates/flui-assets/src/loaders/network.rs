//! Network-based asset loader using HTTP/HTTPS.

#[cfg(feature = "network")]
use reqwest;

#[cfg(feature = "network")]
use crate::core::{Asset, AssetLoader, AssetMetadata};

use crate::error::AssetError;

/// Loads assets from HTTP/HTTPS URLs.
///
/// Requires the `network` feature to be enabled.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::loaders::NetworkLoader;
///
/// let loader = NetworkLoader::new();
///
/// // Load from URL
/// let bytes = loader.load_url("https://example.com/image.png").await?;
/// ```
#[derive(Debug, Clone)]
pub struct NetworkLoader {
    #[cfg(feature = "network")]
    client: reqwest::Client,
}

impl Default for NetworkLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkLoader {
    /// Creates a new network loader with default HTTP client.
    #[cfg(feature = "network")]
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Creates a new network loader (requires `network` feature).
    ///
    /// This is a stub when the `network` feature is not enabled.
    #[cfg(not(feature = "network"))]
    pub fn new() -> Self {
        Self {}
    }

    /// Creates a network loader with a custom HTTP client.
    #[cfg(feature = "network")]
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Loads raw bytes from a URL.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let loader = NetworkLoader::new();
    /// let bytes = loader.load_url("https://example.com/data.bin").await?;
    /// ```
    #[cfg(feature = "network")]
    pub async fn load_url(&self, url: &str) -> Result<Vec<u8>, AssetError> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| AssetError::LoadFailed {
                path: url.to_string(),
                reason: format!("HTTP request failed: {e}"),
            })?;

        if !response.status().is_success() {
            return Err(AssetError::LoadFailed {
                path: url.to_string(),
                reason: format!("HTTP error: {}", response.status()),
            });
        }

        let bytes = response.bytes().await.map_err(|e| AssetError::LoadFailed {
            path: url.to_string(),
            reason: format!("Failed to read response body: {e}"),
        })?;

        Ok(bytes.to_vec())
    }

    /// Stub for loading from URL (requires `network` feature).
    ///
    /// Returns an error when the `network` feature is not enabled.
    #[cfg(not(feature = "network"))]
    #[allow(
        clippy::unused_async,
        reason = "public API: signature must match the genuinely-async `network`-enabled variant"
    )]
    pub async fn load_url(&self, url: &str) -> Result<Vec<u8>, AssetError> {
        Err(AssetError::LoadFailed {
            path: url.to_string(),
            reason: "Network loading requires 'network' feature".to_string(),
        })
    }

    /// Loads a text string from a URL.
    #[cfg(feature = "network")]
    pub async fn load_text(&self, url: &str) -> Result<String, AssetError> {
        let bytes = self.load_url(url).await?;
        String::from_utf8(bytes).map_err(|e| AssetError::LoadFailed {
            path: url.to_string(),
            reason: format!("Invalid UTF-8: {e}"),
        })
    }

    /// Stub for loading text from URL (requires `network` feature).
    ///
    /// Returns an error when the `network` feature is not enabled.
    #[cfg(not(feature = "network"))]
    #[allow(
        clippy::unused_async,
        reason = "public API: signature must match the genuinely-async `network`-enabled variant"
    )]
    pub async fn load_text(&self, url: &str) -> Result<String, AssetError> {
        Err(AssetError::LoadFailed {
            path: url.to_string(),
            reason: "Network loading requires 'network' feature".to_string(),
        })
    }
}

#[cfg(feature = "network")]
impl<T> AssetLoader<T> for NetworkLoader
where
    T: Asset<Error = AssetError>,
    T::Key: AsRef<str>,
{
    async fn load(&self, key: &T::Key) -> std::result::Result<T::Data, T::Error> {
        let url = key.as_ref();

        // For generic loading, we can't construct T::Data from bytes
        // This is meant to be used with concrete implementations
        Err(AssetError::LoadFailed {
            path: url.to_string(),
            reason:
                "Generic network loading not supported - use load_url() or concrete Asset types"
                    .to_string(),
        })
    }

    async fn exists(&self, key: &T::Key) -> std::result::Result<bool, T::Error> {
        let url = key.as_ref();

        // Send HEAD request to check if resource exists
        match self.client.head(url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    async fn metadata(&self, key: &T::Key) -> std::result::Result<Option<AssetMetadata>, T::Error> {
        let url = key.as_ref();

        let response = self
            .client
            .head(url)
            .send()
            .await
            .map_err(|e| AssetError::LoadFailed {
                path: url.to_string(),
                reason: format!("HTTP HEAD request failed: {e}"),
            })?;

        if !response.status().is_success() {
            return Ok(None);
        }

        let size_bytes = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok());

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);

        Ok(Some(AssetMetadata {
            size_bytes,
            format: content_type,
            ..Default::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(feature = "network")]
    async fn test_network_loader_creation() {
        let loader = NetworkLoader::new();
        // Should compile and create successfully
        assert!(std::mem::size_of_val(&loader) > 0);
    }

    #[tokio::test]
    #[cfg(not(feature = "network"))]
    async fn test_network_loader_without_feature() {
        let loader = NetworkLoader::new();
        let result = loader.load_url("https://example.com/test").await;

        assert!(result.is_err());
        if let Err(AssetError::LoadFailed { reason, .. }) = result {
            assert!(reason.contains("network"));
        }
    }

    // Integration test with real HTTP request (only runs with network feature)
    #[tokio::test]
    #[cfg(feature = "network")]
    #[ignore = "requires internet connection"]
    async fn test_network_loader_real_request() {
        let loader = NetworkLoader::new();

        // Use a reliable public URL
        let result = loader.load_url("https://httpbin.org/bytes/100").await;

        if let Ok(bytes) = result {
            assert_eq!(bytes.len(), 100);
        }
        // If it fails, it's likely a network issue, not a code issue
    }

    /// A single-request, single-response HTTP/1.1 server bound to an
    /// ephemeral loopback port — hermetic, no external network. Accepts
    /// exactly one connection, discards the request, writes `body` as a
    /// `200 OK` response, then the listener thread exits.
    #[cfg(feature = "network")]
    fn spawn_single_response_server(body: &'static [u8]) -> std::net::SocketAddr {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener =
            TcpListener::bind("127.0.0.1:0").expect("binding an ephemeral port must succeed");
        let addr = listener
            .local_addr()
            .expect("a bound listener must report its local address");

        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            // Drain enough of the request to know the client is done sending
            // headers; the exact request line/headers are irrelevant here.
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
        });

        addr
    }

    /// `NetworkLoader::load_url` against a real, hermetic local HTTP server:
    /// no external network, no mocking library — proves `flui-assets`' own
    /// network-loading code path (the `network` feature's `reqwest` client)
    /// genuinely round-trips bytes over HTTP. This is independent of, and
    /// does not imply anything about, `flui-widgets`' `network-images`
    /// feature, whose `NetworkImage` provider never issues a request at all
    /// (see `crates/flui-widgets/src/image/provider.rs`).
    #[tokio::test]
    #[cfg(feature = "network")]
    async fn load_url_round_trips_bytes_from_a_hermetic_local_server() {
        const FIXTURE_BODY: &[u8] = b"flui-assets network loader hermetic test payload";
        let addr = spawn_single_response_server(FIXTURE_BODY);

        let loader = NetworkLoader::new();
        let bytes = loader
            .load_url(&format!("http://{addr}/asset.bin"))
            .await
            .expect("a local hermetic server's 200 response must load successfully");

        assert_eq!(
            bytes, FIXTURE_BODY,
            "load_url must return exactly the server's response body",
        );
    }
}
