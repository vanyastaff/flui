//! A selection's text as the Linux desktops exchange it: the forms an owner
//! offers it in, the one a reader asks for, and the bytes of each.
//!
//! Wayland's data offers name MIME types, X11's selections name targets; the
//! toolkits of both offer text under the same few names, the MIME types and
//! the X11 target names alike, so one table serves both hosts. Pure: the
//! hosts move the bytes.

#![forbid(unsafe_code)]

/// A form of text a selection is offered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextForm {
    /// `text/plain;charset=utf-8`: UTF-8.
    PlainUtf8,
    /// `UTF8_STRING`, X11's UTF-8 target, which Wayland toolkits offer too.
    Utf8String,
    /// `text/plain`: no charset named, read as UTF-8 as every toolkit writes
    /// it.
    Plain,
    /// `STRING`: ISO 8859-1, X11's oldest text target.
    Latin1,
}

impl TextForm {
    /// What an owner offers its text as, the reader's first choice first.
    /// `TEXT` is left out: it names no encoding, and an owner answers it in
    /// whichever it likes.
    pub(crate) const OFFERED: [Self; 4] =
        [Self::PlainUtf8, Self::Utf8String, Self::Plain, Self::Latin1];

    /// The name the form is offered under: a MIME type on Wayland, an atom's
    /// name on X11.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::PlainUtf8 => "text/plain;charset=utf-8",
            Self::Utf8String => "UTF8_STRING",
            Self::Plain => "text/plain",
            Self::Latin1 => "STRING",
        }
    }

    /// The form `name` names; `None` for a form that is not text, or not one
    /// a reader asks for.
    #[cfg(wayland_host)]
    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::OFFERED.into_iter().find(|form| {
            // MIME type parameters are case-insensitive, and `utf-8` is
            // written `UTF-8` as often.
            form.name().eq_ignore_ascii_case(name)
        })
    }

    /// The form a reader asks for among the `offered` names: the first of
    /// [`Self::OFFERED`] that the owner offers; `None` when it offers no
    /// text.
    #[cfg(wayland_host)]
    pub(crate) fn preferred<'a>(offered: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        offered
            .into_iter()
            .filter_map(Self::named)
            .min_by_key(|form| Self::OFFERED.iter().position(|offered| offered == form))
    }

    /// `bytes` received in this form, as text: UTF-8 with any ill-formed
    /// sequence replaced, ISO 8859-1 by its code points. A trailing NUL,
    /// which some owners write, is not text.
    pub(crate) fn decode(self, mut bytes: Vec<u8>) -> String {
        if bytes.last() == Some(&0) {
            bytes.pop();
        }
        match self {
            Self::PlainUtf8 | Self::Utf8String | Self::Plain => match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
            },
            Self::Latin1 => bytes.into_iter().map(char::from).collect(),
        }
    }

    /// How many bytes of text [`decode`](Self::decode) makes of `bytes`,
    /// measured without making it: a reader refuses text past its limit
    /// before ISO 8859-1 or ill-formed UTF-8 grows as it decodes.
    pub(crate) fn decoded_len(self, bytes: &[u8]) -> usize {
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        match self {
            Self::PlainUtf8 | Self::Utf8String | Self::Plain => bytes
                .utf8_chunks()
                .map(|chunk| {
                    // Each ill-formed sequence becomes one U+FFFD.
                    let replaced = if chunk.invalid().is_empty() {
                        0
                    } else {
                        char::REPLACEMENT_CHARACTER.len_utf8()
                    };
                    chunk.valid().len() + replaced
                })
                .sum(),
            Self::Latin1 => bytes.len() + bytes.iter().filter(|byte| **byte >= 0x80).count(),
        }
    }

    /// Whether the form's bytes are UTF-8, the same bytes for each such
    /// form.
    pub(crate) const fn is_utf8(self) -> bool {
        !matches!(self, Self::Latin1)
    }

    /// `text` in this form: UTF-8 as it is, ISO 8859-1 with each character
    /// past it written `?`.
    pub(crate) fn encode(self, text: &str) -> Vec<u8> {
        match self {
            Self::PlainUtf8 | Self::Utf8String | Self::Plain => text.as_bytes().to_vec(),
            Self::Latin1 => text
                .chars()
                .map(|character| u8::try_from(u32::from(character)).unwrap_or(b'?'))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader asks for UTF-8 first, whichever order the owner offers its
    /// forms in, falls back to ISO 8859-1, and asks for nothing where no
    /// text is offered.
    #[cfg(wayland_host)]
    #[test]
    fn a_reader_asks_for_utf8_first() {
        for (case, offered, expected) in [
            (
                "a GTK owner",
                &["text/plain;charset=utf-8", "UTF8_STRING", "STRING"][..],
                Some(TextForm::PlainUtf8),
            ),
            (
                "UTF-8 offered last",
                &["STRING", "text/plain", "UTF8_STRING"],
                Some(TextForm::Utf8String),
            ),
            (
                "the charset in capitals",
                &["image/png", "text/plain;charset=UTF-8"],
                Some(TextForm::PlainUtf8),
            ),
            ("no charset named", &["text/plain"], Some(TextForm::Plain)),
            (
                "only ISO 8859-1",
                &["STRING", "TEXT"],
                Some(TextForm::Latin1),
            ),
            ("no text", &["image/png", "text/uri-list"], None),
            ("nothing", &[], None),
        ] {
            assert_eq!(
                TextForm::preferred(offered.iter().copied()),
                expected,
                "{case}"
            );
        }
    }

    /// Text survives each form it fits in; ISO 8859-1 writes what it cannot
    /// hold as `?`, ill-formed UTF-8 reads as replacement characters, and a
    /// trailing NUL is dropped.
    #[test]
    fn text_round_trips_through_each_form() {
        for form in TextForm::OFFERED {
            assert_eq!(
                form.decode(form.encode("\u{e9}t\u{e9}")),
                "\u{e9}t\u{e9}",
                "{form:?}"
            );
            assert_eq!(TextForm::named(form.name()), Some(form), "{form:?}");
        }
        assert_eq!(TextForm::Latin1.encode("a\u{2014}b"), b"a?b");
        assert_eq!(TextForm::Latin1.decode(vec![0x63, 0xe9]), "c\u{e9}");
        assert_eq!(
            TextForm::PlainUtf8.decode(vec![b'a', 0xff, b'b']),
            "a\u{fffd}b"
        );
        assert_eq!(TextForm::Utf8String.decode(b"text\0".to_vec()), "text");
    }

    /// The decoded length is measured without decoding and matches what
    /// decoding makes, ill-formed UTF-8 and ISO 8859-1's high half included.
    #[test]
    fn the_decoded_length_matches_the_decoded_text() {
        let cases: [&[u8]; 6] = [
            b"",
            b"text\0",
            b"a\xffb",
            b"\xff\xfe\xfd",
            b"\xe2\x82",
            "\u{e9}t\u{e9}".as_bytes(),
        ];
        for bytes in cases {
            for form in TextForm::OFFERED {
                assert_eq!(
                    form.decoded_len(bytes),
                    form.decode(bytes.to_vec()).len(),
                    "{form:?} {bytes:?}"
                );
            }
        }
    }
}
