//! The decidable half of the Win32 OLE drag-and-drop backend (ADR-0038,
//! ADR-0211): which clipboard format carries each transfer format, how a
//! payload is laid out in its `HGLOBAL`, how `DROPEFFECT` bits map to
//! [`TransferActions`], and when the drop source ends the modal loop. Plain
//! functions over bytes and integers, so every host checks them; the COM
//! objects of the OLE half (`ole`) only move bytes in and out of global
//! memory. How often that loop asks for frames is the owner's
//! (`owner::modal_pace`).

#![forbid(unsafe_code)]

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use flui_platform::data_transfer::{
    Mime, RepresentationDescriptor, TransferActions, TransferError, TransferFormat, TransferImage,
    TransferPayload, TransferUri,
};
use flui_platform::drag::IDENTITY_ESSENCE;
use flui_platform::pointer::PointerKind;

/// `DROPEFFECT_COPY`.
pub const EFFECT_COPY: u32 = 1;
/// `DROPEFFECT_MOVE`.
pub const EFFECT_MOVE: u32 = 2;
/// `DROPEFFECT_LINK`.
pub const EFFECT_LINK: u32 = 4;

/// The `DROPEFFECT` bits that permit `actions`.
#[must_use]
pub fn effect_bits(actions: TransferActions) -> u32 {
    let mut bits = 0;
    if actions.contains(TransferActions::COPY) {
        bits |= EFFECT_COPY;
    }
    if actions.contains(TransferActions::MOVE) {
        bits |= EFFECT_MOVE;
    }
    if actions.contains(TransferActions::LINK) {
        bits |= EFFECT_LINK;
    }
    bits
}

/// The actions `DROPEFFECT` bits permit; `DROPEFFECT_SCROLL` and unknown
/// bits are ignored.
#[must_use]
pub fn actions_of(effect: u32) -> TransferActions {
    let mut actions = TransferActions::empty();
    if effect & EFFECT_COPY != 0 {
        actions |= TransferActions::COPY;
    }
    if effect & EFFECT_MOVE != 0 {
        actions |= TransferActions::MOVE;
    }
    if effect & EFFECT_LINK != 0 {
        actions |= TransferActions::LINK;
    }
    actions
}

/// The registered name browsers and Office use for PNG images.
pub const PNG_FORMAT_NAME: &str = "PNG";

/// The clipboard format one representation travels as.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NativeFormat {
    /// `CF_UNICODETEXT`: UTF-16 with a terminating NUL.
    UnicodeText,
    /// CF_HTML: UTF-8 markup behind its offset header.
    Html,
    /// `CF_HDROP`: a `DROPFILES` header and wide paths.
    FileDrop,
    /// A format registered by name: a MIME type, [`PNG_FORMAT_NAME`] or
    /// FLUI's identity format, whose bytes are the payload itself.
    Named(String),
}

/// The clipboard format `format` travels as. The session identity travels
/// under its bare media type, [`IDENTITY_ESSENCE`]: Win32 cannot register a
/// format per session, so the identity's parameters ride in the data.
#[must_use]
pub fn native_format(format: &TransferFormat) -> NativeFormat {
    match format {
        TransferFormat::Text => NativeFormat::UnicodeText,
        TransferFormat::Html => NativeFormat::Html,
        TransferFormat::UriList => NativeFormat::FileDrop,
        TransferFormat::Image { mime } if mime.as_str() == "image/png" => {
            NativeFormat::Named(PNG_FORMAT_NAME.to_owned())
        }
        TransferFormat::Custom { mime } if is_identity(mime.as_str()) => {
            NativeFormat::Named(IDENTITY_ESSENCE.to_owned())
        }
        TransferFormat::Image { mime } | TransferFormat::Custom { mime } => {
            NativeFormat::Named(mime.as_str().to_owned())
        }
    }
}

/// Whether `mime` names FLUI's private session identity.
fn is_identity(mime: &str) -> bool {
    mime.split(';').next() == Some(IDENTITY_ESSENCE)
}

