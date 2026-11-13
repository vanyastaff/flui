//! [`FetchClient`]: HTTP through the page's `fetch`.

use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::task::{Context, Poll};

use flui_platform::http::{
    BodySource, HeaderName, HeaderValue, Headers, HttpAuthority, HttpClient, HttpError, HttpFuture,
    HttpRequest, HttpResponse, ResponseBody, StatusCode,
};
use js_sys::{Array, Promise, Uint8Array};
use tokio::sync::{mpsc, oneshot};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;

/// Body chunks a response holds unread before the page waits for the reader.
const BODY_CHUNKS_IN_FLIGHT: usize = 4;

/// The longest delay `setTimeout` keeps: a longer one overflows its 32-bit
/// count and fires at once.
const LONGEST_TIMER_MS: f64 = 2_147_483_647.0;

// One request through the page's `fetch`, looked up on `globalThis` when the
// request is sent, so a worker sends too. A failure rejects with a tag:
// 'url', 'timeout', 'aborted', or 'connect:' or 'protocol:' followed by the
// browser's message.
#[wasm_bindgen(inline_js = r"
export class FetchExchange {
    constructor(method, url, names, values, body, deadlineMs) {
        this.controller = new AbortController();
        this.timedOut = false;
        this.reader = undefined;
        this.left = false;
        this.timer = undefined;
        this.passed = new Promise(resolve => { this.pass = resolve; });
        let parsed;
        try { parsed = new URL(url); } catch { parsed = undefined; }
        if (!parsed || (parsed.protocol !== 'http:' && parsed.protocol !== 'https:')) {
            this.answer = Promise.reject('url');
            return;
        }
        if (body !== undefined && (method === 'GET' || method === 'HEAD')) {
            this.answer = Promise.reject('protocol:a GET or HEAD request carries no body in a browser');
            return;
        }
        let headers;
        try {
            headers = new Headers();
            names.forEach((name, i) => headers.append(name, values[i]));
        } catch (error) {
            this.answer = Promise.reject('protocol:' + error);
            return;
        }
        if (deadlineMs !== undefined) {
            this.timer = setTimeout(() => {
                this.timedOut = true;
                this.stop();
                this.pass();
            }, deadlineMs);
        }
        let sent;
        try {
            sent = globalThis.fetch(url, { method, headers, body, signal: this.controller.signal });
        } catch (error) {
            sent = Promise.reject(error);
        }
        const answered = Promise.resolve(sent).then(response => {
            this.reader = response.body ? response.body.getReader() : undefined;
            // Stopped while the head was on its way: let go of the body.
            if (this.left) this.stop();
            return [response.status, Array.from(response.headers)];
        }, error => { throw this.failure(error, 'connect:'); });
        // The deadline ends the wait even where `fetch` ignores the abort.
        this.answer = Promise.race([answered, this.passed.then(() => Promise.reject('timeout'))]);
    }
    failure(error, kind) {
        if (this.timedOut) return 'timeout';
        if (this.left) return 'aborted';
        return kind + (error && error.message !== undefined ? error.message : String(error));
    }
    head() { return this.answer; }
    next() {
        if (this.left) return Promise.reject(this.timedOut ? 'timeout' : 'aborted');
        if (!this.reader) return Promise.resolve(undefined);
        // A stopped reader ends its pending read as if the body ended.
        return this.reader.read().then(({ done, value }) => {
            if (this.left) throw this.timedOut ? 'timeout' : 'aborted';
            return done ? undefined : value;
        }, error => { throw this.failure(error, 'protocol:'); });
    }
    stop() {
        this.left = true;
        this.controller.abort();
        if (this.reader) this.reader.cancel().catch(() => {});
        this.reader = undefined;
    }
    abort() {
        clearTimeout(this.timer);
        this.stop();
    }
}
")]
extern "C" {
    /// One request in flight.
    type FetchExchange;

    /// Send a `method` request for `url` with the headers `names[i]:
    /// values[i]` and `body`, aborted after `deadline_ms` when it is given.
    #[wasm_bindgen(constructor)]
    fn new(
        method: &str,
        url: &str,
        names: &Array,
        values: &Array,
        body: Option<Uint8Array>,
        deadline_ms: Option<f64>,
    ) -> FetchExchange;

    /// Resolves to `[status, [[name, value], …]]` when the head arrives.
    #[wasm_bindgen(method)]
    fn head(this: &FetchExchange) -> Promise;

    /// Resolves to the body's next `Uint8Array`, `undefined` at its end.
    #[wasm_bindgen(method)]
    fn next(this: &FetchExchange) -> Promise;

    /// Stop the request and its timer; harmless once it ended.
    #[wasm_bindgen(method)]
    fn abort(this: &FetchExchange);
}

