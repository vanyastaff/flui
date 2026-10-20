//! Network-based asset loader using HTTP/HTTPS.

use reqwest;

use std::time::Duration;

use crate::error::AssetError;

/// How long [`NetworkLoader::new`]'s client waits for a connection.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long [`NetworkLoader::new`]'s client waits for the next bytes of a
/// response.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The largest response body [`NetworkLoader::load_url`] accepts: 256 MiB.
/// A larger one fails instead of growing without bound.
pub const MAX_BODY_BYTES: usize = 256 * 1024 * 1024;

fn read_failed(url: &str, error: &reqwest::Error) -> AssetError {
    AssetError::LoadFailed {
        path: url.to_string(),
        reason: format!("Failed to read response body: {error}"),
    }
}

/// Loads assets from HTTP/HTTPS URLs.
///
/// Requires the `network` feature to be enabled.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_assets::loaders::NetworkLoader;
///
/// let loader = NetworkLoader::new()?;
///
/// // Load from URL
/// let bytes = loader.load_url("https://example.com/image.png").await?;
/// ```
#[derive(Debug, Clone)]
pub struct NetworkLoader {
    client: reqwest::Client,
}

impl NetworkLoader {
    /// Creates a new network loader with default HTTP client.
    ///
    /// Reuse this loader or its clones to share HTTP connections. The default
    /// client gives up on a connection attempt after [`CONNECT_TIMEOUT`] and on
    /// a response that sends nothing for [`READ_TIMEOUT`], so a stalled server
    /// cannot hold a request open forever; use [`Self::with_client`] to
    /// configure other deadlines.
    ///
    /// # Errors
    ///
    /// Returns an error if TLS or resolver initialization fails.
    pub fn new() -> reqwest::Result<Self> {
        reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .build()
            .map(Self::with_client)
    }

    /// Creates a network loader with a custom HTTP client.
    ///
    /// A used client's pooled connections can depend on the runtime that first
    /// drove them. Keep those runtimes driven when sharing this loader across
    /// runtimes. Registry configuration accepts a fresh client builder instead.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Loads raw bytes from a URL.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let loader = NetworkLoader::new()?;
    /// let bytes = loader.load_url("https://example.com/data.bin").await?;
    /// ```
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

        let too_large = || AssetError::LoadFailed {
            path: url.to_string(),
            reason: format!("response body is larger than {MAX_BODY_BYTES} bytes"),
        };
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BODY_BYTES as u64)
        {
            return Err(too_large());
        }
        let mut response = response;
        let first = response.chunk().await.map_err(|e| read_failed(url, &e))?;
        let Some(first) = first else {
            return Ok(Vec::new());
        };
        if first.len() > MAX_BODY_BYTES {
            return Err(too_large());
        }
        // A body that arrives in one chunk is handed over without a copy.
        let Some(second) = response.chunk().await.map_err(|e| read_failed(url, &e))? else {
            return Ok(first.into());
        };
        // Every size is checked before anything is allocated or copied for it:
        // `first.len() <= MAX_BODY_BYTES` and each later check keeps
        // `body.len() <= MAX_BODY_BYTES`, so the subtractions cannot wrap.
        if second.len() > MAX_BODY_BYTES - first.len() {
            return Err(too_large());
        }
        let mut body = Vec::with_capacity(first.len() + second.len());
        body.extend_from_slice(&first);
        body.extend_from_slice(&second);
        while let Some(chunk) = response.chunk().await.map_err(|e| read_failed(url, &e))? {
            if chunk.len() > MAX_BODY_BYTES - body.len() {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// Loads strict UTF-8 text from a URL, preserving any UTF-8 BOM.
    ///
    /// Unlike [`reqwest::Response::text`], rejects invalid UTF-8 instead of
    /// replacing malformed bytes, and does not apply Content-Type charset decoding.
    pub async fn load_text(&self, url: &str) -> Result<String, AssetError> {
        let bytes = self.load_url(url).await?;
        String::from_utf8(bytes).map_err(|e| AssetError::LoadFailed {
            path: url.to_string(),
            reason: format!("Invalid UTF-8: {e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A single-request, single-response HTTP/1.1 server bound to an
    /// ephemeral loopback port — hermetic, no external network. Accepts
    /// exactly one connection, discards the request, writes `body` as a
    /// `200 OK` response, then the listener thread exits.
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
    /// genuinely round-trips bytes over HTTP. Widget image decoding and
    /// presentation are covered by their own consumer tests.
    ///
    /// The `load_url` call is wrapped in a bounded [`tokio::time::timeout`]
    /// shorter than the client's own read timeout, so a wedged exchange (a
    /// server that accepts but never writes) fails this test promptly.
    #[tokio::test]
    async fn load_url_round_trips_bytes_from_a_hermetic_local_server() {
        use std::time::Duration;

        const FIXTURE_BODY: &[u8] = b"flui-assets network loader hermetic test payload";
        const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

        let addr = spawn_single_response_server(FIXTURE_BODY);
        let loader = NetworkLoader::new().expect("default HTTP client initializes");

        let bytes = tokio::time::timeout(
            REQUEST_TIMEOUT,
            loader.load_url(&format!("http://{addr}/asset.bin")),
        )
        .await
        .expect("the hermetic local server must respond within the timeout, not hang")
        .expect("a local hermetic server's 200 response must load successfully");

        assert_eq!(
            bytes, FIXTURE_BODY,
            "load_url must return exactly the server's response body",
        );
    }
}