/// The transfer format a foreign clipboard format reads as, or `None` for a
/// format FLUI does not announce (`CF_TEXT` beside `CF_UNICODETEXT`, shell
/// formats, names that are not media types) or for the identity format,
/// which [`identity_descriptor`] reads.
#[must_use]
pub fn transfer_format(native: &NativeFormat) -> Option<TransferFormat> {
    match native {
        NativeFormat::UnicodeText => Some(TransferFormat::Text),
        NativeFormat::Html => Some(TransferFormat::Html),
        NativeFormat::FileDrop => Some(TransferFormat::UriList),
        NativeFormat::Named(name) if name == PNG_FORMAT_NAME => Some(TransferFormat::Image {
            mime: Mime::new("image/png"),
        }),
        NativeFormat::Named(name) if is_identity(name) => None,
        NativeFormat::Named(name) => {
            let essence = name.split(';').next().unwrap_or_default();
            let (kind, subtype) = essence.split_once('/')?;
            let token = |part: &str| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&byte))
            };
            if !token(kind) || !token(subtype) {
                return None;
            }
            let mime = Mime::new(name);
            Some(if kind.eq_ignore_ascii_case("image") {
                TransferFormat::Image { mime }
            } else {
                TransferFormat::Custom { mime }
            })
        }
    }
}

/// The identity descriptor the identity format's bytes carry, when they are
/// the full media type of a FLUI session identity; anything else reads as a
/// foreign drag.
#[must_use]
pub fn identity_descriptor(bytes: &[u8]) -> Option<RepresentationDescriptor> {
    let text = std::str::from_utf8(bytes).ok()?.trim_end_matches('\0');
    is_identity(text).then(|| RepresentationDescriptor {
        format: TransferFormat::Custom {
            mime: Mime::new(text),
        },
        declared_len: Some(0),
    })
}

/// What a foreign data object offers, in its own order: one descriptor per
/// transfer format (the first clipboard format reading as it wins), with the
/// clipboard format to fetch it by, then the identity when it carries one.
#[must_use]
pub fn offer_representations(
    formats: impl IntoIterator<Item = NativeFormat>,
    identity: Option<RepresentationDescriptor>,
) -> Vec<(NativeFormat, RepresentationDescriptor)> {
    let mut offered: Vec<(NativeFormat, RepresentationDescriptor)> = Vec::new();
    for native in formats {
        let Some(format) = transfer_format(&native) else {
            continue;
        };
        if offered
            .iter()
            .any(|(_, descriptor)| descriptor.format == format)
        {
            continue;
        }
        offered.push((
            native,
            RepresentationDescriptor {
                format,
                declared_len: None,
            },
        ));
    }
    if let Some(identity) = identity {
        offered.push((NativeFormat::Named(IDENTITY_ESSENCE.to_owned()), identity));
    }
    offered
}

/// The bytes `payload` is served as in `native`, or `None` when the payload
/// does not fit the format (a URI list with no local path in `CF_HDROP`).
#[must_use]
pub fn encode(native: &NativeFormat, payload: &TransferPayload) -> Option<Vec<u8>> {
    match (native, payload) {
        (NativeFormat::UnicodeText, TransferPayload::Text(text)) => Some(encode_text(text)),
        (NativeFormat::Html, TransferPayload::Html(markup)) => Some(encode_html(markup)),
        (NativeFormat::FileDrop, TransferPayload::UriList(uris)) => {
            let paths: Vec<&std::path::Path> = uris
                .iter()
                .filter_map(|uri| match uri {
                    TransferUri::Path(path) => Some(path.as_path()),
                    TransferUri::Uri(_) => None,
                })
                .collect();
            (!paths.is_empty()).then(|| encode_file_drop(&paths))
        }
        (NativeFormat::Named(_), TransferPayload::Custom { bytes, .. }) => Some(bytes.to_vec()),
        (NativeFormat::Named(_), TransferPayload::Image(image)) => Some(image.bytes.to_vec()),
        _ => None,
    }
}

