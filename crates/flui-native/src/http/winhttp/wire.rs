//! What WinHTTP's wide strings and error codes mean to
//! `flui_platform::http`, apart from WinHTTP itself so it runs on every
//! host's tests.
//!
//! WinHTTP takes and gives header text as UTF-16 that stands for the bytes
//! on the wire one to one (ISO-8859-1), so a byte goes over as the code unit
//! of the same value and comes back the same way; a code unit past `0xFF`,
//! which WinHTTP never decodes from the wire, comes back as its UTF-8.

#![forbid(unsafe_code)]

use flui_platform::http::{HeaderName, HeaderValue, Headers, HttpError};

/// Where in the exchange WinHTTP failed: what an error code means when it
/// says nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    /// Before the request goes out: the URL, the session, the handles.
    Open,
    /// Sending it and reading the response's head.
    Send,
    /// Reading the body.
    Read,
}

// WinHTTP's error codes (`winhttp.h`), here so this module builds on every
// host.
const TIMEOUT: u32 = 12002;
const INVALID_URL: u32 = 12005;
const UNRECOGNIZED_SCHEME: u32 = 12006;
const NAME_NOT_RESOLVED: u32 = 12007;
const OPERATION_CANCELLED: u32 = 12017;
const CANNOT_CONNECT: u32 = 12029;
const CONNECTION_ERROR: u32 = 12030;
const INVALID_SERVER_RESPONSE: u32 = 12152;
/// The secure channel and certificate failures, `ERROR_WINHTTP_SECURE_*`
/// and `CLIENT_AUTH_CERT_NEEDED`.
const SECURE: [u32; 13] = [
    12037, 12038, 12044, 12045, 12057, 12157, 12169, 12170, 12175, 12179, 12186, 12187, 12188,
];

/// What a `SECURE_FAILURE` notification's flags name, in its words.
const SECURE_FLAGS: [(u32, &str); 8] = [
    (0x1, "the revocation check failed"),
    (0x2, "the certificate is invalid"),
    (0x4, "the certificate was revoked"),
    (0x8, "the certificate's authority is not trusted"),
    (0x10, "the certificate's name does not match the host"),
    (0x20, "the certificate has expired or is not yet valid"),
    (0x40, "the certificate is not for server authentication"),
    (0x8000_0000, "the secure channel failed"),
];

/// The [`HttpError`] WinHTTP's `code` stands for at `stage`, with the flags
/// of a failed secure check (0 for none) and the request's `url`.
pub(super) fn failure(code: u32, stage: Stage, secure_failure: u32, url: &str) -> HttpError {
    let detail = || format!("WinHTTP error {code}");
    match code {
        TIMEOUT => HttpError::Timeout,
        OPERATION_CANCELLED => HttpError::Cancelled,
        INVALID_URL | UNRECOGNIZED_SCHEME => HttpError::InvalidUrl {
            url: url.to_owned(),
        },
        NAME_NOT_RESOLVED | CANNOT_CONNECT => HttpError::Connect { detail: detail() },
        secure if SECURE.contains(&secure) => {
            let named: Vec<&str> = SECURE_FLAGS
                .iter()
                .filter(|&&(flag, _)| secure_failure & flag != 0)
                .map(|&(_, name)| name)
                .collect();
            HttpError::Tls {
                detail: if named.is_empty() {
                    detail()
                } else {
                    format!("{}: {}", detail(), named.join(", "))
                },
            }
        }
        INVALID_SERVER_RESPONSE => HttpError::Protocol { detail: detail() },
        // A connection dropped before the head is one that failed; inside
        // the body it cut the response short.
        CONNECTION_ERROR if stage != Stage::Read => HttpError::Connect { detail: detail() },
        _ => match stage {
            Stage::Open => HttpError::Host { detail: detail() },
            Stage::Send => HttpError::Connect { detail: detail() },
            Stage::Read => HttpError::Protocol { detail: detail() },
        },
    }
}

/// The request's headers as WinHTTP takes them, `Name: value` lines each
/// ended by CRLF, or `None` for none.
pub(super) fn request_headers(headers: &Headers) -> Option<Vec<u16>> {
    let mut block = Vec::new();
    for (name, value) in headers.iter() {
        block.extend(name.as_str().bytes().map(u16::from));
        block.extend([u16::from(b':'), u16::from(b' ')]);
        block.extend(value.as_bytes().iter().copied().map(u16::from));
        block.extend([u16::from(b'\r'), u16::from(b'\n')]);
    }
    (!block.is_empty()).then_some(block)
}

