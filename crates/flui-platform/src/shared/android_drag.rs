//! The decidable half of the Android backend's drag and drop (ADR-0247,
//! ADR-0211): how a `DragEvent`'s clip maps to an offer, what a drop
//! answers, what a FLUI drag puts in its `ClipData`, and how the source learns
//! the end. Cfg-free plain values, so Linux CI pins them;
//! `platforms::android::drag` moves them across JNI.
//!
//! Android delivers a drag to a view through `View.onDragEvent` on the Java
//! main thread, which needs every answer at once: the view keeps receiving
//! the drag only if it accepts `ACTION_DRAG_STARTED`, and the drop's result is
//! the return of `ACTION_DROP`. FLUI's target answers on its owner thread, so
//! the drop is decided from the feedback the target cached for the offer, as
//! the Win32 and web targets decide theirs. Until the drop the clip's data is
//! out of reach and only its `ClipDescription` is known, so an offer is minted
//! from the description's media types; the drop copies the text, HTML and
//! URIs out, and a URI's bytes are read when requested.

use std::sync::Arc;

use flui_platform_api::data_transfer::{
    DropFeedback, Mime, RepresentationDescriptor, TransferActions, TransferFormat, TransferPayload,
    TransferUri,
};
use flui_platform_api::drag::{IDENTITY_ESSENCE, OsDragEnd};

/// `DragEvent.ACTION_DRAG_STARTED`.
pub const ACTION_DRAG_STARTED: i32 = 1;
/// `DragEvent.ACTION_DRAG_LOCATION`.
pub const ACTION_DRAG_LOCATION: i32 = 2;
/// `DragEvent.ACTION_DROP`.
pub const ACTION_DROP: i32 = 3;
/// `DragEvent.ACTION_DRAG_ENDED`.
pub const ACTION_DRAG_ENDED: i32 = 4;
/// `DragEvent.ACTION_DRAG_ENTERED`.
pub const ACTION_DRAG_ENTERED: i32 = 5;
/// `DragEvent.ACTION_DRAG_EXITED`.
pub const ACTION_DRAG_EXITED: i32 = 6;

/// `View.DRAG_FLAG_GLOBAL`: the drag may leave the source's window.
pub const DRAG_FLAG_GLOBAL: i32 = 1 << 8;
/// `View.DRAG_FLAG_GLOBAL_URI_READ`: a target may read the drag's URIs.
pub const DRAG_FLAG_GLOBAL_URI_READ: i32 = 1;

/// `ClipDescription.MIMETYPE_TEXT_INTENT`, which FLUI cannot redeem.
pub const MIMETYPE_TEXT_INTENT: &str = "text/vnd.android.intent";

/// Where one announced representation comes from at the drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipSource {
    /// The items' text, joined.
    Text,
    /// The first item's HTML.
    Html,
    /// Every item's URI.
    UriList,
    /// The bytes behind the first URI, read when requested.
    UriBytes,
    /// FLUI's session identity: its media type says everything.
    Identity,
}

/// The representations a drag of `mime_types` offers, in the description's
/// order: text, HTML and a URI list as themselves, FLUI's identity with its
/// parameters (declared length zero), and any other media type as the bytes
/// of the clip's URI. Intents, duplicates and names that are not media types
/// are not announced.
#[must_use]
pub fn offered<'a>(
    mime_types: impl IntoIterator<Item = &'a str>,
) -> Vec<(ClipSource, RepresentationDescriptor)> {
    let mut offered: Vec<(ClipSource, RepresentationDescriptor)> = Vec::new();
    for mime in mime_types {
        if mime == MIMETYPE_TEXT_INTENT {
            continue;
        }
        let Some(format) = super::html_drag_drop::format_of(mime) else {
            continue;
        };
        if offered.iter().any(|(_, known)| known.format == format) {
            continue;
        }
        let identity = super::html_drag_drop::is_identity(&format);
        let source = match &format {
            TransferFormat::Text => ClipSource::Text,
            TransferFormat::Html => ClipSource::Html,
            TransferFormat::UriList => ClipSource::UriList,
            _ if identity => ClipSource::Identity,
            _ => ClipSource::UriBytes,
        };
        offered.push((
            source,
            RepresentationDescriptor {
                format,
                declared_len: identity.then_some(0),
            },
        ));
    }
    offered
}

/// What a drop's clip carried, copied out while the drop event ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DroppedClip {
    /// Every item's text, joined; `None` when no item had any.
    pub text: Option<String>,
    /// The first item's HTML.
    pub html: Option<String>,
    /// Every item's URI.
    pub uris: Vec<String>,
}

/// One announced representation after the drop.
#[derive(Debug, Clone)]
pub enum Held {
    /// Served from memory.
    Payload(TransferPayload),
    /// The bytes behind a URI, read when requested.
    Uri(HeldUri),
    /// Announced, but the clip did not carry it.
    Missing,
}

/// A dropped content URI whose bytes are read when requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldUri {
    /// The content URI.
    pub uri: String,
    /// The format the offer announced for its bytes.
    pub format: TransferFormat,
}

impl HeldUri {
    /// The payload the URI's `bytes` resolve as: an image for an image
    /// offer, as announced, and raw bytes of the type's essence otherwise.
    #[must_use]
    pub fn payload(&self, bytes: Vec<u8>) -> TransferPayload {
        super::html_drag_drop::payload_of_bytes(&self.format, bytes)
    }
}

