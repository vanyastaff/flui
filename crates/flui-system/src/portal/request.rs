//! A portal call that answers through an `org.freedesktop.portal.Request`
//! object, and the `file://` URIs its answers carry.

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use futures_util::StreamExt;
use zbus::{MatchRule, MessageStream, OwnedObjectPath, OwnedVariant};

use super::PortalError;

pub(super) const DESTINATION: &str = "org.freedesktop.portal.Desktop";
pub(super) const PATH: &str = "/org/freedesktop/portal/desktop";

/// How the user ended a request.
pub(super) enum Response {
    /// Answered, with the portal's results.
    Answered(HashMap<String, OwnedVariant>),
    /// Cancelled by the user.
    Cancelled,
    /// Ended some other way: the portal's own failure.
    Ended,
}

/// Call `method` of `interface` and wait for its `Request`'s `Response`.
///
/// `token` is the request's `handle_token`, already in `body`'s options: it
/// names the `Request` object before the call, so the subscription to its
/// response is in place before the portal can answer.
pub(super) async fn call<B>(
    connection: &zbus::Connection,
    interface: &str,
    method: &str,
    token: &str,
    body: &B,
) -> Result<Response, PortalError>
where
    B: serde::Serialize + zbus::DynamicType,
{
    let sender = connection
        .unique_name()
        .ok_or(PortalError::Failed)?
        .trim_start_matches(':')
        .replace('.', "_");
    let expected = format!("{PATH}/request/{sender}/{token}");
    let mut responses = subscribe(connection, &expected).await?;

    let reply = connection
        .call_method(Some(DESTINATION), PATH, Some(interface), method, body)
        .await?;
    let handle = reply.body().deserialize::<OwnedObjectPath>()?;
    if handle.as_str() != expected {
        // A portal older than `handle_token` names the object itself; its
        // answer can only be heard from here on.
        responses = subscribe(connection, handle.as_str()).await?;
    }

    let message = responses.next().await.ok_or(PortalError::Failed)??;
    let (code, results) = message
        .body()
        .deserialize::<(u32, HashMap<String, OwnedVariant>)>()?;
    Ok(match code {
        0 => Response::Answered(results),
        1 => Response::Cancelled,
        _ => Response::Ended,
    })
}

async fn subscribe(
    connection: &zbus::Connection,
    path: &str,
) -> Result<MessageStream, PortalError> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(DESTINATION)
        .interface("org.freedesktop.portal.Request")
        .member("Response")
        .path(path)
        .build()?;
    Ok(MessageStream::for_match_rule(rule, connection, None).await?)
}

/// The local path a `file://` URI names, or `None` for any other URI: one
/// with another scheme, a remote host, or an escape that is not two hex
/// digits.
pub(super) fn file_path(uri: &str) -> Option<PathBuf> {
    let rest = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("FILE://"))?;
    let path = rest
        .strip_prefix("localhost")
        .unwrap_or(rest)
        .split(['?', '#'])
        .next()?;
    if !path.starts_with('/') {
        return None;
    }
    let mut bytes = Vec::with_capacity(path.len());
    let mut rest = path.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let [high, low, tail @ ..] = tail else {
                return None;
            };
            let hex = |digit: u8| char::from(digit).to_digit(16);
            let value = hex(*high)? * 16 + hex(*low)?;
            bytes.push(u8::try_from(value).expect("BUG: two hex digits fit a byte"));
            rest = tail;
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    Some(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::file_path;

    #[test]
    fn a_file_uri_names_its_decoded_local_path() {
        let cases: [(&str, Option<&[u8]>); 9] = [
            ("file:///home/a/b.txt", Some(b"/home/a/b.txt")),
            ("file://localhost/tmp/x", Some(b"/tmp/x")),
            ("file:///tmp/with%20space", Some(b"/tmp/with space")),
            ("file:///tmp/%C3%A9t%C3%A9", Some("/tmp/été".as_bytes())),
            ("file:///tmp/raw%ff", Some(b"/tmp/raw\xff")),
            ("file:///tmp/bad%2", None),
            ("file://server/share/x", None),
            ("https://example.com/x", None),
            ("file:relative", None),
        ];
        for (uri, expected) in cases {
            let expected = expected.map(|bytes| {
                use std::os::unix::ffi::OsStrExt;
                Path::new(std::ffi::OsStr::from_bytes(bytes)).to_path_buf()
            });
            assert_eq!(file_path(uri), expected, "{uri}");
        }
    }
}
