//! HTTP as a capability of the host (ADR-0280).
//!
//! A widget acquires [`HttpCapability`] in a lifecycle hook and gets an
//! [`Http`] handle. The handle is `Send`: a request runs on a background
//! lane, and its answer reaches the owner thread through whatever carried
//! the work there. Every [`HttpClient`] drives its own requests, so the
//! future [`Http::send`] returns is polled on any thread without an async
//! runtime of the caller's: the host's provider runs the request where its
//! network stack lives and wakes the waiter.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::capability::Capability;

/// What an [`HttpClient`] request resolves to: a `Send` future of `T` or an
/// [`HttpError`].
pub type HttpFuture<T> = Pin<Box<dyn Future<Output = Result<T, HttpError>> + Send + 'static>>;

/// Sends HTTP requests: a host's network stack, or a test's script.
///
/// A provider implements it; widgets and services reach it through
/// [`Http`].
pub trait HttpClient: Send + Sync + 'static {
    /// Send `request`.
    ///
    /// The returned future needs no async runtime of the caller's: the
    /// client runs the request itself and wakes the future when its
    /// response head arrives. Dropping the future, or the response's body
    /// before its end, abandons the request.
    fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse>;
}

/// The marker of the HTTP capability (ADR-0152): its handle is [`Http`].
#[derive(Debug)]
#[non_exhaustive]
pub enum HttpCapability {}

impl Capability for HttpCapability {
    type Handle = Http;
    const NAME: &'static str = "http";
}

/// A widget's or a service's way to the host's HTTP client.
///
/// Cheap to clone; clones share the client and its connections.
#[derive(Clone)]
pub struct Http {
    client: Arc<dyn HttpClient>,
}

impl Http {
    /// A handle over `client`.
    #[must_use]
    pub fn new(client: Arc<dyn HttpClient>) -> Self {
        Self { client }
    }

    /// Send `request`; see [`HttpClient::send`].
    pub fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
        self.client.send(request)
    }
}

impl fmt::Debug for Http {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Http").finish_non_exhaustive()
    }
}

/// An HTTP request method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Method {
    /// `GET`.
    Get,
    /// `HEAD`.
    Head,
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `PATCH`.
    Patch,
    /// `DELETE`.
    Delete,
    /// `OPTIONS`.
    Options,
}

impl Method {
    /// The method as it is written on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A header's name: an RFC 9110 token, kept in lowercase.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HeaderName(Box<str>);

/// A header name or value that HTTP does not allow.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidHeader {
    /// The name is empty or has a character outside an RFC 9110 token.
    #[error("{0:?} is not a header name")]
    Name(String),
    /// The value has a control character other than a tab.
    #[error("a header value may not hold control characters")]
    Value,
}

impl HeaderName {
    /// `name` in lowercase.
    ///
    /// # Errors
    ///
    /// [`InvalidHeader::Name`] when `name` is empty or not a token.
    pub fn try_new(name: &str) -> Result<Self, InvalidHeader> {
        if name.is_empty() || !name.bytes().all(is_token_byte) {
            return Err(InvalidHeader::Name(name.to_owned()));
        }
        Ok(Self(name.to_ascii_lowercase().into_boxed_str()))
    }

    /// The name, in lowercase.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HeaderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A header's value: visible ASCII, spaces and tabs, or opaque bytes above
/// ASCII; never a line break.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HeaderValue(Box<[u8]>);

impl HeaderValue {
    /// `value` as it is written on the wire.
    ///
    /// # Errors
    ///
    /// [`InvalidHeader::Value`] when `value` holds a control character other
    /// than a tab.
    pub fn try_new(value: impl AsRef<[u8]>) -> Result<Self, InvalidHeader> {
        let value = value.as_ref();
        if value.iter().any(|&b| (b < 0x20 && b != b'\t') || b == 0x7f) {
            return Err(InvalidHeader::Value);
        }
        Ok(Self(value.into()))
    }

    /// The value's bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The value as text, when it is valid UTF-8.
    #[must_use]
    pub fn to_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }
}

/// `tchar` of RFC 9110 §5.6.2.
const fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Headers in the order they were added; a name may repeat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Headers(Vec<(HeaderName, HeaderValue)>);

impl Headers {
    /// No headers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `value` under `name`, after any value already there.
    pub fn append(&mut self, name: HeaderName, value: HeaderValue) {
        self.0.push((name, value));
    }