/// An HTTP client over the page's `fetch` (ADR-0280).
///
/// Each request runs in a task of the page and answers through a channel, so
/// the future [`send`](HttpClient::send) returns, and the response's body,
/// are polled on any thread with no runtime of the caller's. Dropping either
/// aborts the request. The browser's own rules apply: a cross-origin request
/// needs the server's CORS consent, forbidden headers are dropped, and a
/// failed connection, TLS handshake or CORS check are one
/// [`HttpError::Connect`], which the page cannot tell apart.
///
/// Every clone answers with one [`HttpAuthority`].
#[derive(Clone, Debug, Default)]
pub struct FetchClient {
    /// What [`HttpClient::authority`] names: the page's network stack, as this
    /// client reaches it.
    page: Arc<()>,
}

impl FetchClient {
    /// A client over the page's `fetch`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl HttpClient for FetchClient {
    fn authority(&self) -> HttpAuthority {
        HttpAuthority::of(&self.page)
    }

    fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
        let (head_tx, head_rx) = oneshot::channel();
        wasm_bindgen_futures::spawn_local(exchange(request, head_tx));
        // A task dropped unanswered is a page that tore it down.
        Box::pin(async move { head_rx.await.unwrap_or(Err(HttpError::Cancelled)) })
    }
}

/// Aborts its exchange when dropped: on every exit of [`exchange`], a reader
/// that left included.
struct Aborting(FetchExchange);

