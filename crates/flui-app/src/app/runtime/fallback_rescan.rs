//! Asking the host again which faces fall back when the app's locale
//! changes (ADR-0271 §4).
//!
//! The host font feed hands on the collection's one [`FallbackOrder`] when
//! it has run. The thread it ran on keeps it and serves locale requests from
//! the owner: each request asks the host for its answer in that locale and
//! installs it, and the owner is woken when the order changed, so every UI
//! runtime lays out again the text it measured before. Where the feed ran on
//! the owner thread (wasm32, or no thread could start), the order stays with
//! the owner and each request is served there.
//!
//! The order follows the host's first preferred locale. The collection is the
//! app's one, so a window whose app resolves another locale keeps the host's
//! order; a per-paragraph locale in the fallback key is the follow-up
//! (`docs/gaps.md` entry 35).

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use flui_painting::{FallbackOrder, SystemFallback};

/// The host's fallback query: its answer for a locale tag.
pub(crate) type AskHost = Arc<dyn Fn(&str) -> SystemFallback + Send + Sync>;

/// The owner's end of the rescan: where it sends the locale the order should
/// follow.
pub(crate) struct FallbackRescan {
    requests: Sender<String>,
    /// The tag sent last, so an unchanged preference sends nothing.
    sent: RefCell<Option<String>>,
    /// The order and its requests, when the feed ran on the owner thread.
    inline: RefCell<Option<Inline>>,
}

/// An order served on the owner thread.
struct Inline {
    order: FallbackOrder,
    requests: Receiver<String>,
    ask: AskHost,
}

impl FallbackRescan {
    /// The owner's end, and the requests the order's thread serves.
    pub(crate) fn new() -> (Self, Receiver<String>) {
        let (requests, receiver) = channel();
        let rescan = Self {
            requests,
            sent: RefCell::new(None),
            inline: RefCell::new(None),
        };
        (rescan, receiver)
    }

    /// Asks for the order of `tag`, the app's locale now, unless it is the
    /// tag asked for last. Returns whether an order served here changed, so
    /// the caller wakes the owner; a thread that serves the order wakes it
    /// itself.
    pub(crate) fn follow(&self, tag: String) -> bool {
        if self.sent.borrow().as_deref() == Some(tag.as_str()) {
            return false;
        }
        self.sent.replace(Some(tag.clone()));
        if self.requests.send(tag).is_err() {
            tracing::debug!("the host font thread has ended; the fallback order stays as it is");
            return false;
        }
        // Out of its cell while the host is asked, so a request the query
        // makes re-entrantly waits in the channel for the next one.
        let Some(mut inline) = self.inline.take() else {
            return false;
        };
        let latest = inline.requests.try_iter().last();
        let changed = latest.is_some_and(|tag| install(&mut inline.order, &tag, &inline.ask));
        self.inline.replace(Some(inline));
        changed
    }

    /// Keeps `order` here, after a feed that ran on the owner thread, and
    /// serves the requests sent before it landed. Returns whether that
    /// changed the order.
    pub(crate) fn keep_inline(
        &self,
        mut order: FallbackOrder,
        requests: Receiver<String>,
        ask: AskHost,
    ) -> bool {
        let latest = requests.try_iter().last();
        let changed = latest.is_some_and(|tag| install(&mut order, &tag, &ask));
        self.inline.replace(Some(Inline {
            order,
            requests,
            ask,
        }));
        changed
    }
}

/// Serves `requests` with `order` on the thread the feed ran on, until the
/// owner's end is dropped. Of the requests waiting, only the newest is
/// served. `wake` is called after each request that changed the order, or
/// whose install panicked, since the generation rises then too.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn serve(
    mut order: FallbackOrder,
    requests: &Receiver<String>,
    ask: &AskHost,
    wake: &(dyn Fn() + Send + Sync),
) {
    while let Ok(first) = requests.recv() {
        let tag = requests.try_iter().last().unwrap_or(first);
        if install(&mut order, &tag, ask) && catch_unwind(AssertUnwindSafe(wake)).is_err() {
            tracing::error!("the wake after a fallback rescan panicked");
        }
    }
}

/// Installs the host's answer for `tag`, unless the order is already for
/// it. Returns whether the owner has to be told: the order changed, or the
/// install panicked part way. A panic is contained here, so a later request
/// is still served; its payload is leaked rather than dropped, since its
/// destructor is the query's code too.
fn install(order: &mut FallbackOrder, tag: &str, ask: &AskHost) -> bool {
    if order.locale() == tag {
        return false;
    }
    match catch_unwind(AssertUnwindSafe(|| order.set_system_fallback(&ask(tag)))) {
        Ok(changed) => changed,
        Err(payload) => {
            tracing::error!(
                locale = tag,
                "installing the fallback order for a new locale panicked; the lists written before the panic are kept"
            );
            std::mem::forget(payload);
            true
        }
    }
}