/// The headers of a response's raw head (`WINHTTP_QUERY_RAW_HEADERS_CRLF`:
/// the status line, then `Name: value` lines, each ended by CRLF).
///
/// # Errors
///
/// [`HttpError::Protocol`] for a line that is not a header.
pub(super) fn response_headers(raw: &[u16]) -> Result<Headers, HttpError> {
    let mut bytes = Vec::with_capacity(raw.len());
    for unit in char::decode_utf16(raw.iter().copied()) {
        let unit = unit.unwrap_or(char::REPLACEMENT_CHARACTER);
        match u8::try_from(u32::from(unit)) {
            Ok(byte) => bytes.push(byte),
            Err(_) => bytes.extend_from_slice(unit.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    let broken = |line: &[u8]| HttpError::Protocol {
        detail: format!("not a header line: {:?}", String::from_utf8_lossy(line)),
    };
    let mut headers = Headers::new();
    // The status line comes first.
    for line in bytes.split(|&byte| byte == b'\n').skip(1) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let colon = line
            .iter()
            .position(|&byte| byte == b':')
            .ok_or_else(|| broken(line))?;
        let name = std::str::from_utf8(&line[..colon]).map_err(|_| broken(line))?;
        let value = line[colon + 1..].trim_ascii();
        let name = HeaderName::try_new(name.trim_ascii()).map_err(|_| broken(line))?;
        let value = HeaderValue::try_new(value).map_err(|_| broken(line))?;
        headers.append(name, value);
    }
    Ok(headers)
}

/// The object a request names on its server: the URL's path and query
/// (WinHTTP's "extra info"), without the fragment, which stays with the
/// client, NUL-ended.
pub(super) fn object_name(path: &[u16], extra: &[u16]) -> Vec<u16> {
    let hash = u16::from(b'#');
    let extra = extra
        .iter()
        .position(|&unit| unit == hash)
        .map_or(extra, |at| &extra[..at]);
    let mut object = Vec::with_capacity(path.len() + extra.len() + 2);
    if path.is_empty() {
        object.push(u16::from(b'/'));
    }
    object.extend_from_slice(path);
    object.extend_from_slice(extra);
    object.push(0);
    object
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    /// Each error code at each stage maps to the error the caller can act
    /// on; a failed secure check names what it failed.
    ///
    /// Red-check: map every code by its stage alone (a timeout reads as a
    /// failed connection, a certificate error as one too).
    #[test]
    fn winhttp_errors_map_to_what_the_caller_can_act_on() {
        let url = "https://example.test/";
        let cases: [(u32, Stage, u32, HttpError); 9] = [
            (TIMEOUT, Stage::Read, 0, HttpError::Timeout),
            (OPERATION_CANCELLED, Stage::Send, 0, HttpError::Cancelled),
            (
                UNRECOGNIZED_SCHEME,
                Stage::Open,
                0,
                HttpError::InvalidUrl {
                    url: url.to_owned(),
                },
            ),
            (
                NAME_NOT_RESOLVED,
                Stage::Send,
                0,
                HttpError::Connect {
                    detail: "WinHTTP error 12007".to_owned(),
                },
            ),
            (
                12175,
                Stage::Send,
                0x8 | 0x20,
                HttpError::Tls {
                    detail: "WinHTTP error 12175: the certificate's authority is not trusted, \
                             the certificate has expired or is not yet valid"
                        .to_owned(),
                },
            ),
            (
                CONNECTION_ERROR,
                Stage::Send,
                0,
                HttpError::Connect {
                    detail: "WinHTTP error 12030".to_owned(),
                },
            ),
            (
                CONNECTION_ERROR,
                Stage::Read,
                0,
                HttpError::Protocol {
                    detail: "WinHTTP error 12030".to_owned(),
                },
            ),
            (
                12019,
                Stage::Open,
                0,
                HttpError::Host {
                    detail: "WinHTTP error 12019".to_owned(),
                },
            ),
            (
                INVALID_SERVER_RESPONSE,
                Stage::Send,
                0,
                HttpError::Protocol {
                    detail: "WinHTTP error 12152".to_owned(),
                },
            ),
        ];
        for (code, stage, flags, expected) in cases {
            assert_eq!(
                failure(code, stage, flags, url),
                expected,
                "{code} at {stage:?}"
            );
        }
    }

    /// Headers go out as `Name: value` lines whose bytes keep their values,
    /// and come back from the raw head the same way, past the status line,
    /// repeated names kept; a line that is not a header is a broken
    /// response.
    ///
    /// Red-check: send a value's bytes as UTF-8 (a byte past 0x7F doubles);
    /// keep the status line as a header (the parse fails).
    #[test]
    fn headers_keep_their_bytes_both_ways() {
        let mut headers = Headers::new();
        for (name, value) in [("Accept", b"*/*".as_slice()), ("X-Name", b"caf\xe9")] {
            headers.append(
                HeaderName::try_new(name).expect("a token"),
                HeaderValue::try_new(value).expect("a value"),
            );
        }
        let block = request_headers(&headers).expect("two headers");
        let mut expected = wide("accept: */*\r\nx-name: caf");
        expected.extend([0xe9, u16::from(b'\r'), u16::from(b'\n')]);
        assert_eq!(block, expected);
        assert_eq!(request_headers(&Headers::new()), None);

        let mut raw = wide("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nSet-Cookie: a=1\r\n");
        raw.extend(wide("Set-Cookie:b=2 \r\nX-Name: caf"));
        raw.extend([0xe9]);
        raw.extend(wide("\r\n\r\n"));
        let parsed = response_headers(&raw).expect("a whole head");
        let pairs: Vec<(String, Vec<u8>)> = parsed
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("content-length".to_owned(), b"5".to_vec()),
                ("set-cookie".to_owned(), b"a=1".to_vec()),
                ("set-cookie".to_owned(), b"b=2".to_vec()),
                ("x-name".to_owned(), b"caf\xe9".to_vec()),
            ]
        );
        assert!(matches!(
            response_headers(&wide("HTTP/1.1 200 OK\r\nno colon here\r\n")),
            Err(HttpError::Protocol { .. })
        ));
    }

    /// The object keeps the path and the query and drops the fragment; an
    /// empty path asks for the root.
    ///
    /// Red-check: send the fragment (it reaches the server).
    #[test]
    fn the_object_name_drops_the_fragment() {
        let name = |path: &str, extra: &str| {
            let mut object = object_name(&wide(path), &wide(extra));
            assert_eq!(object.pop(), Some(0), "NUL-ended");
            String::from_utf16(&object).expect("UTF-16")
        };
        assert_eq!(name("/a/b", "?q=1#top"), "/a/b?q=1");
        assert_eq!(name("/a", "#top"), "/a");
        assert_eq!(name("", ""), "/");
        assert_eq!(name("", "?q"), "/?q");
    }
}
