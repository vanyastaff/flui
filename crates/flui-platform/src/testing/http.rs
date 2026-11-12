//! A scripted HTTP client: the [`HttpCapability`] a headless host registers,
//! so no test reaches the network (ADR-0280).
//!
//! A request answers with what the test scripted for its URL. A URL with no
//! script fails with [`HttpError::Connect`] naming it, so a forgotten script
//! fails loudly instead of reaching a server.
//! [`hold`](ScriptedHttp::hold) keeps a URL's requests unanswered until
//! [`release`](ScriptedHttp::release), so a test decides what happens while
//! one is in flight.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Poll, Waker};

use crate::capability::{CapabilityRegistrar, ProviderContext};
use crate::http::{
    Headers, Http, HttpAuthority, HttpCapability, HttpClient, HttpError, HttpFuture, HttpRequest,
    HttpResponse, ResponseBody, StatusCode,
};

/// What a scripted URL answers.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptedResponse {
    /// A response with this status and body.
    Respond {
        /// The status.
        status: StatusCode,
        /// The body.
        body: Vec<u8>,
    },
    /// A failure.
    Fail(HttpError),
}

impl ScriptedResponse {
    /// `200 OK` with `body`.
    #[must_use]
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self::Respond {
            status: StatusCode::OK,
            body: body.into(),
        }
    }
}

/// The HTTP client a headless test host hands its widgets. Cheap to clone;
/// every clone is the same client.
#[derive(Clone, Default)]
pub struct ScriptedHttp {
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    scripts: HashMap<String, ScriptedResponse>,
    /// The waker of each held request that was polled, by request.
    held: HashMap<u64, (String, Waker)>,
    /// The last request's number.
    next_request: u64,
    holding: Vec<String>,
    requests: Vec<HttpRequest>,
    abandoned: usize,
}

fn lock_state(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state
        .lock()
        .expect("BUG: ScriptedHttp runs no user code while holding its state lock")
}

impl ScriptedHttp {
    /// A client with no script: every request fails until the test scripts
    /// its URL.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer every later request for `url` with `response`, and the held
    /// ones once released.
    pub fn respond(&self, url: impl Into<String>, response: ScriptedResponse) {
        // Converted before the lock: the conversion is the caller's code.
        let url: String = url.into();
        lock_state(&self.state).scripts.insert(url, response);
    }

    /// Keep the requests for `url` unanswered until
    /// [`release`](Self::release).
    pub fn hold(&self, url: impl Into<String>) {
        let url: String = url.into();
        lock_state(&self.state).holding.push(url);
    }

    /// Answer the held requests for `url` and every later one.
    ///
    /// Every held request is woken even when a waker panics; the first
    /// panic is resumed once all of them are.
    pub fn release(&self, url: &str) {
        let wakers: Vec<Waker> = {
            let mut state = lock_state(&self.state);
            state.holding.retain(|held| held != url);
            state
                .held
                .extract_if(|_, (held, _)| held == url)
                .map(|(_, (_, waker))| waker)
                .collect()
        };
        // Woken outside the lock: a waker may poll at once. A later panic is
        // discarded under containment (ADR-0281), the first raised once every
        // held request is woken.
        let mut first = flui_foundation::panic::FirstPanic::new();
        for waker in wakers {
            first.contain(|| waker.wake());
        }
        first.resume();
    }

    /// Every request sent so far, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<HttpRequest> {
        lock_state(&self.state).requests.clone()
    }

    /// How many requests sent so far were for `url`.
    #[must_use]
    pub fn requests_to(&self, url: &str) -> usize {
        lock_state(&self.state)
            .requests
            .iter()
            .filter(|request| request.url() == url)
            .count()
    }

    /// How many requests were dropped before they were answered.
    #[must_use]
    pub fn abandoned(&self) -> usize {
        lock_state(&self.state).abandoned
    }

    /// The handle a widget acquires.
    #[must_use]
    pub fn handle(&self) -> Http {
        Http::new(Arc::new(self.clone()))
    }

    /// Register this client as the provider of [`HttpCapability`].
    pub fn register(&self, registrar: &mut CapabilityRegistrar) {
        let http = self.handle();
        registrar.provide::<HttpCapability>(move |_: &ProviderContext<'_>| Ok(http.clone()));
    }
}

impl HttpClient for ScriptedHttp {
    fn authority(&self) -> HttpAuthority {
        HttpAuthority::of(&self.state)
    }

    fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
        let url = request.url().to_owned();
        let limit = request.body_limit();
        let id = {
            let mut state = lock_state(&self.state);
            state.requests.push(request);
            state.next_request += 1;
            state.next_request
        };
        let mut pending = Pending {
            state: Arc::clone(&self.state),
            id,
            answered: false,
        };
        Box::pin(std::future::poll_fn(move |ctx| {
            // Cloned, and every replaced or unused waker dropped, outside the
            // lock: a waker's clone and drop run its owner's code, which may
            // call back here.
            let waker = ctx.waker().clone();
            let mut state = lock_state(&pending.state);
            if state.holding.contains(&url) {
                let replaced = state.held.insert(pending.id, (url.clone(), waker));
                drop(state);
                drop(replaced);
                return Poll::Pending;
            }
            pending.answered = true;
            let registered = state.held.remove(&pending.id);
            let scripted = state.scripts.get(&url).cloned();
            drop(state);
            drop((waker, registered));
            Poll::Ready(match scripted {
                Some(ScriptedResponse::Respond { status, body }) => Ok(HttpResponse::new(
                    status,
                    Headers::new(),
                    ResponseBody::new(Script(Some(body)), limit),
                )),
                Some(ScriptedResponse::Fail(error)) => Err(error),
                None => Err(HttpError::Connect {
                    detail: format!("no scripted response for {url}"),
                }),
            })
        }))
    }
}

/// A scripted body, in one chunk.
struct Script(Option<Vec<u8>>);

impl crate::http::BodySource for Script {
    fn poll_chunk(
        mut self: std::pin::Pin<&mut Self>,
        _ctx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<Vec<u8>, HttpError>>> {
        Poll::Ready(self.0.take().filter(|body| !body.is_empty()).map(Ok))
    }
}

/// A sent request: counted as abandoned, and its waker let go, when it is
/// dropped before its answer.
struct Pending {
    state: Arc<Mutex<State>>,
    id: u64,
    answered: bool,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.answered {
            return;
        }
        let registered = {
            let mut state = lock_state(&self.state);
            state.abandoned += 1;
            state.held.remove(&self.id)
        };
        drop(registered);
    }
}

impl fmt::Debug for ScriptedHttp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counted under the lock and written after it: the formatter's writer
        // is the caller's code, which may call back here.
        let (scripts, requests) = {
            let state = lock_state(&self.state);
            (state.scripts.len(), state.requests.len())
        };
        f.debug_struct("ScriptedHttp")
            .field("scripts", &scripts)
            .field("requests", &requests)
            .finish_non_exhaustive()
    }
}
