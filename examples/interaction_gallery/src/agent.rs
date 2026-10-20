//! The page's development agent, in a build with the `agent` feature: a
//! browser driver reads the gallery's semantics tree, the same tree UI
//! Automation reads on Windows, from the page itself.
//!
//! Once the window opens, `globalThis.__flui_agent.call(line)` takes one
//! request line and returns a promise of its reply line, in the shape of
//! the desktop agent's replies with BiDi's method naming:
//!
//! | Request | Reply |
//! |---|---|
//! | `{"id":1,"method":"semantics.read","params":<ReadQuery>}` | `{"id":1,"result":<Tree>}` |
//!
//! A failure answers `{"id":n,"error":{"code","message","retry"}}` with
//! ADR-0080's codes. The read is answered at the owner's next drain, a frame
//! boundary, so the promise settles on a later turn of the page's event
//! loop; it never blocks the page's one thread. A page that loads the
//! gallery without the feature has no `__flui_agent`.

use std::time::Duration;

use flui::view::dev_agent::{AgentFault, AgentWindow, DevAgentHook};
use flui_protocol::{ErrorCode, ReadQuery, Retry};
use js_sys::{Function, Object, Promise, Reflect};
use serde_json::{Value, json};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::prelude::{Closure, JsValue};
use wasm_bindgen_futures::{JsFuture, future_to_promise};

/// How long a read waits for the owner before it answers `timeout`.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);
/// Between two looks at an unanswered read: about a frame.
const POLL_MS: i32 = 16;

/// Serves the page's window to a driver through `globalThis.__flui_agent`.
#[derive(Debug, Default)]
pub(crate) struct PageAgent;

impl DevAgentHook for PageAgent {
    fn attach(&mut self) -> bool {
        true
    }

    fn window_opened(&mut self, window: AgentWindow) {
        let call = Closure::<dyn Fn(String) -> Promise>::new(move |line: String| {
            let window = window.clone();
            future_to_promise(async move {
                Ok(JsValue::from_str(&answer(&window, &line).await.to_string()))
            })
        });
        let agent = Object::new();
        let installed =
            Reflect::set(&agent, &JsValue::from_str("call"), call.as_ref()).and_then(|_| {
                Reflect::set(
                    &js_sys::global(),
                    &JsValue::from_str("__flui_agent"),
                    &agent,
                )
            });
        // A page that refuses the global serves no driver, which then finds
        // no `__flui_agent` and says so.
        if installed.is_err() {
            return;
        }
        // The page keeps the function for as long as it lives; so does the
        // window it reads, which holds the window weakly.
        call.forget();
    }
}

/// The reply line to one request line.
async fn answer(window: &AgentWindow, line: &str) -> Value {
    let Ok(request) = serde_json::from_str::<Value>(line) else {
        return failure(
            Value::Null,
            ErrorCode::InvalidArgument,
            Retry::Never,
            "not a JSON request",
        );
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match request.get("method").and_then(Value::as_str) {
        Some("semantics.read") => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let Ok(query) = serde_json::from_value::<ReadQuery>(params) else {
                return failure(
                    id,
                    ErrorCode::InvalidArgument,
                    Retry::Never,
                    "params is not a read query",
                );
            };
            read(window, id, query).await
        }
        _ => failure(
            id,
            ErrorCode::NotSupported,
            Retry::Never,
            "this page serves semantics.read only",
        ),
    }
}

async fn read(window: &AgentWindow, id: Value, query: ReadQuery) -> Value {
    let mut pending = match window.read(query) {
        Ok(pending) => pending,
        Err(fault) => return fault_reply(id, &fault),
    };
    let polls = ANSWER_WITHIN.as_millis() / u128::from(POLL_MS.unsigned_abs());
    for _ in 0..polls {
        match pending.try_take() {
            Some(Ok(tree)) => return json!({ "id": id, "result": tree }),
            Some(Err(fault)) => return fault_reply(id, &fault),
            None => later(POLL_MS).await,
        }
    }
    failure(
        id,
        ErrorCode::Timeout,
        Retry::Soon,
        "the owner did not answer the read",
    )
}

fn fault_reply(id: Value, fault: &AgentFault) -> Value {
    failure(id, fault.code(), fault.retry(), fault.message())
}

fn failure(id: Value, code: ErrorCode, retry: Retry, message: &str) -> Value {
    json!({ "id": id, "error": { "code": code, "message": message, "retry": retry } })
}

/// Resolves after `ms` milliseconds, through the page's own timer.
async fn later(ms: i32) {
    let wait = Promise::new(&mut |resolve, _reject| {
        let timer = Reflect::get(&js_sys::global(), &JsValue::from_str("setTimeout"))
            .ok()
            .and_then(|timer| timer.dyn_into::<Function>().ok());
        match timer {
            Some(timer) => {
                let _ = timer.call2(&JsValue::NULL, &resolve, &JsValue::from(ms));
            }
            // No timer: settle at once, so a read still ends at its bound.
            None => {
                let _ = resolve.call0(&JsValue::NULL);
            }
        }
    });
    let _ = JsFuture::from(wait).await;
}
