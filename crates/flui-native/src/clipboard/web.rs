//! The web host's clipboard: the Navigator Clipboard API.
//!
//! A read asks `navigator.clipboard.readText()` and answers when its promise
//! settles. The browser may refuse it (no user gesture, a denied permission,
//! an insecure context, no Clipboard API at all); the read then answers with
//! the text of this page's last write the clipboard took
//! (`clipboard::page`), so copy and paste inside the application keep
//! working where the system clipboard is out of reach. Before the page wrote
//! anything, a refusal answers its reason: [`ClipboardError::Denied`] for a
//! permission, [`ClipboardError::Unavailable`] otherwise. A write goes to
//! `navigator.clipboard.writeText()` and answers when the browser's promise
//! settles; the page keeps its text only if the clipboard took it, and where
//! the page has no Clipboard API its own clipboard takes every write.

// The page has one thread, so the clipboard claims `Send`/`Sync` by hand: the
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_platform::{
    ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten, Concealment,
    capability::Resolver,
};
use wasm_bindgen_futures::JsFuture;

use crate::clipboard::page::PageClipboard;
use crate::host::{NativeClipboard, admit_text, host_text};

pub struct WebClipboard {
    /// What this page holds: the answer to a read the browser refuses, and
    /// the clipboard where the Navigator Clipboard API is missing.
    page: Arc<PageClipboard>,
    /// Whether a concealed write has warned that it was an ordinary one.
    degraded: AtomicBool,
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
// SAFETY: one thread, as for `Send` above: no second thread can share it.
unsafe impl Sync for WebClipboard {}

impl WebClipboard {
    pub fn new() -> Self {
        Self {
            page: Arc::default(),
            degraded: AtomicBool::new(false),
        }
    }
}

/// The page's Navigator Clipboard API, or `None` where it lacks `method`:
/// an insecure context or an older browser leaves `navigator.clipboard`
/// undefined, and calling into it would throw before any promise exists.
fn system_clipboard(method: &str) -> Option<web_sys::Clipboard> {
    let navigator = web_sys::window()?.navigator();
    let clipboard =
        js_sys::Reflect::get(&navigator, &wasm_bindgen::JsValue::from_str("clipboard")).ok()?;
    if clipboard.is_undefined() || clipboard.is_null() {
        return None;
    }
    // Firefox long had the object without `readText`.
    let callable =
        js_sys::Reflect::get(&clipboard, &wasm_bindgen::JsValue::from_str(method)).ok()?;
    if !callable.is_function() {
        return None;
    }
    Some(wasm_bindgen::JsCast::unchecked_into(clipboard))
}

impl NativeClipboard for WebClipboard {
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        let page = Arc::clone(&self.page);
        let Some(clipboard) = system_clipboard("readText") else {
            let text = page.refused_read(ClipboardError::Unavailable);
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(text);
            return;
        };
        let promise = clipboard.read_text();
        wasm_bindgen_futures::spawn_local(async move {
            let text = match JsFuture::from(promise).await {
                Ok(value) => wasm_bindgen::JsCast::dyn_ref::<js_sys::JsString>(&value).map_or(
                    Ok(None),
                    // Measured in UTF-16 units before the copy into Rust.
                    |text| {
                        let units = usize::try_from(text.length()).unwrap_or(usize::MAX);
                        admit_text(units)?;
                        host_text(String::from(text))
                    },
                ),
                Err(refusal) => {
                    tracing::debug!(
                        ?refusal,
                        "the browser refused a clipboard read; answering this page's last write"
                    );
                    page.refused_read(refusal_error(&refusal))
                }
            };
            let _ = answer.resolve(text);
        });
    }

    /// Answered when the browser's promise settles: `Ok` once the system
    /// clipboard holds the text, the refusal otherwise
    /// ([`ClipboardError::Denied`] for a permission), which leaves the page's
    /// own clipboard as it was. Where the page has no Clipboard API, its own
    /// clipboard is the clipboard and the write answers `Ok` at once.
    fn write(&self, write: ClipboardWrite, answer: Resolver<ClipboardWritten>) {
        if write.is_concealed() {
            degraded_once(&self.degraded);
        }
        let text = write.into_text();
        let promise = system_clipboard("writeText").map(|clipboard| clipboard.write_text(&text));
        let pending = self.page.begin(Arc::from(text));
        let Some(promise) = promise else {
            // A retired owner has nobody left to answer.
            let _ = answer.resolve(self.page.settle(pending, Ok(())));
            return;
        };
        let page = Arc::clone(&self.page);
        wasm_bindgen_futures::spawn_local(async move {
            let written = match JsFuture::from(promise).await {
                Ok(_) => Ok(()),
                Err(refusal) => {
                    tracing::debug!(?refusal, "the browser refused a clipboard write");
                    Err(refusal_error(&refusal))
                }
            };
            let _ = answer.resolve(page.settle(pending, written));
        });
    }

    /// The Clipboard API has no way to mark an entry: a concealed write is
    /// an ordinary one, kept by whatever clipboard history the system has.
    fn concealment(&self) -> Concealment {
        Concealment::Unavailable
    }
}

/// Warn, once per clipboard and in a debug build, that a concealed write was
/// written as an ordinary one (ADR-0152 §6).
fn degraded_once(degraded: &AtomicBool) {
    if cfg!(debug_assertions) && !degraded.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            "a concealed clipboard write is an ordinary one on the web: \
             the Clipboard API cannot keep it out of the clipboard's history"
        );
    }
}

/// Why the browser refused a read: a `NotAllowedError` is the user's or the
/// page's permission (no gesture, a denied prompt, a document without focus).
fn refusal_error(refusal: &wasm_bindgen::JsValue) -> ClipboardError {
    let name = js_sys::Reflect::get(refusal, &wasm_bindgen::JsValue::from_str("name"))
        .ok()
        .and_then(|name| name.as_string());
    refusal_from_name(name.as_deref())
}

/// The error a `DOMException` name stands for.
fn refusal_from_name(name: Option<&str>) -> ClipboardError {
    match name {
        Some("NotAllowedError" | "SecurityError") => ClipboardError::Denied,
        _ => ClipboardError::Unavailable,
    }
}
