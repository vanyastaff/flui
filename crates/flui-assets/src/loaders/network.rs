//! Network-based asset loader using HTTP/HTTPS.

use reqwest;

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
    client: reqwest::Client,
}

impl Default for NetworkLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkLoader {
    /// Creates a new network loader with default HTTP client.
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Creates a network loader with a custom HTTP client.
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

    /// Loads a text string from a URL.
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
    /// The `load_url` call is wrapped in a bounded [`tokio::time::timeout`]:
    /// `NetworkLoader::new()` builds a `reqwest::Client` with no request
    /// timeout of its own, so a wedged exchange (a server that accepts but
    /// never writes) would otherwise hang this test — and CI — forever.
    #[tokio::test]
    async fn load_url_round_trips_bytes_from_a_hermetic_local_server() {
        use std::time::Duration;

        const FIXTURE_BODY: &[u8] = b"flui-assets network loader hermetic test payload";
        const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

        let addr = spawn_single_response_server(FIXTURE_BODY);
        let loader = NetworkLoader::new();

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
