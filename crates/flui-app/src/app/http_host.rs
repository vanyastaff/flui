//! The built-in HTTP capability (ADR-0280).
//!
//! Every host sends through a stack of its own: the page's `fetch` on the
//! web, the system's `HttpURLConnection` on Android, WinHTTP on Windows,
//! `NSURLSession` on macOS and iOS, and `ureq` on Linux and the BSDs, which
//! have no system stack. Each exchange holds a slot of the run's IO lane until
//! it ends, so a full or stopping lane refuses it as it refuses any work, and
//! a lane that shuts down under an exchange ends it. The caller's future only
//! waits on a channel: it needs no runtime of its own, and an embedder's
//! executors send as the host's own lanes do. Dropping it, or the response's
//! body, abandons the exchange.

use std::sync::{Arc, Weak};

use flui_platform::Unsupported;
use flui_platform::capability::{CapabilityProvider, ProviderContext};
use flui_platform::http::{Http, HttpCapability};
use flui_runtime::execution::ExecutionServices;

use super::capability::ExecutionSlot;

/// The built-in HTTP provider over the run's lanes.
pub(crate) struct HostHttp {
    slot: ExecutionSlot,
    /// The host's own stack, one authority per run: the page's `fetch`,
    /// Android's `HttpURLConnection`, WinHTTP, `NSURLSession` or `ureq`.
    system: Http,
}

impl HostHttp {
    pub(crate) fn new(slot: ExecutionSlot) -> Self {
        Self {
            slot,
            #[cfg(target_arch = "wasm32")]
            system: Http::new(Arc::new(flui_native::http::FetchClient::new())),
            #[cfg(target_os = "android")]
            system: Http::new(Arc::new(flui_native::http::UrlConnectionClient::new())),
            #[cfg(target_os = "windows")]
            system: Http::new(Arc::new(flui_native::http::WinHttpClient::new())),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            system: Http::new(Arc::new(flui_native::http::UrlSessionClient::new())),
            #[cfg(not(any(
                target_arch = "wasm32",
                target_os = "android",
                target_os = "windows",
                target_os = "macos",
                target_os = "ios"
            )))]
            system: Http::new(Arc::new(flui_native::http::UreqClient::new())),
        }
    }

    /// The client this provider hands out over `services`.
    fn http(&self, services: Weak<ExecutionServices>) -> Http {
        Http::new(Arc::new(held::LaneHeld::new(services, self.system.clone())))
    }
}

impl CapabilityProvider<HttpCapability> for HostHttp {
    fn provide(&self, _ctx: &ProviderContext<'_>) -> Result<Http, Unsupported> {
        let services = self.slot.borrow().clone().ok_or_else(|| {
            // Before the host installs a UI runtime it has no lanes.
            Unsupported::of::<HttpCapability>(flui_platform::UnsupportedReason::Unavailable)
        })?;
        Ok(self.http(services))
    }
}

/// A client whose exchanges each hold a slot of the run's IO lane: the host's
/// own stack (the page's `fetch`, Android's `HttpURLConnection`, WinHTTP,
/// `NSURLSession`, `ureq`) runs off the lane, and is admitted by it all the
/// same.
mod held {
    use std::future::poll_fn;
    use std::pin::Pin;
    use std::sync::Weak;
    use std::task::{Context, Poll};

    use flui_platform::executor::SpawnError;
    use flui_platform::http::{
        BodySource, Http, HttpAuthority, HttpClient, HttpError, HttpFuture, HttpRequest,
        HttpResponse, ResponseBody,
    };
    use flui_runtime::execution::ExecutionServices;
    use tokio::sync::oneshot;

    /// Sends through `client`, each exchange holding an IO slot from the
    /// request until its body ends, fails or is dropped.
    pub(super) struct LaneHeld {
        services: Weak<ExecutionServices>,
        client: Http,
    }

