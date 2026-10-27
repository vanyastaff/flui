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
use flui_platform::data_transfer::{Mime, TransferFormat, TransferPayload};

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

#[cfg(any(
    windows,
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "macos", target_os = "linux")
    ),
    test
))]
pub(crate) use uri_list::{PathStyle, TransferUri};

/// The entries of a `text/uri-list` and the `file:` URIs of local paths
/// (RFC 2483, RFC 8089), as the hosts that read paths or URIs write and read
/// them.
#[cfg(any(
    windows,
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "macos", target_os = "linux")
    ),
    test
))]
mod uri_list {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use flui_platform::data_transfer::{Mime, TransferPayload};

    /// The media type of a URI list.
    const URI_LIST: &str = "text/uri-list";

    /// How a host's paths are spelled. A drive (`C:\`), a UNC share
    /// (`\\host\share`) and a verbatim prefix (`\\?\`) are Windows paths; a
    /// POSIX path is rooted at `/` and may hold a backslash in a name.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum PathStyle {
        /// Win32 paths (`CF_HDROP`, a winit host on Windows).
        #[cfg_attr(
            not(any(windows, test)),
            expect(dead_code, reason = "only a Windows host spells Win32 paths")
        )]
        Windows,
        /// POSIX paths (Android, a winit host on macOS and Linux).
        #[cfg_attr(
            all(windows, not(test)),
            expect(dead_code, reason = "a Windows host spells only Win32 paths")
        )]
        Posix,
    }

    impl PathStyle {
        /// The style of the paths this host's OS hands out.
        #[cfg(all(feature = "winit-backend", target_os = "windows"))]
        pub(crate) const HOST: Self = Self::Windows;

        /// The style of the paths this host's OS hands out.
        #[cfg(all(
            feature = "winit-backend",
            any(target_os = "macos", target_os = "linux")
        ))]
        pub(crate) const HOST: Self = Self::Posix;
    }

    /// One entry of a `text/uri-list` as a host reads or writes it: a local
    /// path, which travels as its `file:` URI, or any other URI.
    ///
    /// The OS hands a host paths (`CF_HDROP`, winit's dropped files) or URIs (a
    /// clip's items); the application sees the list as the bytes of
    /// `text/uri-list`, the one form every protocol shares.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum TransferUri {
        /// A local filesystem path.
        Path(PathBuf),
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

    impl TransferUri {
        /// The payload a URI list of `uris` is delivered as: `text/uri-list`,
        /// one URI per CRLF-terminated line, each path spelled in `style`. A
        /// path with no `file:` URI (relative, of another style, or not
        /// Unicode, which no reader would take back) is left out.
        #[must_use]
        pub(crate) fn list_payload(
            uris: impl IntoIterator<Item = Self>,
            style: PathStyle,
        ) -> TransferPayload {
            let mut list = String::new();
            for uri in uris {
                let Some(uri) = uri.to_uri(style) else {
                    tracing::debug!(
                        ?uri,
                        ?style,
                        "a path with no file URI is left out of the list"
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

        /// The entries of a URI-list payload, a `file:` URI of a local path
        /// read as that path in `style`; `None` for a payload of any other
        /// type, or one that is not UTF-8. Blank lines and `#` comments are
        /// skipped.
        #[cfg(any(windows, target_os = "android", test))]
        #[must_use]
        pub(crate) fn list_of(payload: &TransferPayload, style: PathStyle) -> Option<Vec<Self>> {
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
                        path_of_file_uri(line, style)
                            .map_or_else(|| Self::Uri(line.to_owned()), Self::Path)
                    })
                    .collect(),
            )
        }

        /// This entry as a URI, or `None` for a path with no `file:` URI.
        fn to_uri(&self, style: PathStyle) -> Option<String> {
            match self {
                Self::Uri(uri) => Some(uri.clone()),
                Self::Path(path) => file_uri(path, style),
            }
        }
    }

    /// Whether `text` starts with a drive (`C:`), alone or before a separator.
    fn starts_with_drive(text: &str) -> bool {
        match text.as_bytes() {
            [letter, b':'] => letter.is_ascii_alphabetic(),
            [letter, b':', separator, ..] => {
                letter.is_ascii_alphabetic() && matches!(separator, b'/' | b'\\')
            }
            _ => false,
        }
    }

    /// The `file:` URI of an absolute `path` in `style`:
    ///
    /// - Windows: a drive path `C:\a b` as `file:///C:/a%20b`, a UNC path
    ///   `\\host\share\a` as `file://host/share/a`, a verbatim `\\?\C:\a` or
    ///   `\\?\UNC\host\share\a` as the path it spells, a path rooted with no
    ///   drive (`\a`) as `file:///a`;
    /// - POSIX: `/a b` as `file:///a%20b`, a backslash in a name escaped.
    ///
    /// `None` for a relative path, a Windows path that names no drive or share
    /// (`\\?\Volume{…}`), and a path that is not Unicode (an unpaired
    /// surrogate, bytes that are not UTF-8): no reader takes those back, so
    /// none is written. Every byte but the unreserved ones, `/` and `:` is
    /// percent-encoded.
    fn file_uri(path: &Path, style: PathStyle) -> Option<String> {
        let text = path.to_str()?;
        let (prefix, rest) = match style {
            PathStyle::Windows => {
                if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
                    ("file://", unc)
                } else if let Some(verbatim) = text.strip_prefix(r"\\?\") {
                    starts_with_drive(verbatim).then_some(("file:///", verbatim))?
                } else if let Some(unc) = text.strip_prefix(r"\\") {
                    ("file://", unc)
                } else if starts_with_drive(text) {
                    ("file:///", text)
                } else if text.starts_with(['\\', '/']) {
                    ("file://", text)
                } else {
                    return None;
                }
            }
            PathStyle::Posix => (text.starts_with('/').then_some("file://")?, text),
        };
        let mut uri = String::with_capacity(prefix.len() + rest.len());
        uri.push_str(prefix);
        for byte in rest.bytes() {
            match byte {
                b'\\' if style == PathStyle::Windows => uri.push('/'),
                b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'.'
                | b'_'
                | b'~'
                | b'/'
                | b':' => {
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

    /// The local path a `file:` URI names in `style`, or `None` for any other
    /// URI, a `file:` URI with no absolute path, a malformed percent escape, a
    /// path that is not UTF-8, and, in the POSIX style, a remote host.
    /// Windows: `file:///C:/a` reads as `C:\a`, `file://host/share/a` as
    /// `\\host\share\a`, `file:///a` as `\a`. POSIX: `file:///a` (and
    /// `file://localhost/a`) reads as `/a`.
    #[cfg(any(windows, target_os = "android", test))]
    fn path_of_file_uri(uri: &str, style: PathStyle) -> Option<PathBuf> {
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
        let remote = !host.is_empty() && !host.eq_ignore_ascii_case("localhost");
        let local = decoded.strip_prefix('/');
        match style {
            PathStyle::Windows if remote => {
                let host = String::from_utf8(percent_decoded(host)?).ok()?;
                Some(format!(r"\\{host}{}", decoded.replace('/', "\\")).into())
            }
            PathStyle::Windows => {
                let local = local?;
                Some(if starts_with_drive(local) {
                    local.replace('/', "\\").into()
                } else {
                    decoded.replace('/', "\\").into()
                })
            }
            PathStyle::Posix if remote => None,
            PathStyle::Posix => decoded.starts_with('/').then(|| decoded.into()),
        }
    }

    /// `text` with its percent escapes decoded, or `None` for a malformed one:
    /// a `%` takes exactly two hexadecimal digits.
    #[cfg(any(windows, target_os = "android", test))]
    fn percent_decoded(text: &str) -> Option<Vec<u8>> {
        let hex = |byte: u8| char::from(byte).to_digit(16);
        let bytes = text.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while let Some(&byte) = bytes.get(index) {
            if byte == b'%' {
                let high = hex(*bytes.get(index + 1)?)?;
                let low = hex(*bytes.get(index + 2)?)?;
                out.push(u8::try_from(high * 16 + low).expect("BUG: two hex digits fit a byte"));
                index += 3;
            } else {
                out.push(byte);
                index += 1;
            }
        }
        Some(out)
    }
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
