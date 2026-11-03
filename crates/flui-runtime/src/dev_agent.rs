//! The host half of the development-agent seam (ADR-0095 §3, ADR-0288 §7).
//!
//! A [`DevAgentHook`] is user code a development tool supplies; this module
//! is the only code that calls it, for every host: `flui-app`'s runners and
//! `flui-testing`'s headless UI runtime drive the same [`DevAgentHost`], so
//! the containment below is written and tested once. It names no transport:
//! the endpoint, its framing and its credentials belong to the tool.
//!
//! An event loop owns its [`DevAgentHost`]. It [attaches](DevAgentHost::attach)
//! the hooks it took when it starts, before its first window, and drops the
//! host when it ends, which detaches them. For each window with content it
//! [publishes](DevAgentHost::publish) the window, or, when the UI runtime has
//! to be handed over between vending and publishing, it
//! [vends](DevAgentHost::vend) the window's [`AgentWindow`] first and
//! [hands it over](DevAgentHost::window_opened) once the window is installed.
//! Nothing is vended while no hook serves, so a hook that does not serve, or
//! failed to attach, costs no semantics work; a window's semantics work lasts
//! only while a hook keeps its [`AgentWindow`], so a hook that detached or
//! panicked ends it too.
//!
//! # Failure containment
//!
//! The host is a plain value on its loop's thread and every call that runs a
//! hook takes `&mut self`: a hook has no way to reach the host, so no call
//! re-enters it and no lock is held while a hook runs. A hook call that
//! panics is logged, and that hook is dropped, inside its own containment
//! because its `Drop` may panic too, and never called again; the window it was
//! being handed goes with it, and the other hooks keep being served. The
//! panic's payload is discarded under containment (ADR-0281 §3). The caller
//! continues, and the UI runtime is untouched. Dropping the host detaches
//! every hook it keeps, then drops each, each contained, so a `Drop` that
//! panics never escapes a loop's teardown or turns an unwind into an abort.

use std::fmt;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};

use flui_foundation::PresentationId;
use flui_view::dev_agent::{AgentWindow, DevAgentHook};

use crate::ui_runtime::UiRuntime;

/// The development-agent hooks one event loop serves, owned by that loop.
///
/// `!Send + !Sync`: it stays on the thread whose loop attached the hooks,
/// the owner thread every hook call is made on, so the detach its drop runs
/// is made there too.
#[must_use = "dropping the host detaches its hooks"]
pub struct DevAgentHost {
    /// The hooks that answered they serve and have not panicked since, in
    /// the order they were attached.
    hooks: Vec<Box<dyn DevAgentHook>>,
    /// `*const ()` is neither `Send` nor `Sync`.
    _owner_affine: PhantomData<*const ()>,
}

impl DevAgentHost {
    /// Attach `hooks` for this loop, in order.
    ///
    /// A hook that answers it does not serve is dropped without a detach, and
    /// one whose `attach` panics is dropped too; neither is handed a window.
    /// Every hook kept is detached once, when the host drops.
    pub fn attach(hooks: impl IntoIterator<Item = Box<dyn DevAgentHook>>) -> Self {
        let mut serving = Vec::new();
        for mut hook in hooks {
            match contained(|| hook.attach()) {
                Some(true) => serving.push(hook),
                Some(false) => contain(|| drop(hook)),
                None => disable("attach", hook),
            }
        }
        Self {
            hooks: serving,
            _owner_affine: PhantomData,
        }
    }

    /// Whether a hook serves: one attached and has not panicked since.
    #[must_use]
    pub fn is_serving(&self) -> bool {
        !self.hooks.is_empty()
    }

    /// The [`AgentWindow`] for `presentation`, vended only while a hook
    /// serves: see [`UiRuntime::dev_agent_window`]. Every hook is handed a
    /// clone of the one window, so they share its element handles.
    #[must_use]
    pub fn vend(
        &self,
        ui_runtime: &UiRuntime,
        presentation: PresentationId,
    ) -> Option<AgentWindow> {
        if !self.is_serving() {
            return None;
        }
        ui_runtime.dev_agent_window(presentation)
    }

    /// Hand `window` to every hook, in the order they were attached. Does
    /// nothing when no hook serves; the window is then dropped.
    pub fn window_opened(&mut self, window: AgentWindow) {
        let hooks = std::mem::take(&mut self.hooks);
        let mut kept = Vec::with_capacity(hooks.len());
        for mut hook in hooks {
            let handed = window.clone();
            if contained(|| hook.window_opened(handed)).is_some() {
                kept.push(hook);
            } else {
                disable("window_opened", hook);
            }
        }
        self.hooks = kept;
    }

    /// Vend `presentation`'s window and hand it to the hooks at once, for a
    /// host whose window is installed by the time it publishes.
    pub fn publish(&mut self, ui_runtime: &UiRuntime, presentation: PresentationId) {
        if let Some(window) = self.vend(ui_runtime, presentation) {
            self.window_opened(window);
        }
    }
}

impl Drop for DevAgentHost {
    /// The loop ended, or its owner unwinds: each hook is detached, then
    /// dropped, each inside its own containment.
    fn drop(&mut self) {
        for mut hook in std::mem::take(&mut self.hooks) {
            if contained(|| hook.detach()).is_some() {
                contain(|| drop(hook));
            } else {
                disable("detach", hook);
            }
        }
    }
}

impl fmt::Debug for DevAgentHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DevAgentHost")
            .field("serving", &self.hooks.len())
            .finish_non_exhaustive()
    }
}

/// Log a hook's panic and drop the hook without letting its `Drop` unwind
/// into the caller.
fn disable(what: &'static str, hook: Box<dyn DevAgentHook>) {
    contain(|| {
        tracing::error!(
            call = what,
            "development agent hook panicked; it serves no agent until the app restarts"
        );
    });
    contain(|| drop(hook));
}

/// Run `body`: its value, or `None` when it panicked. The panic's payload is
/// discarded under containment (ADR-0281 §2).
fn contained<R>(body: impl FnOnce() -> R) -> Option<R> {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: an agent request's panic is discarded so the server keeps serving"
    )]
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => Some(value),
        Err(payload) => {
            flui_foundation::panic::discard_payload(payload);
            None
        }
    }
}

/// Run `body`, swallowing a panic as [`contained`] does.
fn contain(body: impl FnOnce()) {
    let _: Option<()> = contained(body);
}

#[cfg(test)]
mod tests;
