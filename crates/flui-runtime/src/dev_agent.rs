//! The host half of the development-agent seam (ADR-0095 §3).
//!
//! A [`DevAgentHook`] is user code a development tool supplies; this module
//! is the only code that calls it, for every host: `flui-app`'s desktop and
//! iOS runners and `flui-testing`'s headless realm drive the same
//! [`DevAgentHost`], so the containment below is written and tested once.
//! It names no transport: the endpoint, its framing and its credentials
//! belong to the tool.
//!
//! A host [attaches](DevAgentHost::attach) the hook once per event loop,
//! before the first window, and keeps the returned [`DevAgentAttachment`]
//! until the loop ends; dropping it detaches the hook. For each window with
//! content it [publishes](DevAgentHost::publish) the window, or, when the
//! realm has to be handed over between vending and publishing, it
//! [vends](DevAgentHost::vend) the window's [`AgentWindow`] first and
//! [hands it over](DevAgentHost::window_opened) once the window is
//! installed. Nothing is vended while the hook is not attached, so a hook
//! that failed to attach costs no semantics work.
//!
//! # Failure containment
//!
//! Each call runs with the hook lent out of its slot and no lock held, so a
//! hook that re-enters the host finds the slot empty and the nested call does
//! nothing instead of deadlocking. A call that panics is logged, and the hook
//! is dropped — inside its own containment, because its `Drop` may panic too
//! — and never called again; an [`AgentWindow`] it was being handed is
//! dropped with it. The panic's payload is forgotten rather than dropped, for
//! the same reason. The caller continues, and the realm is untouched.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use flui_foundation::PresentationId;
use flui_view::dev_agent::{AgentWindow, DevAgentHook};
use parking_lot::Mutex;

use crate::ui_realm::UiRealm;

/// A development-agent hook and its attachment state, shared by every clone.
///
/// `Clone` shares the one hook, so every window a host opens with clones of
/// the same configuration is handed to the same hook.
#[derive(Clone)]
pub struct DevAgentHost(Arc<Mutex<Slot>>);

struct Slot {
    /// `None` while a call has the hook lent out, and for good once it
    /// panicked.
    hook: Option<Box<dyn DevAgentHook>>,
    /// Between a successful `attach` and its `detach`.
    attached: bool,
    /// A `detach` arrived while the hook was lent; the lender runs it when
    /// the call returns.
    detach_owed: bool,
    /// The hook panicked and was dropped.
    disabled: bool,
}

/// What lending the hook for one call produced.
enum Lent<R> {
    /// No hook: a call already has it, or it panicked earlier.
    Absent,
    /// The call returned.
    Returned(R),
    /// The call panicked; the hook is gone.
    Panicked,
}

impl DevAgentHost {
    /// Take ownership of `hook`. Nothing is called until [`Self::attach`].
    #[must_use]
    pub fn new(hook: impl DevAgentHook) -> Self {
        Self(Arc::new(Mutex::new(Slot {
            hook: Some(Box::new(hook)),
            attached: false,
            detach_owed: false,
            disabled: false,
        })))
    }

    /// Attach the hook for this event loop. The returned attachment detaches
    /// it when dropped, so it must outlive the loop.
    ///
    /// `None` when the hook already panicked, when it is already attached to
    /// a live loop (refused with a warning: the hook's contract is one attach
    /// per detach), or when `attach` panicked.
    #[must_use = "dropping the attachment detaches the hook"]
    pub fn attach(&self) -> Option<DevAgentAttachment> {
        {
            let mut slot = self.0.lock();
            if slot.disabled || slot.hook.is_none() {
                return None;
            }
            if slot.attached {
                drop(slot);
                tracing::warn!(
                    "development agent hook is already attached to a running loop; this loop \
                     serves no agent"
                );
                return None;
            }
            slot.attached = true;
        }
        match self.lend("attach", DevAgentHook::attach) {
            Lent::Returned(()) => Some(DevAgentAttachment { host: self.clone() }),
            Lent::Absent | Lent::Panicked => {
                self.0.lock().attached = false;
                None
            }
        }
    }

