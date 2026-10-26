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

#[cfg(test)]
mod tests {
    use flui_platform::data_transfer::{Mime, TransferFormat, TransferPayload};

    use super::{format_of, payload_of_bytes};

    /// A media type offers its transfer format; a type FLUI does not announce
    /// (the browser's `Files`, Chromium's private types, a name that is no
    /// media type) offers none.
    #[test]
    fn a_media_type_offers_its_transfer_format() {
        let image = |mime: &str| {
            Some(TransferFormat::Image {
                mime: Mime::new(mime),
            })
        };
        let custom = |mime: &str| {
            Some(TransferFormat::Custom {
                mime: Mime::new(mime),
            })
        };
        let rows = [
            ("text/plain", Some(TransferFormat::Text)),
            ("text/html", Some(TransferFormat::Html)),
            ("text/uri-list", Some(TransferFormat::UriList)),
            ("image/png", image("image/png")),
            ("application/pdf", custom("application/pdf")),
            (
                "application/x-flui-drag; session=7",
                custom("application/x-flui-drag; session=7"),
            ),
            ("Files", None),
            ("chromium/x-renderer-taint", None),
            ("text/", None),
            ("/plain", None),
            ("text plain", None),
            ("text/pla in", None),
        ];
        for (media_type, format) in rows {
            assert_eq!(format_of(media_type), format, "{media_type:?}");
        }
    }

    /// Bytes carry the payload of their format: an image as it was announced,
    /// any other format the raw bytes of its type's essence, and a format a
    /// file never announces `application/octet-stream`.
    #[test]
    fn bytes_carry_the_payload_of_their_format() {
        let carried = |format: &TransferFormat| match payload_of_bytes(format, b"bytes".to_vec()) {
            TransferPayload::Image(image) => (
                "image",
                image.mime.as_str().to_owned(),
                image.bytes.to_vec(),
            ),
            TransferPayload::Custom { mime, bytes } => {
                ("custom", mime.as_str().to_owned(), bytes.to_vec())
            }
            other => panic!("bytes carried as {other:?}"),
        };
        let rows = [
            (
                TransferFormat::Image {
                    mime: Mime::new("image/png"),
                },
                ("image", "image/png"),
            ),
            (
                TransferFormat::Custom {
                    mime: Mime::new("application/x-flui-drag; session=7"),
                },
                ("custom", "application/x-flui-drag"),
            ),
            (TransferFormat::Text, ("custom", "application/octet-stream")),
            (TransferFormat::Html, ("custom", "application/octet-stream")),
            (
                TransferFormat::UriList,
                ("custom", "application/octet-stream"),
            ),
        ];
        for (format, (kind, mime)) in rows {
            assert_eq!(
                carried(&format),
                (kind, mime.to_owned(), b"bytes".to_vec()),
                "{format:?}"
            );
        }
    }
}