/// What `source` of `descriptor` holds once `clip` dropped.
#[must_use]
pub fn held(
    source: &ClipSource,
    descriptor: &RepresentationDescriptor,
    clip: &DroppedClip,
) -> Held {
    match source {
        ClipSource::Text => clip.text.clone().map_or(Held::Missing, |text| {
            Held::Payload(TransferPayload::Text(text))
        }),
        ClipSource::Html => clip.html.clone().map_or(Held::Missing, |html| {
            Held::Payload(TransferPayload::Html(html))
        }),
        ClipSource::UriList if clip.uris.is_empty() => Held::Missing,
        ClipSource::UriList => Held::Payload(TransferPayload::UriList(
            clip.uris.iter().cloned().map(TransferUri::Uri).collect(),
        )),
        ClipSource::Identity => Held::Payload(TransferPayload::Custom {
            mime: match &descriptor.format {
                TransferFormat::Custom { mime } => mime.clone(),
                _ => Mime::new(IDENTITY_ESSENCE),
            },
            bytes: Arc::from(Vec::new()),
        }),
        ClipSource::UriBytes => clip.uris.first().map_or(Held::Missing, |uri| {
            Held::Uri(HeldUri {
                uri: uri.clone(),
                format: descriptor.format.clone(),
            })
        }),
    }
}

/// The actions a drag over FLUI permits. Android has no drop effects: a drag
/// from another app is a copy. A drag this process started (its local state
/// is set) permits what its source advertised.
#[must_use]
pub fn incoming_allowed(own: Option<TransferActions>) -> TransferActions {
    own.unwrap_or(TransferActions::COPY)
}

/// The action a drop takes: the one the target's cached feedback accepts
/// among those the drag permits (copy, then move, then link), or `None`,
/// which refuses the drop.
#[must_use]
pub fn drop_action(feedback: DropFeedback, allowed: TransferActions) -> Option<TransferActions> {
    super::ole_drag_drop::answered_action(feedback, allowed)
}

/// What a FLUI drag puts in its `ClipData`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipParts {
    /// The description's media types, the session identity among them.
    pub mime_types: Vec<String>,
    /// The text item's text.
    pub text: Option<String>,
    /// The text item's HTML.
    pub html: Option<String>,
    /// One item per URI.
    pub uris: Vec<String>,
}

impl ClipParts {
    /// The `startDragAndDrop` flags: always global, since a sibling window
    /// of the runtime is another window too, and letting targets read the
    /// URIs when there are any.
    #[must_use]
    pub fn flags(&self) -> i32 {
        if self.uris.is_empty() {
            DRAG_FLAG_GLOBAL
        } else {
            DRAG_FLAG_GLOBAL | DRAG_FLAG_GLOBAL_URI_READ
        }
    }
}

/// The clip of a drag offering `representations`, each served (or not) as
/// `payloads` says, in the same order.
///
/// Text and HTML share the first item; each URI is an item of its own. A
/// `file:` URI or a path is left out: Android refuses to let one leave the
/// process through a clip (`FileUriExposedException` from API 24). Bytes have
/// no place in a clip without a content provider, so a representation served
/// as bytes is left out too, except FLUI's identity, which travels as its
/// media type alone.
#[must_use]
pub fn clip_parts(
    representations: &[RepresentationDescriptor],
    payloads: &[Option<TransferPayload>],
) -> ClipParts {
    let mut parts = ClipParts::default();
    for (descriptor, payload) in representations.iter().zip(payloads) {
        let identity = super::html_drag_drop::is_identity(&descriptor.format);
        let carried = match payload {
            _ if identity => true,
            Some(TransferPayload::Text(text)) => {
                parts.text = Some(text.clone());
                true
            }
            Some(TransferPayload::Html(html)) => {
                parts.html = Some(html.clone());
                true
            }
            Some(TransferPayload::UriList(uris)) => {
                let before = parts.uris.len();
                parts.uris.extend(uris.iter().filter_map(|uri| match uri {
                    TransferUri::Uri(uri) if !uri.starts_with("file:") => Some(uri.clone()),
                    _ => None,
                }));
                parts.uris.len() > before
            }
            _ => false,
        };
        if carried {
            parts.mime_types.push(mime_type(&descriptor.format));
        }
    }
    parts
}

fn mime_type(format: &TransferFormat) -> String {
    match format {
        TransferFormat::Text => "text/plain".to_owned(),
        TransferFormat::Html => "text/html".to_owned(),
        TransferFormat::UriList => "text/uri-list".to_owned(),
        TransferFormat::Image { mime } | TransferFormat::Custom { mime } => {
            mime.as_str().to_owned()
        }
    }
}

/// How a FLUI drag ended, from `ACTION_DRAG_ENDED`'s result: refused or
/// cancelled when no target took it; the action FLUI's own target took when
/// it landed in this process; otherwise copy when the drag allowed it, since
/// another app's drop is a copy, else the one action it allowed first.
#[must_use]
pub fn source_end(
    result: bool,
    local_drop: Option<TransferActions>,
    allowed: TransferActions,
) -> OsDragEnd {
    if !result {
        return OsDragEnd::Cancelled;
    }
    if let Some(action) = local_drop {
        return OsDragEnd::Dropped(action);
    }
    let action = [
        TransferActions::COPY,
        TransferActions::MOVE,
        TransferActions::LINK,
    ]
    .into_iter()
    .find(|action| allowed.contains(*action));
    action.map_or(OsDragEnd::Cancelled, OsDragEnd::Dropped)
}