impl Drop for Aborting {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Sends `request` and answers its head on `head_tx`, then streams its body
/// to the reader; stops as soon as nobody waits.
async fn exchange(
    request: HttpRequest,
    mut head_tx: oneshot::Sender<Result<HttpResponse, HttpError>>,
) {
    let limit = request.body_limit;
    let exchange = Aborting(start(&request));
    let head = JsFuture::from(exchange.0.head());
    let Some(head) = unless_closed(head, poll_fn(|ctx| head_tx.poll_closed(ctx))).await else {
        return;
    };
    let head = head.map_err(|reason| match failure(reason) {
        Rejection::Url => HttpError::InvalidUrl {
            url: request.url.clone(),
        },
        Rejection::Other(error) => error,
    });
    let (status, headers) = match head.and_then(|head| read_head(&head)) {
        Ok(head) => head,
        Err(error) => {
            let _ = head_tx.send(Err(error));
            return;
        }
    };
    let declared = headers
        .get("content-length")
        .and_then(HeaderValue::to_str)
        .and_then(|length| length.trim().parse::<u64>().ok());
    if declared.is_some_and(|length| length > limit) {
        let _ = head_tx.send(Err(HttpError::TooLarge { limit }));
        return;
    }
    let (body_tx, body_rx) = mpsc::channel(BODY_CHUNKS_IN_FLIGHT);
    let chunks = Chunks {
        receiver: body_rx,
        ended: false,
    };
    let answered = HttpResponse::new(status, headers, ResponseBody::new(chunks, limit));
    if head_tx.send(Ok(answered)).is_err() {
        return;
    }
    // The page's timer stops the exchange at the deadline, so a reader that
    // stops reading still finds the request's `Timeout` after the chunks it
    // left in the channel.
    loop {
        let next = JsFuture::from(exchange.0.next());
        let chunk = match unless_closed(next, body_tx.closed()).await {
            None => return,
            Some(Ok(value)) if value.is_undefined() => Chunk::End,
            Some(Ok(value)) => match value.dyn_into::<Uint8Array>() {
                Ok(bytes) => Chunk::Bytes(bytes.to_vec()),
                Err(_) => Chunk::Failed(protocol("a body chunk is not bytes")),
            },
            Some(Err(reason)) => Chunk::Failed(match failure(reason) {
                Rejection::Url => protocol("the body failed"),
                Rejection::Other(error) => error,
            }),
        };
        let last = !matches!(chunk, Chunk::Bytes(_));
        match body_tx.send(chunk).await {
            Ok(()) if !last => {}
            _ => return,
        }
    }
}

/// The page's exchange for `request`.
fn start(request: &HttpRequest) -> FetchExchange {
    let names = Array::new();
    let values = Array::new();
    for (name, value) in request.headers.iter() {
        names.push(&JsValue::from_str(name.as_str()));
        values.push(&JsValue::from_str(&byte_string(value.as_bytes())));
    }
    let body = (!request.body.is_empty()).then(|| Uint8Array::from(request.body.as_slice()));
    // A deadline past what the timer can hold is no deadline.
    let deadline_ms = Some(request.deadline.as_secs_f64() * 1000.0)
        .filter(|ms| *ms <= LONGEST_TIMER_MS)
        .map(f64::ceil);
    FetchExchange::new(
        request.method.as_str(),
        request.url.as_str(),
        &names,
        &values,
        body,
        deadline_ms,
    )
}

/// `bytes` as the browser's `ByteString`: one character per byte.
fn byte_string(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

/// The status and headers of a head the page answered.
fn read_head(head: &JsValue) -> Result<(StatusCode, Headers), HttpError> {
    let head = head
        .dyn_ref::<Array>()
        .ok_or_else(|| protocol("the head is not an array"))?;
    let status = head
        .get(0)
        .as_f64()
        .filter(|status| status.fract() == 0.0 && (0.0..=f64::from(u16::MAX)).contains(status))
        .ok_or_else(|| protocol("the status is not a number"))?;
    let status = StatusCode::try_new(status as u16)?;
    let entries = head
        .get(1)
        .dyn_into::<Array>()
        .map_err(|_| protocol("the headers are not an array"))?;
    let mut headers = Headers::new();
    for entry in entries.iter() {
        let entry = entry
            .dyn_into::<Array>()
            .map_err(|_| protocol("a header is not a pair"))?;
        let (Some(name), Some(value)) = (entry.get(0).as_string(), entry.get(1).as_string()) else {
            return Err(protocol("a header is not a pair of strings"));
        };
        // A `ByteString` holds one byte per character.
        let value = value
            .chars()
            .map(|c| u8::try_from(u32::from(c)))
            .collect::<Result<Vec<u8>, _>>()
            .map_err(|_| protocol("a header value is not a byte string"))?;
        let name = HeaderName::try_new(&name).map_err(protocol)?;
        let value = HeaderValue::try_new(value).map_err(protocol)?;
        headers.append(name, value);
    }
    Ok((status, headers))
}

fn protocol(error: impl std::fmt::Display) -> HttpError {
    HttpError::Protocol {
        detail: error.to_string(),
    }
}

/// A rejection of the page's exchange.
enum Rejection {
    /// The URL is not an HTTP one; the request names it.
    Url,
    Other(HttpError),
}

/// What a rejection of the page's exchange means to the caller.
fn failure(reason: JsValue) -> Rejection {
    let reason = reason.as_string().unwrap_or_default();
    Rejection::Other(match reason.as_str() {
        "url" => return Rejection::Url,
        "timeout" => HttpError::Timeout,
        "aborted" => HttpError::Cancelled,
        _ => match reason.strip_prefix("connect:") {
            Some(detail) => HttpError::Connect {
                detail: detail.to_owned(),
            },
            None => protocol(reason.trim_prefix("protocol:")),
        },
    })
}

/// `work`'s output, or `None` once `closed` resolves first.
async fn unless_closed<T>(
    work: impl Future<Output = T>,
    closed: impl Future<Output = ()>,
) -> Option<T> {
    let mut work = pin!(work);
    let mut closed = pin!(closed);
    poll_fn(|ctx| {
        if closed.as_mut().poll(ctx).is_ready() {
            return Poll::Ready(None);
        }
        work.as_mut().poll(ctx).map(Some)
    })
    .await
}

/// What the page forwards of a response's body.
enum Chunk {
    Bytes(Vec<u8>),
    /// The body ended where its server ended it.
    End,
    Failed(HttpError),
}

/// The body chunks the page forwards.
struct Chunks {
    receiver: mpsc::Receiver<Chunk>,
    /// Whether the page reported the body's end or its failure.
    ended: bool,
}

impl BodySource for Chunks {
    fn poll_chunk(
        mut self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Option<Result<Vec<u8>, HttpError>>> {
        if self.ended {
            return Poll::Ready(None);
        }
        Poll::Ready(match std::task::ready!(self.receiver.poll_recv(ctx)) {
            Some(Chunk::Bytes(bytes)) => Some(Ok(bytes)),
            Some(Chunk::End) => {
                self.ended = true;
                None
            }
            Some(Chunk::Failed(error)) => {
                self.ended = true;
                Some(Err(error))
            }
            // The page tore the task down mid-body.
            None => {
                self.ended = true;
                Some(Err(HttpError::Cancelled))
            }
        })
    }
}
