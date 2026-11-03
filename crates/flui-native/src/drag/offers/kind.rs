//! What a drag's types offer and what their bytes carry, where types are
//! named as X selection targets and Wayland data offers name them: media
//! types, beside the X text forms (`UTF8_STRING`, `STRING`, `TEXT`) that
//! toolkits offer on both. The X11 and Wayland drags share it.

use flui_platform::data_transfer::{Mime, TransferError, TransferFormat, TransferPayload};

use crate::clipboard::text::TextForm;
use crate::drag::offers::{format_of, payload_of_bytes};

/// What a representation's bytes are read as.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    /// Text in one of its forms.
    Text(TextForm),
    /// HTML markup, UTF-8 or UTF-16 behind its byte-order mark.
    Html,
    /// The bytes of a media type, as the source wrote them.
    Bytes(TransferFormat),
}

impl Kind {
    /// The format the representation announces.
    pub(crate) fn format(&self) -> TransferFormat {
        match self {
            Self::Text(_) => TransferFormat::Text,
            Self::Html => TransferFormat::Html,
            Self::Bytes(format) => format.clone(),
        }
    }

    /// The payload `bytes` of this kind carry.
    pub(crate) fn payload(&self, mut bytes: Vec<u8>) -> TransferPayload {
        match self {
            Self::Text(form) => TransferPayload::Text(form.decode(bytes)),
            Self::Html => TransferPayload::Html(html(bytes)),
            Self::Bytes(TransferFormat::UriList) => {
                // A list some sources end with a NUL.
                if bytes.last() == Some(&0) {
                    bytes.pop();
                }
                TransferPayload::Custom {
                    mime: Mime::new("text/uri-list"),
                    bytes: bytes.into(),
                }
            }
            Self::Bytes(format) => payload_of_bytes(format, bytes),
        }
    }
}

impl Kind {
    /// How many bytes the payload [`payload`](Self::payload) makes of
    /// `bytes` holds, measured without making it: a fetch refuses text
    /// past its limit before ill-formed or wider text grows as it
    /// decodes.
    pub(crate) fn decoded_len(&self, bytes: &[u8]) -> usize {
        match self {
            Self::Text(form) => form.decoded_len(bytes),
            Self::Html => {
                let utf16 = |pairs: &[u8], unit: fn([u8; 2]) -> u16| {
                    let units = pairs.as_chunks::<2>().0.iter().map(|pair| unit(*pair));
                    let (len, last) = char::decode_utf16(units)
                        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
                        .fold((0, None), |(len, _), char| {
                            (len + char.len_utf8(), Some(char))
                        });
                    // A trailing NUL is dropped.
                    len - usize::from(last == Some('\0'))
                };
                match bytes {
                    [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
                    [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
                    _ => TextForm::Utf8String.decoded_len(bytes),
                }
            }
            // A URI list's trailing NUL is dropped.
            Self::Bytes(TransferFormat::UriList) => bytes.strip_suffix(&[0]).unwrap_or(bytes).len(),
            Self::Bytes(_) => bytes.len(),
        }
    }
}

/// HTML markup: UTF-16 behind a byte-order mark, as Firefox writes it,
/// UTF-8 otherwise, with any ill-formed sequence replaced and a
/// trailing NUL dropped.
fn html(bytes: Vec<u8>) -> String {
    let utf16 = |pairs: &[u8], unit: fn([u8; 2]) -> u16| {
        let units = pairs.as_chunks::<2>().0.iter().map(|pair| unit(*pair));
        char::decode_utf16(units)
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<String>()
    };
    let mut text = match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ => match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
        },
    };
    if text.ends_with('\0') {
        text.pop();
    }
    text
}

/// The representations a drag's `types` (each a key, an atom or the name itself, and its name)
/// offer, each with the type it is converted to, in the source's order:
/// text once, in the form a reader prefers, where the first text form
/// stood; HTML; every other media type as named. A type that names no
/// media type (`TARGETS`, `COMPOUND_TEXT`, a toolkit's private name) is
/// left out, as is a second one of a format.
pub(crate) fn representations<K: Copy>(types: &[(K, &str)]) -> Vec<(K, Kind)> {
    let text = TextForm::preferred(types.iter().map(|(_, name)| *name)).and_then(|form| {
        let (atom, _) = types
            .iter()
            .find(|(_, name)| TextForm::named(name) == Some(form))?;
        Some((*atom, Kind::Text(form)))
    });
    let mut offered: Vec<(K, Kind)> = Vec::new();
    for (atom, name) in types {
        let kind = if TextForm::named(name).is_some() {
            match &text {
                Some(text) => text.clone(),
                None => continue,
            }
        } else if name.split(';').next().map(str::trim) == Some("text/html") {
            (*atom, Kind::Html)
        } else {
            match format_of(name) {
                Some(format) => (*atom, Kind::Bytes(format)),
                None => continue,
            }
        };
        if offered
            .iter()
            .all(|(_, held)| held.format() != kind.1.format())
        {
            offered.push(kind);
        }
    }
    offered
}

/// The type names a representation of `format` is offered under, the most
/// precise first: text in UTF-8 under the names every toolkit reads, every
/// other format under its media type.
pub(crate) fn names_of(format: &TransferFormat) -> Vec<String> {
    match format {
        TransferFormat::Text => vec![
            "UTF8_STRING".to_owned(),
            "text/plain;charset=utf-8".to_owned(),
            "text/plain".to_owned(),
        ],
        TransferFormat::Html => vec!["text/html".to_owned()],
        TransferFormat::UriList => vec!["text/uri-list".to_owned()],
        TransferFormat::Image { mime } | TransferFormat::Custom { mime } => {
            vec![mime.as_str().to_owned()]
        }
    }
}

/// The bytes `payload` is written as: text and HTML in UTF-8, any other
/// payload as it is.
pub(crate) fn bytes_of(payload: TransferPayload) -> Vec<u8> {
    match payload {
        TransferPayload::Text(text) | TransferPayload::Html(text) => text.into_bytes(),
        TransferPayload::Custom { bytes, .. } => bytes.to_vec(),
    }
}

/// The error a payload of `len` bytes past `limit` is refused with.
pub(crate) fn too_large(len: usize, limit: u64) -> Option<TransferError> {
    let actual = u64::try_from(len).unwrap_or(u64::MAX);
    (actual > limit).then_some(TransferError::TooLarge { actual, limit })
}

#[cfg(test)]
mod tests;