    /// Whether the hook is attached and has not panicked.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        let slot = self.0.lock();
        slot.attached && !slot.disabled
    }

    /// The [`AgentWindow`] for `presentation`, vended only while the hook is
    /// attached: see [`UiRealm::dev_agent_window`].
    #[must_use]
    pub fn vend(&self, realm: &UiRealm, presentation: PresentationId) -> Option<AgentWindow> {
        if !self.is_attached() {
            return None;
        }
        realm.dev_agent_window(presentation)
    }

    /// Hand `window` to the hook. Does nothing when the hook is not attached
    /// or has panicked; the window is then dropped.
    pub fn window_opened(&self, window: AgentWindow) {
        if !self.is_attached() {
            return;
        }
        let _ = self.lend("window_opened", |hook| hook.window_opened(window));
    }

    /// Vend `presentation`'s window and hand it to the hook at once, for a
    /// host whose window is installed by the time it publishes.
    pub fn publish(&self, realm: &UiRealm, presentation: PresentationId) {
        if let Some(window) = self.vend(realm, presentation) {
            self.window_opened(window);
        }
    }

    /// Run `call` on the hook with the hook out of the slot and no lock
    /// held. See the module docs for why.
    fn lend<R>(
        &self,
        what: &'static str,
        call: impl FnOnce(&mut dyn DevAgentHook) -> R,
    ) -> Lent<R> {
        let Some(mut hook) = self.0.lock().hook.take() else {
            return Lent::Absent;
        };
        match catch_unwind(AssertUnwindSafe(|| call(hook.as_mut()))) {
            Ok(value) => {
                let owed = std::mem::take(&mut self.0.lock().detach_owed);
                if owed && catch_unwind(AssertUnwindSafe(|| hook.detach())).is_err() {
                    self.disable("detach", hook);
                    return Lent::Returned(value);
                }
                self.0.lock().hook = Some(hook);
                Lent::Returned(value)
            }
            Err(payload) => {
                // The payload's own `Drop` may panic; never run it.
                std::mem::forget(payload);
                self.disable(what, hook);
                Lent::Panicked
            }
        }
    }

    /// Log a hook's panic and drop the hook without letting its `Drop`
    /// unwind into the caller.
    fn disable(&self, what: &'static str, hook: Box<dyn DevAgentHook>) {
        self.0.lock().disabled = true;
        contain(|| {
            tracing::error!(
                call = what,
                "development agent hook panicked; no agent is served until the app restarts"
            );
        });
        contain(|| drop(hook));
    }

    /// The loop ended: detach the hook.
    fn detach(&self) {
        {
            let mut slot = self.0.lock();
            if !slot.attached {
                return;
            }
            slot.attached = false;
        }
        if let Lent::Absent = self.lend("detach", DevAgentHook::detach) {
            // A call has the hook lent (or it is gone for good, where this
            // flag is never read): that call's lender runs the detach.
            self.0.lock().detach_owed = true;
        }
    }
}

impl fmt::Debug for DevAgentHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slot = self.0.lock();
        f.debug_struct("DevAgentHost")
            .field("attached", &slot.attached)
            .field("disabled", &slot.disabled)
            .finish_non_exhaustive()
    }
}

/// Keeps a [`DevAgentHost`]'s hook attached; dropping it detaches the hook.
#[must_use = "dropping the attachment detaches the hook"]
pub struct DevAgentAttachment {
    host: DevAgentHost,
}

impl fmt::Debug for DevAgentAttachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevAgentAttachment")
            .field("host", &self.host)
            .finish()
    }
}

impl Drop for DevAgentAttachment {
    fn drop(&mut self) {
        self.host.detach();
    }
}

/// Run `body`, swallowing a panic without running its payload's `Drop`,
/// which may panic too.
fn contain(body: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(body)) {
        std::mem::forget(payload);
    }
}

#[cfg(test)]
mod tests;
