//! [`PageAssets`]: a Web package's assets, fetched beside its page.

use flui_platform::assets::{AssetFuture, AssetKey, AssetLoadError, AssetSource};
use flui_platform::http::{Http, HttpError, HttpRequest, StatusCode};

use super::page_url::{asset_url, page_directory};

/// Reads asset `key` from the URL `flui_assets/<key>` relative to the page's
/// directory, each segment percent-encoded, through the page's `fetch`
/// (ADR-0384 §3). A `404` is [`NotFound`](AssetLoadError::NotFound); the
/// limit bounds the body as an HTTP request's does.
#[derive(Debug)]
pub struct PageAssets {
    /// The page's directory, ending in `/`.
    base: String,
    http: Http,
}

impl PageAssets {
    /// The assets beside this page: `None` where there is no document to
    /// name a base URL (a worker).
    #[must_use]
    pub fn of_this_page() -> Option<Self> {
        let base = web_sys::window()?.document()?.base_uri().ok().flatten()?;
        Some(Self::new(
            &base,
            Http::new(std::sync::Arc::new(crate::http::FetchClient::new())),
        ))
    }

    /// The assets beside the page at `page_url`, fetched with `http`.
    #[must_use]
    pub fn new(page_url: &str, http: Http) -> Self {
        Self {
            base: page_directory(page_url).to_owned(),
            http,
        }
    }
}

impl AssetSource for PageAssets {
    fn load(&self, key: &AssetKey, limit: u64) -> AssetFuture<Vec<u8>> {
        let sent = self
            .http
            .send(HttpRequest::get(asset_url(&self.base, key)).body_limit(limit));
        let key = key.clone();
        Box::pin(async move {
            let failed = |error: HttpError| match error {
                HttpError::TooLarge { limit } => AssetLoadError::TooLarge {
                    key: key.clone(),
                    limit,
                },
                other => AssetLoadError::Io {
                    key: key.clone(),
                    source: std::io::Error::other(other),
                },
            };
            let response = sent.await.map_err(failed)?;
            let status = response.status();
            if status == StatusCode::NOT_FOUND {
                return Err(AssetLoadError::NotFound { key });
            }
            if !status.is_success() {
                return Err(AssetLoadError::Io {
                    key: key.clone(),
                    source: std::io::Error::other(format!("the page answered {status}")),
                });
            }
            response.into_body().into_bytes().await.map_err(failed)
        })
    }
}
