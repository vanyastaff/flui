//! Asking the host again which faces fall back when the app's locale
//! changes (ADR-0271 §4).
//!
//! The host font feed hands on the collection's one [`FallbackOrder`] when
//! it has run. The thread it ran on keeps it and serves the owner's
//! requests: each locale asks the host for its answer in that locale and
//! installs it, and the owner is woken when the order changed, so every UI
//! runtime lays out again the text it measured before. A development
//! restart builds a fresh collection (ADR-0393 §4) and sends its order: the
//! thread serves that one from then on, at the locale the owner last asked
//! for, else the one the replaced order followed. Where the feed ran on the
//! owner thread (wasm32, or no thread could start), the order stays with
//! the owner and each request is served there.
//!
//! The order follows the host's first preferred locale. The collection is the
//! app's one, so a window whose app resolves another locale keeps the host's
//! order; a per-paragraph locale in the fallback key is the follow-up
//! (`docs/gaps.md#locale-edge-layout-direction-per-app-language-system-fonts-and-fallback`).

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use flui_painting::{FallbackOrder, SystemFallback};

/// The host's fallback query: its answer for a locale tag.
pub(crate) type AskHost = Arc<dyn Fn(&str) -> SystemFallback + Send + Sync>;

/// What the owner asks of the thread that serves the order.
pub(crate) enum Request {
    /// Follow this locale tag, the app's locale now.
    Follow(String),
    /// Serve this order, a fresh collection's, in place of the one served,
    /// at the tag the owner sent last (`None` when it sent none).
    #[cfg_attr(
        all(target_arch = "wasm32", not(test)),
        expect(
            dead_code,
            reason = "a development restart runs on the desktop loop alone"
        )
    )]
    Serve {
        order: FallbackOrder,
        follows: Option<String>,
    },
}

impl Request {
    /// The tag a [`Request::Follow`] carries: what a test reads of the
    /// requests the owner sent.
    #[cfg(test)]
    pub(crate) fn into_followed(self) -> Option<String> {
        match self {
            Self::Follow(tag) => Some(tag),
            Self::Serve { .. } => None,
        }
    }
}

/// The owner's end of the rescan: where it sends the locale the order should
/// follow, and the order of a fresh collection.
pub(crate) struct FallbackRescan {
    requests: Sender<Request>,
    /// The tag sent last, so an unchanged preference sends nothing.
    sent: RefCell<Option<String>>,
    /// The order and its requests, when the feed ran on the owner thread.
    inline: RefCell<Option<Inline>>,
}

/// An order served on the owner thread.
struct Inline {
    order: FallbackOrder,
    requests: Receiver<Request>,
    ask: AskHost,
}

impl FallbackRescan {
    /// The owner's end, and the requests the order's thread serves.
    pub(crate) fn new() -> (Self, Receiver<Request>) {
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
        self.send(Request::Follow(tag))
    }

    /// Has `order`, a fresh collection's, served from now on, at the locale
    /// the owner asked for last, or the one the order it replaces followed
    /// when it asked for none. Returns whether an order served here changed,
    /// as [`Self::follow`] does.
    #[cfg_attr(
        all(target_arch = "wasm32", not(test)),
        expect(
            dead_code,
            reason = "a development restart runs on the desktop loop alone"
        )
    )]
    pub(crate) fn replace_order(&self, order: FallbackOrder) -> bool {
        let follows = self.sent.borrow().clone();
        self.send(Request::Serve { order, follows })
    }

    fn send(&self, request: Request) -> bool {
        if self.requests.send(request).is_err() {
            tracing::debug!("the host font thread has ended; the fallback order stays as it is");
            return false;
        }
        // Out of its cell while the host is asked, so a request the query
        // makes re-entrantly waits in the channel for the next one.
        let Some(mut inline) = self.inline.take() else {
            return false;
        };
        let changed = apply(&mut inline.order, inline.requests.try_iter(), &inline.ask);
        self.inline.replace(Some(inline));
        changed
    }

    /// Keeps `order` here, after a feed that ran on the owner thread, and
    /// serves the requests sent before it landed. Returns whether that
    /// changed the order.
    pub(crate) fn keep_inline(
        &self,
        mut order: FallbackOrder,
        requests: Receiver<Request>,
        ask: AskHost,
    ) -> bool {
        let changed = apply(&mut order, requests.try_iter(), &ask);
        self.inline.replace(Some(Inline {
            order,
            requests,
            ask,
        }));
        changed
    }
}

/// Serves `requests` with `order` on the thread the feed ran on, until the
/// owner's end is dropped. Of the requests waiting, all are applied and the
/// newest locale installed once. `wake` is called after each batch that
/// changed the order, or whose install panicked, since the generation rises
/// then too.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn serve(
    mut order: FallbackOrder,
    requests: &Receiver<Request>,
    ask: &AskHost,
    wake: &(dyn Fn() + Send + Sync),
) {
    while let Ok(first) = requests.recv() {
        let waiting = std::iter::once(first).chain(requests.try_iter());
        if apply(&mut order, waiting, ask) {
            contained("the wake after a fallback rescan", wake);
        }
    }
}

/// Applies `requests` to `order` in the order sent: a fresh collection's
/// order replaces it and is served at the tag the owner sent last, or at the
/// locale the replaced one followed when it sent none; a later tag asks for
/// another. The newest locale is then installed once. Returns whether the
/// owner has to be told ([`install`]).
pub(super) fn apply(
    order: &mut FallbackOrder,
    requests: impl Iterator<Item = Request>,
    ask: &AskHost,
) -> bool {
    let mut tag = None;
    for request in requests {
        match request {
            Request::Follow(next) => tag = Some(next),
            Request::Serve {
                order: fresh,
                follows,
            } => {
                // The owner's last tag, even one whose install on the
                // replaced order panicked part way and left its locale behind.
                let replaced = std::mem::replace(order, fresh);
                tag = Some(
                    follows
                        .or(tag)
                        .unwrap_or_else(|| replaced.locale().to_owned()),
                );
            }
        }
    }
    tag.is_some_and(|tag| install(order, &tag, ask))
}

/// Installs the host's answer for `tag`, unless the order is already for
/// it. Returns whether the owner has to be told: the order changed, or the
/// install panicked part way. A panic is contained here, so a later request
/// is still served; its payload is discarded under containment, since its
/// destructor is the query's code too (ADR-0281 §2).
fn install(order: &mut FallbackOrder, tag: &str, ask: &AskHost) -> bool {
    if order.locale() == tag {
        return false;
    }
    contained(
        "installing the fallback order for a new locale (the lists written before the panic are kept)",
        || order.set_system_fallback(&ask(tag)),
    )
    .unwrap_or(true)
}

/// Runs `f`, containing a panic in it: the panic is logged as `what` and its
/// payload is discarded under containment, since the payload's destructor is
/// the panicking code's too (ADR-0281 §2).
pub(crate) fn contained<T>(what: &str, f: impl FnOnce() -> T) -> Option<T> {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a rescan callback's panic is logged and contained"
    )]
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => Some(value),
        Err(payload) => {
            tracing::error!("{what} panicked");
            flui_foundation::panic::discard_payload(payload);
            None
        }
    }
}