    /// The first value under `name`, compared without case.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&HeaderValue> {
        self.0
            .iter()
            .find(|(own, _)| own.as_str().eq_ignore_ascii_case(name))
            .map(|(_, value)| value)
    }

    /// Every header, in the order they were added.
    pub fn iter(&self) -> impl Iterator<Item = (&HeaderName, &HeaderValue)> {
        self.0.iter().map(|(name, value)| (name, value))
    }

    /// How many headers there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// An HTTP request: its method, URL, headers, body and limits.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct HttpRequest {
    method: Method,
    url: String,
    headers: Headers,
    body: Vec<u8>,
    body_limit: u64,
    deadline: Duration,
}

impl HttpRequest {
    /// The largest response body a request accepts unless it says
    /// otherwise: 256 MiB.
    pub const DEFAULT_BODY_LIMIT: u64 = 256 * 1024 * 1024;

    /// How long a request may take, response body included, unless it says
    /// otherwise: two minutes.
    pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(120);

    /// A `method` request for `url`, with no headers, no body and the
    /// default limits. The client checks the URL when it sends.
    #[must_use]
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Headers::new(),
            body: Vec::new(),
            body_limit: Self::DEFAULT_BODY_LIMIT,
            deadline: Self::DEFAULT_DEADLINE,
        }
    }

    /// A `GET` for `url`; see [`Self::new`].
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self::new(Method::Get, url)
    }

    /// Add a header.
    #[must_use = "the builder returns the updated request"]
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.append(name, value);
        self
    }

    /// Send `body` with the request.
    #[must_use = "the builder returns the updated request"]
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Accept a response body of at most `limit` bytes; a longer one fails
    /// with [`HttpError::TooLarge`] without being read whole.
    #[must_use = "the builder returns the updated request"]
    pub fn with_body_limit(mut self, limit: u64) -> Self {
        self.body_limit = limit;
        self
    }

    /// Give up with [`HttpError::Timeout`] when the request, response body
    /// included, takes longer than `deadline`.
    #[must_use = "the builder returns the updated request"]
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    /// The method.
    #[must_use]
    pub fn method(&self) -> Method {
        self.method
    }

    /// The URL, as given.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The headers.
    #[must_use]
    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// The largest response body accepted.
    #[must_use]
    pub fn body_limit(&self) -> u64 {
        self.body_limit
    }

    /// How long the request may take.
    #[must_use]
    pub fn deadline(&self) -> Duration {
        self.deadline
    }
}

/// A response's status code, 100 to 999.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StatusCode(u16);

impl StatusCode {
    /// `200 OK`.
    pub const OK: Self = Self(200);
    /// `404 Not Found`.
    pub const NOT_FOUND: Self = Self(404);

    /// The status `code`.
    ///
    /// # Errors
    ///
    /// [`HttpError::Protocol`] when `code` is outside 100 to 999.
    pub fn try_new(code: u16) -> Result<Self, HttpError> {
        if (100..=999).contains(&code) {
            Ok(Self(code))
        } else {
            Err(HttpError::Protocol {
                detail: format!("status code {code} is outside 100 to 999"),
            })
        }
    }