    impl LaneHeld {
        pub(super) fn new(services: Weak<ExecutionServices>, client: Http) -> Self {
            Self { services, client }
        }
    }

    impl HttpClient for LaneHeld {
        fn authority(&self) -> HttpAuthority {
            self.client.authority()
        }

        fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
            let Some(services) = self.services.upgrade() else {
                return refused(SpawnError::ShuttingDown);
            };
            // The slot is the lane's task that waits for the exchange to let
            // go of `release`; the lane's admission refuses it as any work,
            // and a lane that shuts down drops it, which ends the exchange.
            let (release, released) = oneshot::channel::<()>();
            if let Err(reason) = services.spawn_io(Box::pin(async move {
                let _ = released.await;
            })) {
                return refused(reason);
            }
            let mut head = self.client.send(request);
            Box::pin(async move {
                let mut release = release;
                let response = poll_fn(|ctx| {
                    if release.poll_closed(ctx).is_ready() {
                        // Dropping `head` abandons the request.
                        return Poll::Ready(Err(HttpError::Cancelled));
                    }
                    head.as_mut().poll(ctx)
                })
                .await?;
                let status = response.status();
                let headers = response.headers().clone();
                let body = Held {
                    body: Some(response.into_body()),
                    release: Some(release),
                };
                // The inner body keeps the request's limit.
                Ok(HttpResponse::new(
                    status,
                    headers,
                    ResponseBody::new(body, u64::MAX),
                ))
            })
        }
    }

    fn refused(reason: SpawnError) -> HttpFuture<HttpResponse> {
        Box::pin(std::future::ready(Err(HttpError::Refused(reason))))
    }

    /// A body that holds its exchange's IO slot until it ends.
    struct Held {
        /// `None` once the lane shut down under it: dropping it abandoned the
        /// exchange.
        body: Option<ResponseBody>,
        /// Dropped at the body's end or failure, or with the body.
        release: Option<oneshot::Sender<()>>,
    }

    impl BodySource for Held {
        fn poll_chunk(
            mut self: Pin<&mut Self>,
            ctx: &mut Context<'_>,
        ) -> Poll<Option<Result<Vec<u8>, HttpError>>> {
            if let Some(release) = self.release.as_mut()
                && release.poll_closed(ctx).is_ready()
            {
                self.release = None;
                self.body = None;
                return Poll::Ready(Some(Err(HttpError::Cancelled)));
            }
            let Some(body) = self.body.as_mut() else {
                return Poll::Ready(None);
            };
            let chunk = std::task::ready!(body.poll_chunk(ctx));
            if !matches!(chunk, Some(Ok(_))) {
                self.release = None;
            }
            Poll::Ready(chunk)
        }
    }
}

// The tests run where `ureq` sends: real sockets against a loopback server.
#[cfg(all(
    test,
    not(any(
        target_arch = "wasm32",
        target_os = "android",
        target_os = "windows",
        target_os = "macos",
        target_os = "ios"
    ))
))]
mod tests {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    use flui_platform::http::{Http, HttpError, HttpRequest};
    use flui_runtime::execution::{DeterministicExecutors, ExecutionServices};

    use super::*;

