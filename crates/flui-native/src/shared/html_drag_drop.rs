//! The decidable half of the web backend's HTML drag and drop (ADR-0038,
//! ADR-0211 §11): how `DataTransfer` types and effect names map to transfer
//! formats and actions, and what string a payload is stored as. Cfg-free
//! plain functions, so Linux CI pins them; `platforms::web::drag` moves the
//! values in and out of the DOM.
//!
//! The browser carries every representation as a string keyed by its
//! (lowercased) type, plus files. FLUI's session identity travels under its
//! full media type, parameters included, so a hover reads it from `types`
//! while the data itself is still out of reach.

use std::sync::Arc;

use flui_platform::data_transfer::{
    DropFeedback, Mime, RepresentationDescriptor, RepresentationIndex, TransferActions,
    TransferFormat, TransferImage, TransferPayload, TransferUri,
};
use flui_platform::drag::OsDragProvider;

/// The actions an `effectAllowed` value permits. `uninitialized` (a drag
/// from outside the page) and unknown values permit every action, as the
/// browser does for them.
#[must_use]
pub fn allowed_of(effect_allowed: &str) -> TransferActions {
    match effect_allowed {
        "none" => TransferActions::empty(),
        "copy" => TransferActions::COPY,
        "move" => TransferActions::MOVE,
        "link" => TransferActions::LINK,
        "copyMove" => TransferActions::COPY | TransferActions::MOVE,
        "copyLink" => TransferActions::COPY | TransferActions::LINK,
        "linkMove" => TransferActions::LINK | TransferActions::MOVE,
        _ => TransferActions::all(),
    }
}

/// The `effectAllowed` value that permits `actions`; `"none"` for none.
#[must_use]
pub fn effect_allowed_of(actions: TransferActions) -> &'static str {
    let (copy, moves, link) = (
        actions.contains(TransferActions::COPY),
        actions.contains(TransferActions::MOVE),
        actions.contains(TransferActions::LINK),
    );
    match (copy, moves, link) {
        (true, true, true) => "all",
        (true, true, false) => "copyMove",
        (true, false, true) => "copyLink",
        (false, true, true) => "linkMove",
        (true, false, false) => "copy",
        (false, true, false) => "move",
        (false, false, true) => "link",
        (false, false, false) => "none",
    }
}

/// The `dropEffect` a target answers with: the one action its cached
/// feedback accepts among those the drag permits (copy, then move, then
/// link), or `"none"`, which refuses the drop.
#[must_use]
pub fn drop_effect(feedback: DropFeedback, allowed: TransferActions) -> &'static str {
    match feedback.resolve(allowed) {
        Some(action) if action == TransferActions::COPY => "copy",
        Some(action) if action == TransferActions::MOVE => "move",
        Some(action) if action == TransferActions::LINK => "link",
        _ => "none",
    }
}

/// The action a `dropEffect` names; empty for `"none"` and unknown values.
#[must_use]
pub fn action_of(drop_effect: &str) -> TransferActions {
    match drop_effect {
        "copy" => TransferActions::COPY,
        "move" => TransferActions::MOVE,
        "link" => TransferActions::LINK,
        _ => TransferActions::empty(),
    }
}

/// The `DataTransfer` type of `Files`, which names the files of a drag, not
/// a string representation.
pub const FILES_TYPE: &str = "Files";

/// What one `DataTransfer` type offers, or `None` for a type FLUI does not
/// announce (`Files`, a browser's private types, names that are not media
/// types). FLUI's identity reads as itself: a custom type with its
/// parameters.
#[must_use]
pub fn format_of(dom_type: &str) -> Option<TransferFormat> {
    match dom_type {
        "text/plain" => Some(TransferFormat::Text),
        "text/html" => Some(TransferFormat::Html),
        "text/uri-list" => Some(TransferFormat::UriList),
        _ => {
            let essence = dom_type.split(';').next().unwrap_or_default();
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
            let mime = Mime::new(dom_type);
            Some(if kind == "image" {
                TransferFormat::Image { mime }
            } else {
                TransferFormat::Custom { mime }
            })
        }
    }
}

/// Where one announced representation comes from at the drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomSource {
    /// `getData` of this type.
    Data(String),
    /// The drag's file at this index of `DataTransfer.files`.
    File(usize),
}

