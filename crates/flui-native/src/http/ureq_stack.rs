//! The native hosts without a system HTTP stack (Linux and the BSDs) send
//! through `ureq`, a blocking client, over rustls with the platform's
//! verifier, so the system's trust store decides and the proxy comes from
//! the environment (`HTTPS_PROXY`, `NO_PROXY`, ...).
//!
//! Each request runs on a thread of its own (the `blocking` module's model)
//! on an `ureq` agent of its own, because an agent is what `ureq` interrupts:
//! [`ureq::Agent::shutdown`] closes the agent's connection under a blocked
//! connect handshake, send or read, which is how a dropped request, a dropped
//! body and a passed deadline end it. An agent whose body ended goes back to
//! a small pool of idle agents, so the next request to the same origin reuses
//! its connection. Which of the two happens is decided once, under the slot's
//! lock: an abort that comes after the agent went back to the pool finds the
//! slot empty and reaches no later request.

use std::io::Read;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use flui_platform::http::{
    HeaderName, HeaderValue, HttpClient, HttpError, HttpFuture, HttpRequest, HttpResponse,
    StatusCode,
};
use ureq::http::{Request, Uri};
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, Body, Error};

use super::blocking::{Abort, BlockingClient, Connection, Head, Stack};
use super::head::HeadBuilder;

/// The longest a connect, a name lookup or a wait for the next bytes may take
/// before the exchange fails with [`HttpError::Timeout`], whatever its own
/// deadline.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// Idle agents the pool keeps, each holding the connection its last request
/// left open.
const IDLE_AGENTS: usize = 8;

/// Bytes read from a body per chunk.
const CHUNK: usize = 16 * 1024;

/// The built-in [`HttpClient`] of the native hosts without a system stack.
pub struct UreqClient(BlockingClient<Agents>);

impl UreqClient {
    /// A client with a pool of its own.
    #[must_use]
    pub fn new() -> Self {
        Self(BlockingClient::new(Agents::new()))
    }
}

impl Default for UreqClient {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for UreqClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UreqClient").finish_non_exhaustive()
    }
}

impl HttpClient for UreqClient {
    fn authority(&self) -> flui_platform::http::HttpAuthority {
        self.0.authority()
    }

    fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
        self.0.send(request)
    }
}

/// The stack: agents taken from, and given back to, one pool.
pub(super) struct Agents {
    pool: Arc<Pool>,
}

impl Agents {
    fn new() -> Self {
        Self {
            pool: Arc::new(Pool::new()),
        }
    }
}

impl Stack for Agents {
    fn open(&self, request: &HttpRequest) -> Result<Box<dyn Connection>, HttpError> {
        let invalid = || HttpError::InvalidUrl {
            url: request.url().to_owned(),
        };
        let uri = Uri::try_from(request.url()).map_err(|_| invalid())?;
        let origin = match (uri.scheme_str(), uri.authority()) {
            (Some(scheme @ ("http" | "https")), Some(authority)) => {
                format!("{scheme}://{authority}")
            }
            _ => return Err(invalid()),
        };
        let mut outgoing = Request::builder()
            .method(request.method().as_str())
            .uri(uri);
        for (name, value) in request.headers().iter() {
            outgoing = outgoing.header(name.as_str(), value.as_bytes());
        }
        let outgoing = outgoing
            .body(request.body().to_vec())
            .map_err(|_| invalid())?;
        let agent = self.pool.take(&origin);
        Ok(Box::new(UreqConnection {
            slot: Arc::new(Slot(Mutex::new(Some(agent.clone())))),
            agent,
            pool: Arc::clone(&self.pool),
            origin,
            url: request.url().to_owned(),
            request: Some(outgoing),
            body: None,
        }))
    }
}

/// The idle agents, each with the origin its last request named.
pub(super) struct Pool {
    config: ureq::config::Config,
    idle: Mutex<Vec<(String, Agent)>>,
}

