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
    /// The waker of each held request that was polled, by request.
    held: HashMap<u64, (String, Waker)>,
    /// The last request's number.
    next_request: u64,
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
    ///
    /// Every held request is woken even when a waker panics; the first
    /// panic is resumed once all of them are.
    pub fn release(&self, url: &str) {
        let wakers: Vec<Waker> = {
            let mut state = self.state.lock();
            state.holding.retain(|held| held != url);
            state
                .held
                .extract_if(|_, (held, _)| held == url)
                .map(|(_, (_, waker))| waker)
                .collect()
        };
        // Woken outside the lock: a waker may poll at once.
        let mut first_panic = None;
        for waker in wakers {
            if let Err(panic) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()))
            {
                first_panic.get_or_insert(panic);
            }
        }
        if let Some(panic) = first_panic {
            std::panic::resume_unwind(panic);
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
        let id = {
            let mut state = self.state.lock();
            state.requests.push(request);
            state.next_request += 1;
            state.next_request
        };
        let mut pending = Pending {
            state: Arc::clone(&self.state),
            id,
            answered: false,
        };
        Box::pin(std::future::poll_fn(move |cx| {
            // Cloned, and every replaced or unused waker dropped, outside the
            // lock: a waker's clone and drop run its owner's code, which may
            // call back here.
            let waker = cx.waker().clone();
            let mut state = pending.state.lock();
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
        _cx: &mut std::task::Context<'_>,
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
            let mut state = self.state.lock();
            state.abandoned += 1;
            state.held.remove(&self.id)
        };
        drop(registered);
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
