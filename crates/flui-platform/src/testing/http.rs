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
use std::sync::Arc;
use std::task::{Poll, Waker};

use parking_lot::Mutex;

use crate::capability::{CapabilityRegistrar, ProviderContext};
use crate::http::{
    Headers, Http, HttpCapability, HttpClient, HttpError, HttpFuture, HttpRequest, HttpResponse,
    ResponseBody, StatusCode,
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
    held: HashMap<String, Vec<Waker>>,
    holding: Vec<String>,
    requests: Vec<HttpRequest>,
    abandoned: usize,
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
        self.state.lock().scripts.insert(url.into(), response);
    }

    /// Keep the requests for `url` unanswered until
    /// [`release`](Self::release).
    pub fn hold(&self, url: impl Into<String>) {
        self.state.lock().holding.push(url.into());
    }

    /// Answer the held requests for `url` and every later one.
    pub fn release(&self, url: &str) {
        let wakers = {
            let mut state = self.state.lock();
            state.holding.retain(|held| held != url);
            state.held.remove(url).unwrap_or_default()
        };
        // Woken outside the lock: a waker may poll at once.
        for waker in wakers {
            waker.wake();
        }
    }

    /// Every request sent so far, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<HttpRequest> {
        self.state.lock().requests.clone()
    }

    /// How many requests sent so far were for `url`.
    #[must_use]
    pub fn requests_to(&self, url: &str) -> usize {
        self.state
            .lock()
            .requests
            .iter()
            .filter(|request| request.url() == url)
            .count()
    }

    /// How many requests were dropped before they were answered.
    #[must_use]
    pub fn abandoned(&self) -> usize {
        self.state.lock().abandoned
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
    fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
        let url = request.url().to_owned();
        let limit = request.body_limit();
        self.state.lock().requests.push(request);
        let mut pending = Pending {
            state: Arc::clone(&self.state),
            answered: false,
        };
        Box::pin(std::future::poll_fn(move |cx| {
            let mut state = pending.state.lock();
            if state.holding.contains(&url) {
                state
                    .held
                    .entry(url.clone())
                    .or_default()
                    .push(cx.waker().clone());
                return Poll::Pending;
            }
            pending.answered = true;
            Poll::Ready(match state.scripts.get(&url) {
                Some(ScriptedResponse::Respond { status, body }) => Ok(HttpResponse::new(
                    *status,
                    Headers::new(),
                    ResponseBody::new(Script(Some(body.clone())), limit),
                )),
                Some(ScriptedResponse::Fail(error)) => Err(error.clone()),
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
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<Vec<u8>, HttpError>>> {
        Poll::Ready(self.0.take().filter(|body| !body.is_empty()).map(Ok))
    }
}

/// Counts a request dropped before its answer.
struct Pending {
    state: Arc<Mutex<State>>,
    answered: bool,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.answered {
            self.state.lock().abandoned += 1;
        }
    }
}

impl fmt::Debug for ScriptedHttp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.lock();
        f.debug_struct("ScriptedHttp")
            .field("scripts", &state.scripts.len())
            .field("requests", &state.requests.len())
            .finish_non_exhaustive()
    }
}