impl Pool {
    fn new() -> Self {
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .root_certs(RootCerts::PlatformVerifier)
            .unversioned_rustls_crypto_provider(Arc::new(
                rustls::crypto::aws_lc_rs::default_provider(),
            ))
            .build();
        let config = Agent::config_builder()
            .tls_config(tls)
            // A status is the caller's to read, not a failure of the exchange.
            .http_status_as_error(false)
            .timeout_resolve(Some(STEP_TIMEOUT))
            .timeout_connect(Some(STEP_TIMEOUT))
            .timeout_per_read(Some(STEP_TIMEOUT))
            .build();
        Self {
            config,
            idle: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<(String, Agent)>> {
        // Nothing panics while the lock is held; a poisoned one is still
        // whole.
        self.idle.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// An idle agent last used for `origin`, else a new one.
    fn take(&self, origin: &str) -> Agent {
        let reused = {
            let mut idle = self.lock();
            idle.iter()
                .position(|(last, _)| last == origin)
                .map(|at| idle.swap_remove(at).1)
        };
        reused.unwrap_or_else(|| Agent::new_with_config(self.config.clone()))
    }

    /// Keeps `agent`, whose last request named `origin`, for the next
    /// request; the oldest idle agent makes room.
    fn give_back(&self, origin: String, agent: Agent) {
        let evicted = {
            let mut idle = self.lock();
            let evicted = (idle.len() == IDLE_AGENTS).then(|| idle.remove(0));
            idle.push((origin, agent));
            evicted
        };
        // Outside the lock: dropping an agent closes its connection.
        drop(evicted);
    }
}

/// The request's agent until either the abort shuts it down or the ended
/// request gives it back to the pool: whichever takes it first.
struct Slot(Mutex<Option<Agent>>);

impl Slot {
    fn take(&self) -> Option<Agent> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

impl Abort for Slot {
    fn abort(&self) {
        // Outside the lock: the shutdown closes sockets.
        if let Some(agent) = self.take() {
            agent.shutdown();
        }
    }
}

struct UreqConnection {
    /// The request runs on this clone; the slot holds the one the pool or the
    /// abort takes.
    agent: Agent,
    slot: Arc<Slot>,
    pool: Arc<Pool>,
    origin: String,
    url: String,
    request: Option<Request<Vec<u8>>>,
    body: Option<ureq::BodyReader<'static>>,
}

impl Connection for UreqConnection {
    fn aborter(&self) -> Arc<dyn Abort> {
        self.slot.clone()
    }

    fn send(&mut self) -> Result<Head, HttpError> {
        let request = self.request.take().ok_or_else(|| HttpError::Host {
            detail: "the request was already sent".to_owned(),
        })?;
        let (parts, body) = request.into_parts();
        let answer = if body.is_empty() {
            self.agent.run(Request::from_parts(parts, ()))
        } else {
            self.agent.run(Request::from_parts(parts, body))
        };
        let response = answer.map_err(|error| failure(&error, Stage::Send, &self.url))?;
        let status = StatusCode::try_new(response.status().as_u16())?;
        let mut headers = HeadBuilder::new();
        for (name, value) in response.headers() {
            let broken = || HttpError::Protocol {
                detail: format!("a response header the contract refuses: {name}"),
            };
            headers.try_append(
                HeaderName::try_new(name.as_str()).map_err(|_| broken())?,
                HeaderValue::try_new(value.as_bytes()).map_err(|_| broken())?,
            )?;
        }
        self.body = Some(Body::into_reader(response.into_body()));
        Ok(Head {
            status,
            headers: headers.finish(),
        })
    }

    fn read(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        let Some(body) = self.body.as_mut() else {
            return Ok(None);
        };
        let mut chunk = vec![0; CHUNK];
        loop {
            match body.read(&mut chunk) {
                Ok(0) => {
                    // At its end the body handed its connection back to the
                    // agent, which can go back to the pool with it.
                    self.body = None;
                    if let Some(agent) = self.slot.take() {
                        self.pool.give_back(std::mem::take(&mut self.origin), agent);
                    }
                    return Ok(None);
                }
                Ok(read) => {
                    chunk.truncate(read);
                    return Ok(Some(chunk));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    return Err(failure(&Error::from(error), Stage::Read, &self.url));
                }
            }
        }
    }
}

/// Where in the exchange `ureq` failed: what an error means when it says
/// nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Sending the request and reading the response's head.
    Send,
    /// Reading the body.
    Read,
}

/// The [`HttpError`] `error` stands for at `stage`, for the request to `url`.
fn failure(error: &Error, stage: Stage, url: &str) -> HttpError {
    let detail = || error.to_string();
    match error {
        Error::Timeout(_) => HttpError::Timeout,
        Error::BadUri(_) | Error::Http(_) | Error::RequireHttpsOnly(_) => HttpError::InvalidUrl {
            url: url.to_owned(),
        },
        Error::HostNotFound
        | Error::ConnectionFailed
        | Error::ConnectProxyFailed(_)
        | Error::InvalidProxyUrl => HttpError::Connect { detail: detail() },
        Error::Tls(_) | Error::Pem(_) | Error::Rustls(_) | Error::TlsRequired => {
            HttpError::Tls { detail: detail() }
        }
        Error::Protocol(_)
        | Error::TooManyRedirects
        | Error::RedirectFailed
        | Error::LargeResponseHeader(..)
        | Error::BodyStalled => HttpError::Protocol { detail: detail() },
        Error::Io(io) => {
            if let Some(tls) = tls_failure(io) {
                HttpError::Tls {
                    detail: tls.to_string(),
                }
            } else if io.kind() == std::io::ErrorKind::TimedOut {
                HttpError::Timeout
            } else {
                by_stage(stage, detail())
            }
        }
        _ => by_stage(stage, detail()),
    }
}

/// A failure that names nothing more: a connection that failed before the
/// head, or a body cut short.
fn by_stage(stage: Stage, detail: String) -> HttpError {
    match stage {
        Stage::Send => HttpError::Connect { detail },
        Stage::Read => HttpError::Protocol { detail },
    }
}

/// The TLS failure behind `error`. An `io::Error`'s `source` skips the error
/// it wraps, so the walk steps into each wrapped error instead.
fn tls_failure(error: &std::io::Error) -> Option<&rustls::Error> {
    let mut link: Option<&(dyn std::error::Error + 'static)> = error
        .get_ref()
        .map(|wrapped| wrapped as &(dyn std::error::Error + 'static));
    while let Some(current) = link {
        if let Some(tls) = current.downcast_ref::<rustls::Error>() {
            return Some(tls);
        }
        if let Some(Error::Rustls(tls)) = current.downcast_ref::<Error>() {
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

#[cfg(test)]
mod tests;
