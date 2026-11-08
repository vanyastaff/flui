//! Length-only decoding between the hosts' text encodings and Rust UTF-8.
//!
//! Every scanner stops at the clipboard's byte limit without allocating the
//! decoded text. The subsequent conversion therefore cannot allocate a string
//! that the read will refuse.

use flui_platform::ClipboardError;

use super::admit_utf8;

/// Add one decoded scalar's width, refusing the whole read at the limit.
fn add_decoded(total: usize, width: usize) -> Result<usize, ClipboardError> {
    let total = total.checked_add(width).ok_or(ClipboardError::TooLarge)?;
    admit_utf8(total)?;
    Ok(total)
}

/// How many UTF-8 bytes `units` decode to, each unpaired surrogate as U+FFFD,
/// as `String::from_utf16_lossy` and wasm-bindgen write it.
#[cfg(any(windows, target_arch = "wasm32", test))]
pub(crate) fn decoded_utf16_len(
    units: impl IntoIterator<Item = u16>,
) -> Result<usize, ClipboardError> {
    char::decode_utf16(units).try_fold(0, |total, unit| {
        let width = unit.map_or(char::REPLACEMENT_CHARACTER.len_utf8(), char::len_utf8);
        add_decoded(total, width)
    })
}

/// How many UTF-8 bytes JNI's modified UTF-8 `bytes` decode to through
/// `JNIStr::to_str`. That converter accepts ordinary UTF-8 directly, otherwise
/// tries modified UTF-8, and falls back to ordinary lossy UTF-8 for the whole
/// buffer if either strict decoding fails. Measure all three without the
/// `String` either successful branch would allocate.
#[cfg(any(target_os = "android", test))]
pub(crate) fn decoded_mutf8_len(bytes: &[u8]) -> Result<usize, ClipboardError> {
    if std::str::from_utf8(bytes).is_ok() {
        return add_decoded(0, bytes.len());
    }

    match strict_mutf8_len(bytes)? {
        Some(len) => Ok(len),
        None => lossy_utf8_len(bytes),
    }
}

/// Strict modified UTF-8, or `None` when JNI would use its lossy UTF-8
/// fallback. Reaching the limit is already conclusive: for every valid strict
/// prefix, lossy ordinary UTF-8 is at least as long as this decoded length.
#[cfg(any(target_os = "android", test))]
fn strict_mutf8_len(bytes: &[u8]) -> Result<Option<usize>, ClipboardError> {
    let mut index = 0;
    let mut decoded = 0;
    while index < bytes.len() {
        let (width, decoded_width) = match bytes[index] {
            0x01..=0x7f => (1, 1),
            0xc0 if bytes.get(index + 1) == Some(&0x80) => (2, 1),
            0xc2..=0xdf if valid_utf8_sequence(bytes, index, 2) => (2, 2),
            0xed if matches!(
                bytes.get(index..index + 3),
                Some([0xed, 0xa0..=0xaf, 0x80..=0xbf])
            ) =>
            {
                if matches!(
                    bytes.get(index + 3..index + 6),
                    Some([0xed, 0xb0..=0xbf, 0x80..=0xbf])
                ) {
                    (6, 4)
                } else {
                    return Ok(None);
                }
            }
            0xe0..=0xef if valid_utf8_sequence(bytes, index, 3) => (3, 3),
            _ => return Ok(None),
        };
        decoded = add_decoded(decoded, decoded_width)?;
        index += width;
    }
    Ok(Some(decoded))
}

#[cfg(any(target_os = "android", test))]
fn valid_utf8_sequence(bytes: &[u8], index: usize, width: usize) -> bool {
    bytes
        .get(index..index + width)
        .is_some_and(|sequence| std::str::from_utf8(sequence).is_ok())
}

/// `String::from_utf8_lossy`'s output length, without its output allocation.
#[cfg(any(target_os = "android", test))]
fn lossy_utf8_len(mut bytes: &[u8]) -> Result<usize, ClipboardError> {
    let mut decoded = 0;
    loop {
        match std::str::from_utf8(bytes) {
            Ok(valid) => return add_decoded(decoded, valid.len()),
            Err(error) => {
                let valid = error.valid_up_to();
                decoded = add_decoded(decoded, valid)?;
                decoded = add_decoded(decoded, char::REPLACEMENT_CHARACTER.len_utf8())?;
                match error.error_len() {
                    Some(invalid) => bytes = &bytes[valid + invalid..],
                    None => return Ok(decoded),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::ClipboardError;

    use super::super::MAX_TEXT;
    use super::{decoded_mutf8_len, decoded_utf16_len};

    #[test]
    fn utf16_length_matches_the_lossy_decode() {
        let cases: &[&[u16]] = &[
            &[0x0041, 0x00e9, 0x20ac, 0xd83d, 0xde00],
            &[0xd800],
            &[0xdc00],
            &[0xd800, 0x0041, 0xdc00],
        ];
        for &units in cases {
            assert_eq!(
                decoded_utf16_len(units.iter().copied()),
                Ok(String::from_utf16_lossy(units).len())
            );
        }
    }

    #[test]
    fn utf16_length_is_measured_at_the_byte_boundary() {
        let at_limit = vec![0x00e9; MAX_TEXT / 2];
        assert_eq!(decoded_utf16_len(at_limit.into_iter()), Ok(MAX_TEXT));
        let past_limit = vec![0x00e9; MAX_TEXT / 2 + 1];
        assert_eq!(
            decoded_utf16_len(past_limit.into_iter()),
            Err(ClipboardError::TooLarge)
        );
    }

    #[test]
    fn mutf8_length_matches_all_jni_conversion_branches() {
        let cases: &[(&[u8], usize)] = &[
            (b"plain", 5),
            (&[0xc0, 0x80], 1),
            // A raw NUL is accepted by the initial ordinary-UTF-8 fast path,
            // but selects the fallback when another sequence requires decoding.
            (&[0x00, 0xc0, 0x80], 7),
            (&[0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80], 4),
            ("😀".as_bytes(), 4),
            (&[0xed, 0xa0, 0xbd], 9),
            (&[0xe2, 0x82], 3),
            (&[0xff], 3),
            // One unpaired surrogate makes JNI reinterpret the whole buffer
            // as ordinary lossy UTF-8, including the earlier NUL and pair.
            (
                &[
                    0xc0, 0x80, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80, 0xed, 0xa0, 0xbd,
                ],
                33,
            ),
            // A four-byte ordinary UTF-8 scalar likewise selects the fallback
            // when the buffer also contains a modified-UTF-8 NUL.
            (&[0xf0, 0x9f, 0x98, 0x80, 0xc0, 0x80], 10),
        ];
        for &(bytes, expected) in cases {
            assert_eq!(decoded_mutf8_len(bytes), Ok(expected), "{bytes:02x?}");
        }
    }
}