/// The payload `bytes`, fetched in `native`, carry for `format`.
///
/// # Errors
///
/// [`TransferError::Decode`] when the bytes are not well-formed for the
/// clipboard format.
pub fn decode(
    native: &NativeFormat,
    format: &TransferFormat,
    bytes: &[u8],
) -> Result<TransferPayload, TransferError> {
    let failed = || TransferError::Decode {
        format: format.clone(),
    };
    match (native, format) {
        (NativeFormat::UnicodeText, _) => decode_text(bytes).map(TransferPayload::Text),
        (NativeFormat::Html, _) => decode_html(bytes).map(TransferPayload::Html),
        (NativeFormat::FileDrop, _) => decode_file_drop(bytes).map(|paths| {
            TransferPayload::UriList(paths.into_iter().map(TransferUri::Path).collect())
        }),
        // The identity is read from the descriptor; its payload is empty, as
        // the source runtime serves it.
        (NativeFormat::Named(name), _) if is_identity(name) => Ok(TransferPayload::Custom {
            mime: Mime::new(IDENTITY_ESSENCE),
            bytes: Arc::from(Vec::new()),
        }),
        (NativeFormat::Named(_), TransferFormat::Image { mime }) => {
            Ok(TransferPayload::Image(TransferImage {
                mime: mime.clone(),
                bytes: Arc::from(bytes),
            }))
        }
        (NativeFormat::Named(_), TransferFormat::Custom { mime }) => Ok(TransferPayload::Custom {
            mime: Mime::new(mime.as_str().split(';').next().unwrap_or_default()),
            bytes: Arc::from(bytes),
        }),
        (NativeFormat::Named(_), _) => Err(()),
    }
    .map_err(|()| failed())
}

/// `CF_UNICODETEXT`: UTF-16LE with a terminating NUL.
#[must_use]
pub fn encode_text(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn utf16_units(bytes: &[u8]) -> impl Iterator<Item = u16> + '_ {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
}

/// `CF_UNICODETEXT` up to its first NUL, or the whole allocation without
/// one (another process may omit it; the allocation bounds the read).
fn decode_text(bytes: &[u8]) -> Result<String, ()> {
    let units: Vec<u16> = utf16_units(bytes).take_while(|unit| *unit != 0).collect();
    String::from_utf16(&units).map_err(|_| ())
}

const HTML_PREFIX: &str = "<html><body>\r\n<!--StartFragment-->";
const HTML_SUFFIX: &str = "<!--EndFragment-->\r\n</body></html>";

/// CF_HTML: the offset header, then the markup as the fragment of a
/// document.
#[must_use]
pub fn encode_html(markup: &str) -> Vec<u8> {
    let header_len = header(0, 0, 0, 0).len();
    let start_html = header_len;
    let start_fragment = start_html + HTML_PREFIX.len();
    let end_fragment = start_fragment + markup.len();
    let end_html = end_fragment + HTML_SUFFIX.len();
    let mut out = header(start_html, end_html, start_fragment, end_fragment).into_bytes();
    out.extend_from_slice(HTML_PREFIX.as_bytes());
    out.extend_from_slice(markup.as_bytes());
    out.extend_from_slice(HTML_SUFFIX.as_bytes());
    out
}

fn header(
    start_html: usize,
    end_html: usize,
    start_fragment: usize,
    end_fragment: usize,
) -> String {
    format!(
        "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\n\
         StartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n"
    )
}

/// The fragment of a CF_HTML document, or its whole HTML span when the
/// fragment offsets are absent or out of range.
fn decode_html(bytes: &[u8]) -> Result<String, ()> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    let bytes = &bytes[..end];
    // The header is ASCII and short; the markup after it may be anything.
    let header = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
    let offset = |key: &str| -> Option<usize> {
        header
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
            .and_then(|value| value.trim().parse().ok())
    };
    let span = |start: Option<usize>, end: Option<usize>| match (start, end) {
        (Some(start), Some(end)) if start <= end && end <= bytes.len() => Some(&bytes[start..end]),
        _ => None,
    };
    let chosen = span(offset("StartFragment"), offset("EndFragment"))
        .or_else(|| span(offset("StartHTML"), offset("EndHTML")))
        .ok_or(())?;
    String::from_utf8(chosen.to_vec()).map_err(|_| ())
}

/// The size of a `DROPFILES` header.
const DROPFILES_LEN: usize = 20;

