//! What a drag that names its representations by media type offers, and the
//! payload their bytes carry: the browser's `DataTransfer` types and Android's
//! `ClipDescription` speak media types alike.

use std::sync::Arc;

use flui_platform::data_transfer::{Mime, TransferFormat, TransferImage, TransferPayload};

/// What one media type offers, or `None` for a type FLUI does not announce
/// (the browser's `Files`, a browser's private types, names that are not
/// media types). FLUI's identity reads as itself: a custom type with its
/// parameters.
#[must_use]
pub(crate) fn format_of(media_type: &str) -> Option<TransferFormat> {
    match media_type {
        "text/plain" => Some(TransferFormat::Text),
        "text/html" => Some(TransferFormat::Html),
        "text/uri-list" => Some(TransferFormat::UriList),
        _ => {
            let essence = media_type.split(';').next().unwrap_or_default();
            let (kind, subtype) = essence.split_once('/')?;
            let token = |part: &str| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&byte))
            };
            if !token(kind) || !token(subtype) || kind == "chromium" {
                return None;
            }
            let mime = Mime::new(media_type);
            Some(if kind == "image" {
                TransferFormat::Image { mime }
            } else {
                TransferFormat::Custom { mime }
            })
        }
    }
}

/// The payload a dropped file's bytes, or a representation's string, carry
/// for `format`: an image for an image format, as announced, and raw bytes of
/// the type's essence otherwise (`application/octet-stream` for a format a
/// file never announces).
#[must_use]
pub(crate) fn payload_of_bytes(format: &TransferFormat, bytes: Vec<u8>) -> TransferPayload {
    let bytes = Arc::from(bytes);
    match format {
        TransferFormat::Image { mime } => TransferPayload::Image(TransferImage {
            mime: mime.clone(),
            bytes,
        }),
        TransferFormat::Custom { mime } => TransferPayload::Custom {
            mime: Mime::new(mime.as_str().split(';').next().unwrap_or_default()),
            bytes,
        },
        TransferFormat::Text | TransferFormat::Html | TransferFormat::UriList => {
            TransferPayload::Custom {
                mime: Mime::new("application/octet-stream"),
                bytes,
            }
        }
    }
}
