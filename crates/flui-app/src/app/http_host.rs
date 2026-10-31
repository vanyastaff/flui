//! The built-in HTTP capability (ADR-0280).
//!
//! A request is sent from the run's IO lane, whose default pool runs inside a
//! Tokio runtime, so `reqwest` drives it there and its connections stay in
//! one pool per run. The caller's future only waits on a channel: it needs
//! no runtime of its own. Dropping it, or the response's body, closes the
//! channel, and the exchange on the lane stops at its next step.
//!
//! Where FLUI has no client yet (Android, the web, a host on its embedder's
//! executors) a request answers [`HttpError::NotYetOnThisHost`]: a gap of
//! FLUI's, reported as such and never skipped.

use std::sync::Arc;

use flui_platform::Unsupported;
use flui_platform::capability::{CapabilityProvider, ProviderContext};
use flui_platform::http::{
    Http, HttpAuthority, HttpCapability, HttpClient, HttpError, HttpFuture, HttpHost,
};
use flui_platform::http::{HttpRequest, HttpResponse};

use super::capability::ExecutionSlot;

/// The built-in HTTP provider over the run's lanes.
pub(crate) struct HostHttp {
    #[cfg_attr(
        any(target_os = "android", target_arch = "wasm32"),
        expect(dead_code, reason = "these hosts have no HTTP client yet")
    )]
    slot: ExecutionSlot,
    /// One connection pool per run, built on the first request.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    client: Arc<std::sync::OnceLock<Result<reqwest::Client, String>>>,
}

impl HostHttp {
    pub(crate) fn new(slot: ExecutionSlot) -> Self {
        Self {
            slot,
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            client: Arc::default(),
        }
    }
}

impl CapabilityProvider<HttpCapability> for HostHttp {
    fn provide(&self, _cx: &ProviderContext<'_>) -> Result<Http, Unsupported> {
        #[cfg(target_os = "android")]
        return Ok(Http::new(Arc::new(NotYet(Arc::new(HttpHost::Android)))));
        #[cfg(target_arch = "wasm32")]
        return Ok(Http::new(Arc::new(NotYet(Arc::new(HttpHost::Web)))));
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        {
            let services = self.slot.borrow().clone().ok_or_else(|| {
                // Before the host installs a UI runtime it has no lanes.
                Unsupported::of::<HttpCapability>(flui_platform::UnsupportedReason::Unavailable)
            })?;
            Ok(Http::new(Arc::new(lanes::LaneHttp::new(
                services,
                Arc::clone(&self.client),
            ))))
        }
    }
}

/// A client on a host whose FLUI has none yet.
#[cfg_attr(
    not(any(target_os = "android", target_arch = "wasm32")),
    expect(
        dead_code,
        reason = "a native host on its embedder's executors answers from the lane"
    )
)]
/// Its authority is its own: it answers nothing a cache could share.
struct NotYet(Arc<HttpHost>);

impl HttpClient for NotYet {
    fn authority(&self) -> HttpAuthority {
        HttpAuthority::of(&self.0)
    }

    fn send(&self, _request: HttpRequest) -> HttpFuture<HttpResponse> {
        let host = *self.0;
        Box::pin(std::future::ready(Err(HttpError::NotYetOnThisHost {
            host,
        })))
    }
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod lanes {
    use std::future::{Future, poll_fn};
    use std::pin::{Pin, pin};
    use std::sync::{Arc, OnceLock, Weak};
    use std::task::{Context, Poll};
    use std::time::Duration;

    use flui_platform::executor::SpawnError;
    use flui_platform::http::{
        BodySource, HeaderName, HeaderValue, Headers, HttpAuthority, HttpClient, HttpError,
        HttpFuture, HttpHost, HttpRequest, HttpResponse, Method, ResponseBody, StatusCode,
    };
    use flui_runtime::execution::ExecutionServices;
    use tokio::sync::{mpsc, oneshot};

    /// How long the client waits for a connection.
    const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

    /// How long the client waits for the next bytes of a response.
    const READ_TIMEOUT: Duration = Duration::from_secs(30);

    /// Body chunks a response holds unread before the lane waits for the
    /// reader.
    const BODY_CHUNKS_IN_FLIGHT: usize = 4;

    /// The run's pool, built once.
    type ClientCell = Arc<OnceLock<Result<reqwest::Client, String>>>;

    /// Sends requests from the run's IO lane.
    pub(super) struct LaneHttp {
        services: Weak<ExecutionServices>,
        client: ClientCell,
    }

    impl LaneHttp {
        pub(super) fn new(services: Weak<ExecutionServices>, client: ClientCell) -> Self {
            Self { services, client }
        }
    }

    impl HttpClient for LaneHttp {
        // The run's pool answers: every handle the provider hands out shares it.
        fn authority(&self) -> HttpAuthority {
            HttpAuthority::of(&self.client)
        }

        fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
            let Some(services) = self.services.upgrade() else {
                return refused(SpawnError::ShuttingDown);
            };
            // `reqwest` runs on Tokio's IO and timer drivers, which FLUI's
            // own IO lane enables; an embedder's executor may run no Tokio,
            // or one without them, and `reqwest` would panic there.
            if !services.owns_default_pools() {
                return Box::pin(std::future::ready(Err(HttpError::NotYetOnThisHost {
                    host: HttpHost::EmbedderExecutors,
                })));
            }
            let (head_tx, head_rx) = oneshot::channel();
            let exchange = exchange(Arc::clone(&self.client), request, head_tx);
            if let Err(reason) = services.spawn_io(Box::pin(exchange)) {
                return refused(reason);
            }
            // A sender dropped unanswered is a lane that shut down under it.
            Box::pin(async move { head_rx.await.unwrap_or(Err(HttpError::Cancelled)) })
        }
    }

