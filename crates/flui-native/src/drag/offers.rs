//! The drag feature's model: what a host's one data-transfer source keeps of
//! the offers it mints ([`OfferTable`]), the entries of a URI list
//! (`TransferUri`, where a host reads paths or URIs), and,
//! for the files that name representations by media type (the browser's
//! `DataTransfer` types and Android's `ClipDescription` speak media types
//! alike), what a media type offers and the payload its bytes carry.

use std::num::NonZeroU32;
use std::sync::Arc;

use flui_foundation::DataTransferId;
use flui_platform::data_transfer::RepresentationDescriptor;
#[cfg(any(target_os = "android", target_arch = "wasm32", test))]
use flui_platform::data_transfer::TransferFormat;
#[cfg(any(
    windows,
    target_os = "android",
    target_arch = "wasm32",
    all(
        feature = "winit-backend",
        any(target_os = "macos", target_os = "linux")
    ),
    test
))]
use flui_platform::data_transfer::{Mime, TransferPayload};

// ============================================================================
// The offer table: the single minting authority
// ============================================================================

/// One slot of the offer slab. The generation survives the offer: retiring
/// bumps it, so the next occupant of the slot mints a different id.
#[derive(Debug)]
struct OfferSlot {
    generation: NonZeroU32,
    representations: Option<Arc<[RepresentationDescriptor]>>,
}

/// Slab of live offers, each the representations it announced, with
/// generation-checked lookup. One per platform instance, private inside its
/// one `DataTransferSource`, behind the source's own state lock.
///
/// This is the **single minting authority** for [`DataTransferId`]s
/// (ADR-0038 §2): a generational check is only a defense within one table,
/// so a platform instance must never operate two of these.
#[derive(Debug, Default)]
pub(crate) struct OfferTable {
    slots: Vec<OfferSlot>,
    free: Vec<u32>,
}

impl OfferTable {
    /// Mint a live offer announcing `representations`, returning its
    /// generational id (`new_gen(slot, generation)`).
    pub(crate) fn mint(
        &mut self,
        representations: Arc<[RepresentationDescriptor]>,
    ) -> DataTransferId {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            debug_assert!(
                slot.representations.is_none(),
                "free list held an occupied slot"
            );
            slot.representations = Some(representations);
            return DataTransferId::new_gen(index, slot.generation);
        }

        let index =
            u32::try_from(self.slots.len()).expect("BUG: offer table exceeds u32::MAX slots");
        let generation = NonZeroU32::MIN;
        self.slots.push(OfferSlot {
            generation,
            representations: Some(representations),
        });
        DataTransferId::new_gen(index, generation)
    }

    /// The representations live offer `id` announced. Slot **and** generation
    /// must match, so a retired id never addresses the slot's next occupant.
    #[cfg(any(
        target_os = "android",
        target_arch = "wasm32",
        all(
            feature = "winit-backend",
            any(target_os = "windows", target_os = "macos", target_os = "linux")
        ),
        test
    ))]
    #[must_use]
    pub(crate) fn get(&self, id: DataTransferId) -> Option<&Arc<[RepresentationDescriptor]>> {
        let slot = self.slots.get(id.index() as usize)?;
        if slot.generation != id.generation() {
            return None;
        }
        slot.representations.as_ref()
    }

    /// Retire a live offer: free the slot and bump its generation so the id
    /// is permanently stale. Returns what the offer announced, or `None` if
    /// `id` was already stale (idempotent).
    ///
    /// A slot whose generation would overflow is never reused — it is
    /// dropped from circulation rather than allowed to recycle generation 1.
    pub(crate) fn retire(&mut self, id: DataTransferId) -> Option<Arc<[RepresentationDescriptor]>> {
        let slot_index = id.index() as usize;
        let slot = self.slots.get_mut(slot_index)?;
        if slot.generation != id.generation() || slot.representations.is_none() {
            return None;
        }
        let representations = slot.representations.take();
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(id.index());
        }
        // else: generation space exhausted for this slot — leave it out of
        // the free list forever rather than risk ABA reuse.
        representations
    }
}

// ============================================================================
// URI lists
// ============================================================================

/// The media type of a URI list (RFC 2483).
#[cfg(any(
    windows,
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "macos", target_os = "linux")
    ),
    test
))]
const URI_LIST: &str = "text/uri-list";

/// One entry of a `text/uri-list` as a host reads or writes it: a local path,
/// which travels as its `file:` URI (RFC 8089), or any other URI.
///
/// The OS hands a host paths (`CF_HDROP`, winit's dropped files) or URIs (a
/// clip's items); the application sees the list as the bytes of
/// `text/uri-list`, the one form every protocol shares.
#[cfg(any(
    windows,
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "macos", target_os = "linux")
    ),
    test
))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TransferUri {
    /// A local filesystem path.
    Path(std::path::PathBuf),
    /// Any other URI, unparsed.
    #[cfg_attr(
        not(any(windows, target_os = "android", test)),
        expect(
            dead_code,
            reason = "the winit host on macOS and Linux reads only paths"
        )
    )]
    Uri(String),
}

#[cfg(any(
    windows,
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "macos", target_os = "linux")
    ),
    test
))]
impl TransferUri {
    /// The payload a URI list of `uris` is delivered as: `text/uri-list`, one
    /// URI per CRLF-terminated line. A relative path, which no URI names, is
    /// left out.
    #[must_use]
    pub(crate) fn list_payload(uris: impl IntoIterator<Item = Self>) -> TransferPayload {
        let mut list = String::new();
        for uri in uris {
            let Some(uri) = uri.to_uri() else {
                tracing::debug!(
                    ?uri,
                    "a relative path has no file URI; left out of the list"
                );
                continue;
            };
            list.push_str(&uri);
            list.push_str("\r\n");
        }
        TransferPayload::Custom {
            mime: Mime::new(URI_LIST),
            bytes: Arc::from(list.into_bytes()),
        }
    }