/// The representations a hovering drag offers, in the browser's order: one
/// per string type FLUI announces (the identity with its parameters, its
/// declared length zero), then one per file, by its media type
/// (`application/octet-stream` when the browser has none).
#[must_use]
pub fn offered<'a>(
    types: impl IntoIterator<Item = &'a str>,
    file_types: impl IntoIterator<Item = &'a str>,
) -> Vec<(DomSource, RepresentationDescriptor)> {
    let mut offered: Vec<(DomSource, RepresentationDescriptor)> = Vec::new();
    for dom_type in types {
        let Some(format) = format_of(dom_type) else {
            continue;
        };
        if offered.iter().any(|(_, known)| known.format == format) {
            continue;
        }
        let declared_len = format.is_drag_identity().then_some(0);
        offered.push((
            DomSource::Data(dom_type.to_owned()),
            RepresentationDescriptor {
                format,
                declared_len,
            },
        ));
    }
    for (index, file_type) in file_types.into_iter().enumerate() {
        let mime = if file_type.is_empty() {
            "application/octet-stream"
        } else {
            file_type
        };
        let format = if mime.starts_with("image/") {
            TransferFormat::Image {
                mime: Mime::new(mime),
            }
        } else {
            TransferFormat::Custom {
                mime: Mime::new(mime),
            }
        };
        offered.push((
            DomSource::File(index),
            RepresentationDescriptor {
                format,
                declared_len: None,
            },
        ));
    }
    offered
}

/// The payload a `getData` string carries for `format`.
#[must_use]
pub fn payload_of(format: &TransferFormat, text: String) -> TransferPayload {
    match format {
        TransferFormat::Text => TransferPayload::Text(text),
        TransferFormat::Html => TransferPayload::Html(text),
        // RFC 2483: one URI per CRLF-separated line, `#` lines are comments.
        TransferFormat::UriList => TransferPayload::UriList(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(|line| TransferUri::Uri(line.to_owned()))
                .collect(),
        ),
        TransferFormat::Image { .. } | TransferFormat::Custom { .. } => {
            payload_of_bytes(format, text.into_bytes())
        }
    }
}

/// The payload a dropped file's bytes, or a `getData` string's, carry for
/// `format`: an image for an image format, as announced, and raw bytes of
/// the type's essence otherwise (`application/octet-stream` for a format a
/// file never announces).
#[must_use]
pub fn payload_of_bytes(format: &TransferFormat, bytes: Vec<u8>) -> TransferPayload {
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

/// A drag's provider panicked while it served a representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPanicked;

/// What a source stores at `dragstart`: the `DataTransfer` type and string of
/// each of `representations` that `provider` serves and the browser can
/// carry, in order. A representation the provider refuses is left out.
///
/// # Errors
///
/// [`ProviderPanicked`] when the provider panics: the panic is contained and
/// reported here, and nothing after it is asked for. The caller still owns
/// the session and ends it.
pub fn serve_stored(
    representations: &[RepresentationDescriptor],
    provider: &mut OsDragProvider,
) -> Result<Vec<(String, String)>, ProviderPanicked> {
    let mut out = Vec::new();
    for (index, descriptor) in representations.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            break;
        };
        let served = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            provider(RepresentationIndex(index))
        }));
        match served {
            Ok(Ok(payload)) => out.extend(stored(descriptor, &payload)),
            Ok(Err(error)) => {
                tracing::debug!(%error, index, "a representation could not be served");
            }
            Err(payload) => {
                // A payload whose destructor panics must not unwind from here.
                std::mem::forget(payload);
                super::panic_boundary::contain_owner_callback(|| {
                    tracing::error!("a drag's provider panicked");
                });
                return Err(ProviderPanicked);
            }
        }
    }
    Ok(out)
}

/// The `DataTransfer` type and string a source stores `payload` of
/// `descriptor` as at `dragstart`, or `None` when the browser cannot carry
/// it (bytes that are not UTF-8: `setData` takes strings).
#[must_use]
pub fn stored(
    descriptor: &RepresentationDescriptor,
    payload: &TransferPayload,
) -> Option<(String, String)> {
    let dom_type = match &descriptor.format {
        TransferFormat::Text => "text/plain".to_owned(),
        TransferFormat::Html => "text/html".to_owned(),
        TransferFormat::UriList => "text/uri-list".to_owned(),
        TransferFormat::Image { mime } | TransferFormat::Custom { mime } => {
            mime.as_str().to_owned()
        }
    };
    let text = match payload {
        TransferPayload::Text(text) | TransferPayload::Html(text) => text.clone(),
        TransferPayload::UriList(uris) => uris
            .iter()
            .map(|uri| match uri {
                TransferUri::Uri(uri) => uri.clone(),
                TransferUri::Path(path) => format!("file://{}", path.display()),
            })
            .collect::<Vec<_>>()
            .join("\r\n"),
        TransferPayload::Custom { bytes, .. } => std::str::from_utf8(bytes).ok()?.to_owned(),
        TransferPayload::Image(image) => std::str::from_utf8(&image.bytes).ok()?.to_owned(),
    };
    Some((dom_type, text))
}