    fn refused(reason: SpawnError) -> HttpFuture<HttpResponse> {
        Box::pin(std::future::ready(Err(HttpError::Refused(reason))))
    }

    /// Sends `request` and answers its head on `head_tx`, then streams its
    /// body to the reader; stops as soon as nobody waits.
    async fn exchange(
        client: ClientCell,
        request: HttpRequest,
        mut head_tx: oneshot::Sender<Result<HttpResponse, HttpError>>,
    ) {
        let limit = request.body_limit();
        let send = match build(&client, request) {
            Ok(send) => send,
            Err(error) => {
                let _ = head_tx.send(Err(error));
                return;
            }
        };
        let Some(sent) = unless_closed(send, poll_fn(|cx| head_tx.poll_closed(cx))).await else {
            return;
        };
        let mut response = match sent {
            Ok(response) => response,
            Err(error) => {
                let _ = head_tx.send(Err(transport(&error)));
                return;
            }
        };
        if response
            .content_length()
            .is_some_and(|length| length > limit)
        {
            let _ = head_tx.send(Err(HttpError::TooLarge { limit }));
            return;
        }
        let head = match head(&response) {
            Ok(head) => head,
            Err(error) => {
                let _ = head_tx.send(Err(error));
                return;
            }
        };
        let (body_tx, body_rx) = mpsc::channel(BODY_CHUNKS_IN_FLIGHT);
        let (status, headers) = head;
        let chunks = Chunks {
            receiver: body_rx,
            ended: false,
        };
        let answered = HttpResponse::new(status, headers, ResponseBody::new(chunks, limit));
        if head_tx.send(Ok(answered)).is_err() {
            return;
        }
        loop {
            let next = unless_closed(response.chunk(), body_tx.closed()).await;
            let chunk = match next {
                None => return,
                Some(Ok(None)) => Chunk::End,
                Some(Ok(Some(bytes))) => Chunk::Bytes(bytes.to_vec()),
                Some(Err(error)) => Chunk::Failed(transport(&error)),
            };
            let last = !matches!(chunk, Chunk::Bytes(_));
            if body_tx.send(chunk).await.is_err() || last {
                return;
            }
        }
    }

    /// `work`'s output, or `None` once `closed` resolves first: the reader
    /// left.
    async fn unless_closed<T>(
        work: impl Future<Output = T>,
        closed: impl Future<Output = ()>,
    ) -> Option<T> {
        let mut work = pin!(work);
        let mut closed = pin!(closed);
        poll_fn(|cx| {
            if closed.as_mut().poll(cx).is_ready() {
                return Poll::Ready(None);
            }
            work.as_mut().poll(cx).map(Some)
        })
        .await
    }