    /// The entries of a URI-list payload, a `file:` URI of a local path read
    /// as that path; `None` for a payload of any other type, or one that is
    /// not UTF-8. Blank lines and `#` comments are skipped (RFC 2483).
    #[cfg(any(windows, target_os = "android", test))]
    #[must_use]
    pub(crate) fn list_of(payload: &TransferPayload) -> Option<Vec<Self>> {
        let TransferPayload::Custom { mime, bytes } = payload else {
            return None;
        };
        if mime.as_str().split(';').next() != Some(URI_LIST) {
            return None;
        }
        let list = std::str::from_utf8(bytes).ok()?;
        Some(
            list.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(|line| {
                    path_of_file_uri(line).map_or_else(|| Self::Uri(line.to_owned()), Self::Path)
                })
                .collect(),
        )
    }

    /// This entry as a URI, or `None` for a relative path.
    fn to_uri(&self) -> Option<String> {
        match self {
            Self::Uri(uri) => Some(uri.clone()),
            Self::Path(path) => file_uri(path),
        }
    }
}

/// Whether `bytes` start with a drive (`C:`), alone or before a separator.
#[cfg(any(
    windows,
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "macos", target_os = "linux")
    ),
    test
))]
fn starts_with_drive(bytes: &[u8]) -> bool {
    match bytes {
        [letter, b':'] => letter.is_ascii_alphabetic(),
        [letter, b':', separator, ..] => {
            letter.is_ascii_alphabetic() && matches!(separator, b'/' | b'\\')
        }
        _ => false,
    }
}

/// The `file:` URI of an absolute `path` (RFC 8089): a drive path `C:\a b` as
/// `file:///C:/a%20b`, a UNC path `\\host\share\a` as `file://host/share/a`, a
/// POSIX path `/a b` as `file:///a%20b`; `None` for a relative path. Every
/// byte but the unreserved ones, `/` and `:` is percent-encoded.
#[cfg(any(
    windows,
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "macos", target_os = "linux")
    ),
    test
))]
fn file_uri(path: &std::path::Path) -> Option<String> {
    let bytes = path.as_os_str().as_encoded_bytes();
    let (prefix, rest, windows) = if let Some(unc) = bytes.strip_prefix(br"\\") {
        ("file://", unc, true)
    } else if starts_with_drive(bytes) {
        ("file:///", bytes, true)
    } else if bytes.starts_with(b"/") {
        ("file://", bytes, false)
    } else {
        return None;
    };
    let mut uri = String::with_capacity(prefix.len() + rest.len());
    uri.push_str(prefix);
    for &byte in rest {
        match byte {
            b'\\' if windows => uri.push('/'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                uri.push(char::from(byte));
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                uri.push('%');
                uri.push(char::from(HEX[usize::from(byte >> 4)]));
                uri.push(char::from(HEX[usize::from(byte & 0xF)]));
            }
        }
    }
    Some(uri)
}

/// The local path a `file:` URI names, or `None` for any other URI, a `file:`
/// URI with no absolute path, a malformed percent escape or a path that is not
/// UTF-8. `file:///C:/a` reads as `C:\a`, `file://host/share/a` as
/// `\\host\share\a`, `file:///a` (and `file://localhost/a`) as `/a`.
#[cfg(any(windows, target_os = "android", test))]
fn path_of_file_uri(uri: &str) -> Option<std::path::PathBuf> {
    let scheme = uri.get(..5)?;
    if !scheme.eq_ignore_ascii_case("file:") {
        return None;
    }
    let rest = uri[5..].split(['?', '#']).next().unwrap_or_default();
    let (host, path) = match rest.strip_prefix("//") {
        Some(after) => after.split_at(after.find('/').unwrap_or(after.len())),
        None => ("", rest),
    };
    let decoded = String::from_utf8(percent_decoded(path)?).ok()?;
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        let host = String::from_utf8(percent_decoded(host)?).ok()?;
        return Some(format!(r"\\{host}{}", decoded.replace('/', "\\")).into());
    }
    let local = decoded.strip_prefix('/')?;
    Some(if starts_with_drive(local.as_bytes()) {
        local.replace('/', "\\").into()
    } else {
        decoded.into()
    })
}

/// `text` with its percent escapes decoded, or `None` for a malformed one.
#[cfg(any(windows, target_os = "android", test))]
fn percent_decoded(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    Some(out)
}

// ============================================================================
// Media types
// ============================================================================

/// What one media type offers, or `None` for a type FLUI does not announce
/// (the browser's `Files`, a browser's private types, names that are not
/// media types). FLUI's identity reads as itself: a custom type with its
/// parameters.
#[cfg(any(target_os = "android", target_arch = "wasm32", test))]
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
/// for `format`: the bytes of an image's container, as announced, and of the
/// type's essence otherwise (`application/octet-stream` for a format a file
/// never announces).
#[cfg(any(target_os = "android", target_arch = "wasm32", test))]
#[must_use]
pub(crate) fn payload_of_bytes(format: &TransferFormat, bytes: Vec<u8>) -> TransferPayload {
    let mime = match format {
        TransferFormat::Image { mime } => mime.clone(),
        TransferFormat::Custom { mime } => {
            Mime::new(mime.as_str().split(';').next().unwrap_or_default())
        }
        TransferFormat::Text | TransferFormat::Html | TransferFormat::UriList => {
            Mime::new("application/octet-stream")
        }
    };
    TransferPayload::Custom {
        mime,
        bytes: Arc::from(bytes),
    }
}

#[cfg(test)]
mod tests;
