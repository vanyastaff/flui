//! Shared response-head parsing for the host HTTP stacks.

use std::fmt::Display;

use flui_platform::http::{HeaderName, HeaderValue, Headers, HttpError};

/// The most response headers a head may carry.
pub(crate) const MAX_HEADERS: usize = 1024;

/// Response headers with a bounded number of entries.
pub(crate) struct HeadBuilder {
    headers: Headers,
}

impl HeadBuilder {
    /// No headers.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            headers: Headers::new(),
        }
    }

    /// Add one header, unless the head already holds [`MAX_HEADERS`].
    pub(crate) fn try_append(
        &mut self,
        name: HeaderName,
        value: HeaderValue,
    ) -> Result<(), HttpError> {
        if self.headers.len() >= MAX_HEADERS {
            return Err(protocol(format!("more than {MAX_HEADERS} headers")));
        }
        self.headers.append(name, value);
        Ok(())
    }

    /// Decode a web `ByteString` and add it under `name`.
    #[cfg(any(target_os = "android", target_arch = "wasm32", test))]
    pub(crate) fn try_push_byte_string(
        &mut self,
        name: &str,
        value: &str,
    ) -> Result<(), HttpError> {
        let value = decode_byte_string(value)?;
        let name = HeaderName::try_new(name).map_err(protocol)?;
        let value = HeaderValue::try_new(value).map_err(protocol)?;
        self.try_append(name, value)
    }

    /// The collected headers.
    #[must_use]
    pub(crate) fn finish(self) -> Headers {
        self.headers
    }
}

/// Decode one Unicode scalar to one byte, as a web `ByteString` requires.
#[cfg(any(target_os = "android", target_arch = "wasm32", test))]
fn decode_byte_string(value: &str) -> Result<Vec<u8>, HttpError> {
    value
        .chars()
        .map(|character| u8::try_from(u32::from(character)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| protocol("a header value is not a byte string"))
}

/// Whether the first well-formed `content-length` is strictly over `limit`.
#[must_use]
pub(crate) fn declared_too_large(headers: &Headers, limit: u64) -> bool {
    headers
        .get("content-length")
        .and_then(HeaderValue::to_str)
        .and_then(|length| length.trim().parse::<u64>().ok())
        .is_some_and(|length| length > limit)
}

fn protocol(error: impl Display) -> HttpError {
    HttpError::Protocol {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_builder_enforces_the_header_limit() {
        let mut builder = HeadBuilder::new();
        for _ in 0..MAX_HEADERS {
            builder
                .try_append(
                    HeaderName::try_new("x-value").expect("a token"),
                    HeaderValue::try_new("value").expect("a value"),
                )
                .expect("the header limit accepts 1024 entries");
        }

        let error = builder
            .try_append(
                HeaderName::try_new("x-extra").expect("a token"),
                HeaderValue::try_new("value").expect("a value"),
            )
            .expect_err("the 1025th header is over the limit");
        assert_eq!(
            error,
            HttpError::Protocol {
                detail: "more than 1024 headers".to_owned(),
            }
        );
        assert_eq!(builder.finish().len(), MAX_HEADERS);
    }

    #[test]
    fn byte_string_roundtrips_every_byte() {
        let encoded = (0..=u8::MAX).map(char::from).collect::<String>();
        let decoded = decode_byte_string(&encoded).expect("every scalar names one byte");
        assert_eq!(decoded, (0..=u8::MAX).collect::<Vec<_>>());

        let mut builder = HeadBuilder::new();
        for byte in [b'\t', b' ', b'~', 0x80, 0xff] {
            builder
                .try_push_byte_string("x-value", &char::from(byte).to_string())
                .expect("a valid header value");
        }
        let headers = builder.finish();
        let values = headers
            .iter()
            .map(|(_, value)| value.as_bytes())
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            [&b"\t"[..], &b" "[..], &b"~"[..], &[0x80][..], &[0xff][..]]
        );
    }

    #[test]
    fn byte_string_refuses_a_scalar_above_one_byte() {
        let mut builder = HeadBuilder::new();
        let error = builder
            .try_push_byte_string("x-value", "\u{100}")
            .expect_err("U+0100 does not fit in one byte");
        assert!(matches!(error, HttpError::Protocol { .. }));
        assert!(builder.finish().is_empty());
    }

    #[test]
    fn declared_length_checks_the_first_content_length() {
        assert!(!declared_too_large(&content_length(b"1024"), 1024));
        assert!(declared_too_large(&content_length(b" 1025 \t"), 1024));
        assert!(!declared_too_large(&content_length(b"not-a-number"), 0));
        assert!(!declared_too_large(
            &content_length(b"18446744073709551616"),
            u64::MAX,
        ));
        assert!(!declared_too_large(&content_length(&[0xff]), 0));

        let mut repeated = content_length(b"1024");
        repeated.append(
            HeaderName::try_new("content-length").expect("a token"),
            HeaderValue::try_new("1025").expect("a value"),
        );
        assert!(!declared_too_large(&repeated, 1024));
    }

    fn content_length(value: &[u8]) -> Headers {
        let mut headers = Headers::new();
        headers.append(
            HeaderName::try_new("content-length").expect("a token"),
            HeaderValue::try_new(value).expect("a value"),
        );
        headers
    }
}