    /// The `reqwest` request for `request`, on the run's pool.
    fn build(
        client: &ClientCell,
        request: HttpRequest,
    ) -> Result<impl Future<Output = reqwest::Result<reqwest::Response>>, HttpError> {
        let client = client
            .get_or_init(|| {
                reqwest::Client::builder()
                    .connect_timeout(CONNECT_TIMEOUT)
                    .read_timeout(READ_TIMEOUT)
                    .build()
                    .map_err(|error| error.to_string())
            })
            .as_ref()
            .map_err(|detail| HttpError::Tls {
                detail: detail.clone(),
            })?;
        let invalid = || HttpError::InvalidUrl {
            url: request.url().to_owned(),
        };
        let url = reqwest::Url::parse(request.url()).map_err(|_| invalid())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(invalid());
        }
        let mut builder = client
            .request(method(request.method()), url)
            .timeout(request.deadline());
        for (name, value) in request.headers().iter() {
            builder = builder.header(name.as_str(), value.as_bytes());
        }
        if !request.body().is_empty() {
            builder = builder.body(request.body().to_vec());
        }
        Ok(builder.send())
    }

    fn method(method: Method) -> reqwest::Method {
        reqwest::Method::from_bytes(method.as_str().as_bytes())
            .expect("BUG: every Method's name is a token")
    }

    /// The status and headers of `response`.
    fn head(response: &reqwest::Response) -> Result<(StatusCode, Headers), HttpError> {
        let status = StatusCode::try_new(response.status().as_u16())?;
        let mut headers = Headers::new();
        for (name, value) in response.headers() {
            // `http` validated both already, to the same rules.
            let name = HeaderName::try_new(name.as_str()).map_err(protocol)?;
            let value = HeaderValue::try_new(value.as_bytes()).map_err(protocol)?;
            headers.append(name, value);
        }
        Ok((status, headers))
    }

    fn protocol(error: impl std::fmt::Display) -> HttpError {
        HttpError::Protocol {
            detail: error.to_string(),
        }
    }

    /// What a failed exchange means to the caller.
    fn transport(error: &reqwest::Error) -> HttpError {
        if error.is_timeout() {
            HttpError::Timeout
        } else if let Some(tls) = tls_failure(error) {
            HttpError::Tls {
                detail: tls.to_string(),
            }
        } else if error.is_connect() {
            HttpError::Connect {
                detail: error.to_string(),
            }
        } else {
            protocol(error)
        }
    }

    /// The TLS failure behind `error`, if a handshake or certificate check
    /// failed. Rustls reports it inside an `io::Error`, often one wrapped in
    /// another, and an `io::Error`'s `source` skips the error it wraps, so
    /// the walk steps into each wrapped error instead.
    fn tls_failure(error: &reqwest::Error) -> Option<&rustls::Error> {
        let mut link: Option<&(dyn std::error::Error + 'static)> = Some(error);
        while let Some(current) = link {
            if let Some(tls) = current.downcast_ref::<rustls::Error>() {
                return Some(tls);
            }
            link = match current.downcast_ref::<std::io::Error>() {
                Some(io) => io
                    .get_ref()
                    .map(|wrapped| wrapped as &(dyn std::error::Error + 'static)),
                None => current.source(),
            };
        }
        None
    }

    /// What the lane forwards of a response's body.
    enum Chunk {
        Bytes(Vec<u8>),
        /// The body ended where its server ended it.
        End,
        Failed(HttpError),
    }

    /// The body chunks the lane forwards.
    struct Chunks {
        receiver: mpsc::Receiver<Chunk>,
        /// Whether the lane reported the body's end or its failure.
        ended: bool,
    }

    impl BodySource for Chunks {
        fn poll_chunk(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Vec<u8>, HttpError>>> {
            if self.ended {
                return Poll::Ready(None);
            }
            Poll::Ready(match std::task::ready!(self.receiver.poll_recv(cx)) {
                Some(Chunk::Bytes(bytes)) => Some(Ok(bytes)),
                Some(Chunk::End) => {
                    self.ended = true;
                    None
                }
                Some(Chunk::Failed(error)) => {
                    self.ended = true;
                    Some(Err(error))
                }
                // The lane dropped the exchange mid-body: the host shut down.
                None => {
                    self.ended = true;
                    Some(Err(HttpError::Cancelled))
                }
            })
        }
    }
}

#[cfg(all(test, not(any(target_os = "android", target_arch = "wasm32"))))]
mod tests {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    use flui_platform::http::{Http, HttpError, HttpHost, HttpRequest};
    use flui_runtime::execution::{DeterministicExecutors, ExecutionServices};

    use super::*;

    /// Polls `future` on this thread with no async runtime, as a widget's
    /// owner thread does.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        pollster::block_on(future)
    }

    /// An `Http` over `services`, as the built-in provider hands it out.
    fn http_over(services: &Arc<ExecutionServices>) -> Http {
        Http::new(Arc::new(lanes::LaneHttp::new(
            Arc::downgrade(services),
            Arc::default(),
        )))
    }

    /// A loopback server that answers each connection's request with
    /// `respond(path)` and reports each connection it accepts and each it
    /// sees closed by the client.
    fn serve(
        respond: impl Fn(&str) -> Vec<u8> + Send + 'static,
    ) -> (SocketAddr, mpsc::Receiver<&'static str>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port binds");
        let address = listener
            .local_addr()
            .expect("a bound listener has an address");
        let (events, seen) = mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let _ = events.send("accepted");
                let respond = &respond;
                let events = events.clone();
                let mut request = [0_u8; 2048];
                let Ok(read) = stream.read(&mut request) else {
                    continue;
                };
                let request = String::from_utf8_lossy(&request[..read]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let _ = stream.write_all(&respond(&path));
                let _ = stream.flush();
                // Wait for the client to close its end.
                let mut rest = [0_u8; 64];
                while matches!(stream.read(&mut rest), Ok(n) if n > 0) {}
                let _ = events.send("closed");
            }
        });
        (address, seen)
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    /// The table of what a request on the run's lanes answers: a body, a
    /// status, a body past its limit, a failed TLS handshake, a URL that is
    /// not HTTP, and the host-executor gap; each polled on a thread with no
    /// async runtime.
    #[test]
    fn requests_on_the_runs_lanes_answer_without_a_runtime_of_the_callers() {
        let (address, _seen) = serve(|path| match path {
            "/image" => ok(b"bytes"),
            "/missing" => {
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            }
            _ => ok(&[7_u8; 64]),
        });
        let services = Arc::new(ExecutionServices::with_defaults());
        let http = http_over(&services);

        let response = block_on(http.send(HttpRequest::get(format!("http://{address}/image"))))
            .expect("the server answers");
        assert!(response.status().is_success());
        let body = block_on(response.into_body().into_bytes()).expect("the body arrives");
        assert_eq!(body, b"bytes");

        let missing = block_on(http.send(HttpRequest::get(format!("http://{address}/missing"))))
            .expect("a 404 is a response");
        assert_eq!(missing.status().as_u16(), 404);

        let too_large = block_on(
            http.send(HttpRequest::get(format!("http://{address}/large")).with_body_limit(16)),
        );
        assert!(
            matches!(too_large, Err(HttpError::TooLarge { limit: 16 })),
            "{too_large:?}"
        );

        let handshake = block_on(http.send(HttpRequest::get(format!("https://{address}/image"))));
        assert!(
            matches!(handshake, Err(HttpError::Tls { .. })),
            "a server that does not speak TLS fails the handshake: {handshake:?}"
        );

        let not_http = block_on(http.send(HttpRequest::get("file:///etc/hosts")));
        assert!(
            matches!(not_http, Err(HttpError::InvalidUrl { .. })),
            "{not_http:?}"
        );

        let embedder = DeterministicExecutors::new();
        let hosted = Arc::new(ExecutionServices::with_host(embedder.host_executors()));
        let pending = http_over(&hosted).send(HttpRequest::get(format!("http://{address}/image")));
        assert_eq!(
            embedder.run_until_idle(),
            0,
            "nothing reaches an embedder's executor"
        );
        assert_eq!(
            block_on(pending).map(|_| ()),
            Err(HttpError::NotYetOnThisHost {
                host: HttpHost::EmbedderExecutors
            }),
            "an embedder's executor may run no Tokio drivers: a typed gap, not a panic"
        );
    }

    /// Dropping the request's future, and later the body of an answered one,
    /// closes its connection on the lane.
    #[test]
    fn dropping_a_request_or_its_body_closes_the_connection() {
        let (address, seen) = serve(|path| match path {
            // Never answers.
            "/silent" => Vec::new(),
            // Headers, then a body that never ends.
            _ => b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\npartial".to_vec(),
        });
        let services = Arc::new(ExecutionServices::with_defaults());
        let http = http_over(&services);
        let wait = Duration::from_secs(10);

        let pending = http.send(HttpRequest::get(format!("http://{address}/silent")));
        assert_eq!(seen.recv_timeout(wait), Ok("accepted"));
        drop(pending);
        assert_eq!(
            seen.recv_timeout(wait),
            Ok("closed"),
            "a request dropped before its head closes its connection"
        );

        let response = block_on(http.send(HttpRequest::get(format!("http://{address}/stream"))))
            .expect("the head arrives");
        assert_eq!(seen.recv_timeout(wait), Ok("accepted"));
        drop(response);
        assert_eq!(
            seen.recv_timeout(wait),
            Ok("closed"),
            "a dropped body closes its connection"
        );

        let shut_down = Arc::new(ExecutionServices::with_defaults());
        let response = block_on(
            http_over(&shut_down).send(HttpRequest::get(format!("http://{address}/stream"))),
        )
        .expect("the head arrives");
        assert_eq!(seen.recv_timeout(wait), Ok("accepted"));
        drop(shut_down);
        assert_eq!(
            block_on(response.into_body().into_bytes()),
            Err(HttpError::Cancelled),
            "a body cut off by the host's shutdown is not a short success"
        );

        let services_gone = Arc::new(ExecutionServices::with_defaults());
        let refused = http_over(&services_gone);
        drop(services_gone);
        assert_eq!(
            block_on(refused.send(HttpRequest::get(format!("http://{address}/b")))).map(|_| ()),
            Err(HttpError::Refused(
                flui_platform::executor::SpawnError::ShuttingDown
            ))
        );
    }
}
