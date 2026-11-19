//! [`ImageLoadError`]: why an image load ended without an image.

use std::sync::Arc;

use flui_painting::paint::ImageDecodeError;
use flui_platform::executor::SpawnError;
use flui_platform::http::{HttpError, StatusCode};

use crate::image_cache::ImageSource;

/// Why the load of one [`ImageSource`] ended without an image.
///
/// Every request waiting on one load receives a clone of the same error. A
/// failure is never kept: the next request for the source loads it again.
#[derive(Debug, Clone)]
pub struct ImageLoadError {
    image: ImageSource,
    kind: ImageLoadErrorKind,
}

impl ImageLoadError {
    pub(crate) fn new(image: ImageSource, kind: ImageLoadErrorKind) -> Self {
        Self { image, kind }
    }

    /// The source whose load failed.
    #[must_use]
    pub fn image(&self) -> &ImageSource {
        &self.image
    }

    /// Why it failed.
    #[must_use]
    pub fn kind(&self) -> &ImageLoadErrorKind {
        &self.kind
    }
}

impl std::fmt::Display for ImageLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "loading the image `{}` failed: {}",
            self.image, self.kind
        )
    }
}

impl std::error::Error for ImageLoadError {}

/// Why an image load failed, without the source it failed for.
///
/// The message of each kind includes the error it carries, so the kind has no
/// [`source`](std::error::Error::source) of its own.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum ImageLoadErrorKind {
    /// No file exists at the asset's path.
    #[error("no file exists at this path")]
    NotFound,
    /// The asset's path names something other than a regular file (a
    /// directory, a FIFO, a device), which is never opened.
    #[error("the path is not a regular file")]
    NotAFile,
    /// The encoded image is larger than a load reads, and was not read
    /// whole.
    #[error("the encoded image is larger than {limit} bytes")]
    TooLarge {
        /// The most encoded bytes a load reads
        /// ([`MAX_ENCODED_IMAGE_BYTES`](flui_painting::paint::MAX_ENCODED_IMAGE_BYTES)).
        limit: u64,
    },
    /// Reading the file failed.
    #[error("reading the file failed: {0}")]
    Io(Arc<std::io::Error>),
    /// The bytes did not decode into an image.
    #[error("{0}")]
    Decode(ImageDecodeError),
    /// The HTTP request failed.
    #[error("the request failed: {0}")]
    Http(HttpError),
    /// The server answered a status other than 2xx.
    #[error("the server answered HTTP {0}")]
    HttpStatus(StatusCode),
    /// The background lanes this request was given are shutting down and
    /// took no work.
    #[error("the background lanes refused the load: {0}")]
    LanesRefused(SpawnError),
    /// The read, the request or the decode panicked. Loading the source
    /// again would fail the same way, so the load is not retried.
    #[error("the load panicked{}", panic_suffix(.message.as_deref()))]
    Panicked {
        /// The panic's message, when it was a string.
        message: Option<Arc<str>>,
    },
    /// The lanes kept dropping the load unfinished, and this request stopped
    /// taking it over.
    #[error("the load was dropped unfinished {handovers} times")]
    Abandoned {
        /// How many times the load was taken over before this request gave up.
        handovers: u8,
    },
}

impl ImageLoadErrorKind {
    /// The kind a failed file operation answers.
    pub(crate) fn from_io(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            _ => Self::Io(Arc::new(error)),
        }
    }

    /// The kind a caught panic answers; the payload is discarded under
    /// containment once its message is read (ADR-0281).
    pub(crate) fn panicked(payload: Box<dyn std::any::Any + Send>) -> Self {
        let message = flui_foundation::panic::payload_text(payload.as_ref()).map(Arc::<str>::from);
        flui_foundation::panic::discard_payload(payload);
        Self::Panicked { message }
    }
}

/// `": <message>"` for a panic that had one.
fn panic_suffix(message: Option<&str>) -> String {
    message.map_or_else(String::new, |text| format!(": {text}"))
}