    /// The code as a number.
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self.0
    }

    /// Whether the code is 2xx.
    #[must_use]
    pub const fn is_success(self) -> bool {
        self.0 >= 200 && self.0 < 300
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A response: its status and headers, and a body still to be read.
#[derive(Debug)]
#[non_exhaustive]
pub struct HttpResponse {
    status: StatusCode,
    headers: Headers,
    body: ResponseBody,
}

impl HttpResponse {
    /// A response with `status`, `headers` and `body`.
    #[must_use]
    pub fn new(status: StatusCode, headers: Headers, body: ResponseBody) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    /// The status.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The headers.
    #[must_use]
    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    /// The body, still to be read.
    #[must_use]
    pub fn into_body(self) -> ResponseBody {
        self.body
    }
}

/// Where a [`ResponseBody`]'s bytes come from: a client's connection, or a
/// test's script.
pub trait BodySource: Send + 'static {
    /// The next chunk of the body, `None` at its end. Like
    /// [`HttpClient::send`]'s future, it needs no async runtime of the
    /// caller's.
    fn poll_chunk(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Vec<u8>, HttpError>>>;
}

/// A body whose bytes are all here.
struct Whole(Option<Vec<u8>>);

impl BodySource for Whole {
    fn poll_chunk(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Vec<u8>, HttpError>>> {
        Poll::Ready(self.0.take().filter(|bytes| !bytes.is_empty()).map(Ok))
    }
}

/// A response body, read in chunks within its request's limit.
pub struct ResponseBody {
    source: Pin<Box<dyn BodySource>>,
    limit: u64,
    read: u64,
    /// Set once the body ended or failed.
    done: bool,
}

impl ResponseBody {
    /// A body read from `source`, failing with [`HttpError::TooLarge`] past
    /// `limit` bytes.
    #[must_use]
    pub fn new(source: impl BodySource, limit: u64) -> Self {
        Self {
            source: Box::pin(source),
            limit,
            read: 0,
            done: false,
        }
    }

    /// A body of `bytes`, with no limit of its own.
    #[must_use]
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self::new(Whole(Some(bytes.into())), u64::MAX)
    }

    /// The next chunk, `None` at the end.
    ///
    /// # Errors
    ///
    /// The source's failure, or [`HttpError::TooLarge`] once the bytes read
    /// pass the limit. Every later call answers `None`.
    pub fn poll_chunk(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Vec<u8>, HttpError>>> {
        if self.done {
            return Poll::Ready(None);
        }
        let chunk = match self.source.as_mut().poll_chunk(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(chunk) => chunk,
        };
        let chunk = match chunk {
            Some(Ok(bytes)) => {
                let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                self.read = self.read.saturating_add(len);
                if self.read > self.limit {
                    Some(Err(HttpError::TooLarge { limit: self.limit }))
                } else {
                    Some(Ok(bytes))
                }
            }
            other => other,
        };
        if !matches!(chunk, Some(Ok(_))) {
            self.done = true;
        }
        Poll::Ready(chunk)
    }

    /// The next chunk, `None` at the end; see [`Self::poll_chunk`].
    pub fn next_chunk(&mut self) -> impl Future<Output = Option<Result<Vec<u8>, HttpError>>> + '_ {
        std::future::poll_fn(move |cx| self.poll_chunk(cx))
    }

    /// The whole body.
    ///
    /// # Errors
    ///
    /// The first failure [`Self::next_chunk`] answers.
    pub async fn into_bytes(mut self) -> Result<Vec<u8>, HttpError> {
        let mut bytes = Vec::new();
        while let Some(chunk) = self.next_chunk().await {
            bytes.extend_from_slice(&chunk?);
        }
        Ok(bytes)
    }
}

impl fmt::Debug for ResponseBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseBody")
            .field("limit", &self.limit)
            .field("read", &self.read)
            .finish_non_exhaustive()
    }
}

/// A host whose FLUI has no HTTP client yet, though its OS has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HttpHost {
    /// A native host running on executors its embedder handed FLUI
    /// (`AppConfig::with_executors`), with no Tokio reactor for the default
    /// client.
    EmbedderExecutors,
    /// Android, until its provider sets up the platform's trust store.
    Android,
    /// The web, until its provider sends requests with `fetch`.
    Web,
}

impl fmt::Display for HttpHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmbedderExecutors => "a host on its embedder's executors",
            Self::Android => "Android",
            Self::Web => "the web",
        })
    }
}

/// Why an HTTP request failed. A response with any status is not a
/// failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HttpError {
    /// The URL cannot be requested: it does not parse, or its scheme is not
    /// `http` or `https`.
    #[error("{url:?} is not an HTTP URL")]
    InvalidUrl {
        /// The URL as given.
        url: String,
    },
    /// No connection to the server: its name did not resolve, or it
    /// refused or dropped the connection.
    #[error("could not connect: {detail}")]
    Connect {
        /// What the network stack reported.
        detail: String,
    },
    /// The TLS handshake failed, or the server's certificate is not trusted.
    #[error("TLS failed: {detail}")]
    Tls {
        /// What the network stack reported.
        detail: String,
    },
    /// The request passed its deadline.
    #[error("the request passed its deadline")]
    Timeout,
    /// The response body passed the request's limit.
    #[error("the response body is over the {limit}-byte limit")]
    TooLarge {
        /// The limit the request set.
        limit: u64,
    },
    /// The server's response broke HTTP, or the connection ended inside it.
    #[error("the response is broken: {detail}")]
    Protocol {
        /// What the network stack reported.
        detail: String,
    },
    /// The host dropped the request before it was answered, or its body
    /// before it ended: its lanes shut down under it.
    #[error("the request was cancelled")]
    Cancelled,
    /// The host's lanes refused to send the request: they are full, unable
    /// to start, or shutting down.
    #[error("the host's lanes refused the request: {0}")]
    Refused(crate::executor::SpawnError),
    /// This host's FLUI has no HTTP client yet (a gap of FLUI's, not of the
    /// OS).
    #[error("HTTP is not implemented yet on {host}")]
    NotYetOnThisHost {
        /// The host.
        host: HttpHost,
    },
}
