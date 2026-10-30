//! The web host's clipboard: the Navigator Clipboard API.
//!
//! A read asks `navigator.clipboard.readText()` and answers when its promise
//! settles. The browser may refuse it (no user gesture, a denied permission,
//! an insecure context, no Clipboard API at all); the read then answers with
//! what this page last wrote, so copy and paste inside the application keep
//! working where the system clipboard is out of reach. A write goes to
//! `navigator.clipboard.writeText()` and to that fallback.

// The page has one thread, so the clipboard claims `Send`/`Sync` by hand: the
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::sync::Arc;

use flui_platform::capability::Resolver;
use parking_lot::Mutex;
use wasm_bindgen_futures::JsFuture;

use crate::clipboard::NativeClipboard;

pub struct WebClipboard {
    /// What this page last wrote: the answer to a read the browser refuses,
    /// and the clipboard where the Navigator Clipboard API is missing.
    fallback: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for WebClipboard {
    // Hand-written: the cached text may be private to the user.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebClipboard").finish_non_exhaustive()
    }
}

// SAFETY: the page has one thread (the module refuses `+atomics`), so the value
// is never reached from another.
unsafe impl Send for WebClipboard {}
unsafe impl Sync for WebClipboard {}

impl WebClipboard {
    pub fn new() -> Self {
        Self {
            fallback: Arc::new(Mutex::new(None)),
        }
    }
}

impl NativeClipboard for WebClipboard {
    fn read_text(&self, answer: Resolver<Option<String>>) {
        let fallback = Arc::clone(&self.fallback);
        let Some(window) = web_sys::window() else {
            let text = fallback.lock().clone();
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(text);
            return;
        };
        let promise = window.navigator().clipboard().read_text();
        wasm_bindgen_futures::spawn_local(async move {
            let text = match JsFuture::from(promise).await {
                Ok(value) => value.as_string(),
                Err(refusal) => {
                    tracing::debug!(
                        ?refusal,
                        "the browser refused a clipboard read; answering this page's last write"
                    );
                    fallback.lock().clone()
                }
            };
            let _ = answer.resolve(text);
        });
    }

    fn write_text(&self, text: String) {
        if let Some(window) = web_sys::window() {
            // The promise settles later; a refused write still leaves the
            // fallback, which the next refused read answers.
            let _ = window.navigator().clipboard().write_text(&text);
        }
        *self.fallback.lock() = Some(text);
    }
}