    /// Polls `future` on this thread with no async runtime, as a widget's
    /// owner thread does.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        pollster::block_on(future)
    }

    /// An `Http` over `services`, as the built-in provider hands it out.
    fn http_over(services: &Arc<ExecutionServices>) -> Http {
        HostHttp::new(ExecutionSlot::default()).http(Arc::downgrade(services))
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
    /// not HTTP, and an embedder's executors, which send too; each polled on
    /// a thread with no async runtime.
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

        let too_large =
            block_on(http.send(HttpRequest::get(format!("http://{address}/large")).body_limit(16)));
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
            1,
            "the exchange holds a slot of the embedder's IO lane"
        );
        let response = block_on(pending).expect("an embedder's executors send too");
        assert_eq!(
            block_on(response.into_body().into_bytes()).expect("the body arrives"),
            b"bytes"
        );
    }

    /// Dropping the request's future, and later the body of an answered one,
    /// closes its connection on the lane, and so does a body its reader holds
    /// unread past the request's deadline; a lane shut down under a body
    /// ends it, and a lane gone refuses the request.
    #[test]
    fn dropping_a_request_or_its_body_closes_the_connection() {
        let (address, seen) = serve(|path| match path {
            // Never answers.
            "/silent" => Vec::new(),
            // More body than the lane holds for a reader that stops reading.
            "/large" => ok(&vec![7_u8; 8 * 1024 * 1024]),
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

        let held = block_on(
            http.send(
                HttpRequest::get(format!("http://{address}/large"))
                    .deadline(Duration::from_millis(300)),
            ),
        )
        .expect("the head arrives within the deadline");
        assert_eq!(seen.recv_timeout(wait), Ok("accepted"));
        assert_eq!(
            seen.recv_timeout(wait),
            Ok("closed"),
            "a body held unread past its deadline closes its connection"
        );
        assert_eq!(
            block_on(held.into_body().into_bytes()),
            Err(HttpError::Timeout),
            "the reader learns the deadline passed"
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

    /// The web's exchanges each hold a slot of the IO lane from the request
    /// until the body ends, fails or is dropped, so a full lane refuses the
    /// next request as it refuses any work, and a stopped lane refuses all.
    #[test]
    fn a_held_exchange_takes_an_io_slot_until_its_body_lets_go() {
        use flui_platform::testing::{ScriptedHttp, ScriptedResponse};

        let scripted = ScriptedHttp::new();
        scripted.respond(
            "https://example.test/a",
            ScriptedResponse::ok(b"abc".to_vec()),
        );
        scripted.respond(
            "https://example.test/gone",
            ScriptedResponse::Fail(HttpError::Connect {
                detail: "refused".to_owned(),
            }),
        );
        let embedder = DeterministicExecutors::new();
        let services = Arc::new(ExecutionServices::with_limits(
            Some(embedder.host_executors()),
            1,
            1,
        ));
        let http = Http::new(Arc::new(held::LaneHeld::new(
            Arc::downgrade(&services),
            scripted.handle(),
        )));
        assert_eq!(http.authority(), scripted.handle().authority());
        let saturated = || {
            block_on(http.send(HttpRequest::get("https://example.test/a"))).map(|_| ())
                == Err(HttpError::Refused(
                    flui_platform::executor::SpawnError::Saturated,
                ))
        };

        let response = block_on(http.send(HttpRequest::get("https://example.test/a")))
            .expect("the scripted response");
        embedder.run_until_idle();
        assert!(saturated(), "an answered body holds the lane's one slot");
        drop(response);
        embedder.run_until_idle();
        assert!(!saturated(), "a dropped body gives its slot back");
        embedder.run_until_idle();

        let read = block_on(http.send(HttpRequest::get("https://example.test/a")))
            .expect("the scripted response");
        let mut body = read.into_body();
        let chunk = block_on(body.next_chunk());
        assert_eq!(chunk, Some(Ok(b"abc".to_vec())));
        assert_eq!(block_on(body.next_chunk()), None);
        embedder.run_until_idle();
        assert!(!saturated(), "a body read to its end gives its slot back");
        embedder.run_until_idle();

        assert_eq!(
            block_on(http.send(HttpRequest::get("https://example.test/gone"))).map(|_| ()),
            Err(HttpError::Connect {
                detail: "refused".to_owned()
            })
        );
        embedder.run_until_idle();
        assert!(!saturated(), "a failed request gives its slot back");
        embedder.run_until_idle();

        drop(services);
        assert_eq!(
            block_on(http.send(HttpRequest::get("https://example.test/a"))).map(|_| ()),
            Err(HttpError::Refused(
                flui_platform::executor::SpawnError::ShuttingDown
            ))
        );
    }
}