/// `CF_HDROP`: a `DROPFILES` header naming wide paths, then each path with a
/// NUL and a final NUL.
#[must_use]
pub fn encode_file_drop(paths: &[&std::path::Path]) -> Vec<u8> {
    let mut out = Vec::with_capacity(DROPFILES_LEN);
    out.extend_from_slice(&u32::try_from(DROPFILES_LEN).unwrap_or(20).to_le_bytes());
    out.extend_from_slice(&0_i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0_i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0_i32.to_le_bytes()); // fNC
    out.extend_from_slice(&1_i32.to_le_bytes()); // fWide
    for path in paths {
        for unit in path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
        {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
    out.extend_from_slice(&0_u16.to_le_bytes());
    out
}

/// The paths of a `CF_HDROP` allocation, wide or ANSI, bounded by it.
fn decode_file_drop(bytes: &[u8]) -> Result<Vec<PathBuf>, ()> {
    let field = |at: usize| -> Option<u32> {
        bytes
            .get(at..at + 4)
            .map(|raw| u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    };
    let start = usize::try_from(field(0).ok_or(())?).map_err(|_| ())?;
    let wide = field(16).ok_or(())? != 0;
    let list = bytes.get(start..).ok_or(())?;
    let mut paths = Vec::new();
    if wide {
        let units: Vec<u16> = utf16_units(list).collect();
        for name in units.split(|unit| *unit == 0) {
            if name.is_empty() {
                break;
            }
            paths.push(PathBuf::from(String::from_utf16(name).map_err(|_| ())?));
        }
    } else {
        for name in list.split(|byte| *byte == 0) {
            if name.is_empty() {
                break;
            }
            paths.push(PathBuf::from(String::from_utf8_lossy(name).into_owned()));
        }
    }
    Ok(paths)
}

/// `MK_LBUTTON`, `MK_RBUTTON`, `MK_MBUTTON`, `MK_XBUTTON1` and `MK_XBUTTON2`.
pub const MOUSE_BUTTONS: u32 = 0x01 | 0x02 | 0x10 | 0x20 | 0x40;

/// What the drop source tells the OLE loop (`IDropSource::QueryContinueDrag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragContinuation {
    /// Keep dragging (`S_OK`).
    Continue,
    /// Drop on the target under the pointer (`DRAGDROP_S_DROP`).
    Drop,
    /// End without a drop (`DRAGDROP_S_CANCEL`).
    Cancel,
}

/// Whether a drag that began with the mouse buttons `held_at_start` goes on,
/// given Escape and the key state `keys` (`MK_*` bits): Escape cancels;
/// releasing the buttons that began it drops; pressing another button
/// cancels, as Windows' own sources do. A drag no button began cannot be
/// released, so it is cancelled rather than dropped wherever the cursor is.
#[must_use]
pub const fn continue_drag(escape: bool, keys: u32, held_at_start: u32) -> DragContinuation {
    let began = held_at_start & MOUSE_BUTTONS;
    let held = keys & MOUSE_BUTTONS;
    if escape || began == 0 || held & !began != 0 {
        DragContinuation::Cancel
    } else if held & began == 0 {
        DragContinuation::Drop
    } else {
        DragContinuation::Continue
    }
}

/// The mouse buttons (`MK_*`) whose release ends an OS session `DoDragDrop`
/// would run for a drag of a `kind` contact, given the buttons `held` in the
/// thread's input state as it starts; `None` when OLE's loop cannot carry it.
///
/// That loop follows the mouse alone. A pen or touch contact a window takes
/// as `WM_POINTER` without `DefWindowProc` is never promoted to mouse input,
/// so it sets no mouse button in the key state the loop reads; with no button
/// held the loop could neither follow the contact nor see it lift, and the
/// drag stays in its window instead (ADR-0211 §11).
#[must_use]
pub fn os_loop_buttons(kind: Option<PointerKind>, held: u32) -> Option<u32> {
    if matches!(kind, Some(PointerKind::Touch | PointerKind::Pen { .. })) {
        return None;
    }
    Some(held & MOUSE_BUTTONS).filter(|buttons| *buttons != 0)
}
