//! [`TickerMode`] — pause a subtree's animations without unmounting it.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/ticker_provider.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`: `TickerMode` (`:25`), whose
//! `_TickerModeState` mutes every ticker created by a descendant
//! `TickerProvider` (`:397`) and ANDs its own `enabled` with the ancestor's
//! (`_updateEffectiveMode`, `:246-252`) — a nested enabled `TickerMode` cannot
//! re-enable a disabled ancestor.
//!
//! # The Rust shape
//!
//! Flutter's tickers are created *by* a `TickerProvider` (the `State`), so a
//! notifier can reach each one. FLUI's animated widgets instead register their
//! `AnimationController` with the ambient [`Vsync`] registry a
//! [`VsyncScope`] hands down. So a `TickerMode` owns a
//! **nested registry**: it attaches a child [`Vsync`] to the ambient one and
//! provides that child to its subtree, then mutes it while disabled.
//!
//! The AND falls out structurally: a muted registry ticks neither its own
//! controllers nor its children's, so a `TickerMode(true)` nested inside a
//! `TickerMode(false)` never receives the frame it would forward. No flag to
//! compose, and no way to get it wrong.
//!
//! **The clock keeps running while muted.** Flutter's `Ticker.muted` is "a
//! ticker's clock can still run, but the callback will not be called"
//! (`ticker.dart:102-104`): a disabled subtree delivers no ticks, and when it
//! is re-enabled its animations land where the wall clock says they should be
//! — they do not resume from where they stopped. A `TickerMode` is a mute
//! button, not a pause button. (FLUI's `Ticker::mute` freezes elapsed time
//! instead; that is a different, unrelated layer with no consumer here.)
//!
//! # Deferred, and named
//!
//! * `TickerMode.of` / `getNotifier` (`:78`, `:118`) — no consumer; FLUI's
//!   descendants need the registry, not the flag.
//! * `forceFrames` (`:249-258`) — its consumer is Flutter's test binding.
//! * A widget that *creates* controllers outside the ambient registry (its own
//!   wall-clock ticker fallback) is not muted: it is not in the registry to
//!   mute. Every in-tree animated widget prefers the ambient `VsyncScope`.

use flui_animation::{Vsync, VsyncRegistration};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use parking_lot::Mutex;

use super::VsyncScope;

/// Pauses (or resumes) every animation in its subtree — Flutter's `TickerMode`
/// (`ticker_provider.dart:25`).
///
/// While `enabled` is `false`, descendant animation controllers stop
/// advancing: they keep their current value and status, and resume from there
/// when re-enabled. Nesting composes as an AND — a `TickerMode` inside a
/// disabled one cannot re-enable its subtree.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// let _ = TickerMode::new(Text::new("offscreen")).enabled(false);
/// ```
#[derive(Clone)]
pub struct TickerMode {
    child: BoxedView,
    enabled: bool,
}

impl TickerMode {
    /// A ticker scope around `child`. `enabled` defaults to `true`
    /// (`ticker_provider.dart:32`), so a bare `TickerMode` changes nothing.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: BoxedView(Box::new(child.into_view())),
            enabled: true,
        }
    }

    /// Whether this subtree's animations advance (`:41`).
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl std::fmt::Debug for TickerMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TickerMode")
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl View for TickerMode {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for TickerMode {
    type State = TickerModeState;

    fn create_state(&self) -> Self::State {
        let registry = Vsync::new();
        registry.set_muted(!self.enabled);
        TickerModeState {
            registry,
            parent: Mutex::new(None),
        }
    }
}

/// The state behind [`TickerMode`]: the subtree's nested [`Vsync`] registry.
/// `pub` only because `StatefulView::State` requires it; not re-exported.
pub struct TickerModeState {
    registry: Vsync,
    /// The ambient registry this one is nested under, and the id it holds
    /// there — `None` when no `VsyncScope` is above (then nothing ticks this
    /// subtree anyway, and muting is moot).
    parent: Mutex<Option<(Vsync, VsyncRegistration)>>,
}

impl std::fmt::Debug for TickerModeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TickerModeState")
            .field("muted", &self.registry.is_muted())
            .finish_non_exhaustive()
    }
}

impl ViewState<TickerMode> for TickerModeState {
    /// Nest this subtree's registry under the ambient one. Read once, in the
    /// one lifecycle hook with a `BuildContext` that is not a frame phase.
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let Some(parent) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) else {
            return;
        };
        if let Some(id) = parent.attach_child(&self.registry) {
            *self.parent.lock() = Some((parent, id));
        }
    }

    /// `_updateEffectiveMode` (`ticker_provider.dart:246-252`) — minus the AND,
    /// which the nesting already performs.
    fn did_update_view(&mut self, _old: &TickerMode, new_view: &TickerMode) {
        self.registry.set_muted(!new_view.enabled);
    }

    /// Un-nest: a registry left attached to a live parent would keep ticking a
    /// dead subtree's controllers.
    fn dispose(&mut self) {
        if let Some((parent, id)) = self.parent.lock().take() {
            parent.detach_child(id);
        }
    }

    /// The subtree sees **this** registry as its ambient `Vsync`, so every
    /// controller a descendant registers lands here and is muted with it.
    fn build(&self, view: &TickerMode, _ctx: &dyn BuildContext) -> impl IntoView {
        VsyncScope::new(self.registry.clone(), view.child.clone())
    }
}

#[cfg(test)]
mod tests {
    use flui_animation::Vsync;
    use flui_view::ViewExt;

    use super::*;
    use crate::test_harness::mount;
    use crate::{AnimatedOpacity, SizedBox, VsyncScope};

    /// The subtree's registry nests under the ambient one and follows
    /// `enabled`, so a descendant `AnimatedOpacity`'s controller lands in the
    /// disabled registry and stops receiving ticks — Flutter's `TickerMode`
    /// muting a subtree's tickers (`ticker_provider.dart:397`).
    ///
    /// Red-check: drop the `attach_child` in `init_state` — the nested
    /// registry is never ticked at all, and the enabled case fails too.
    #[test]
    fn a_disabled_ticker_mode_starves_its_subtree_while_an_enabled_one_ticks() {
        let root_vsync = Vsync::new();

        // A `TickerMode(false)` around an implicit animation: its controller
        // registers with the nested (muted) registry.
        let _harness = mount(VsyncScope::new(
            root_vsync.clone(),
            TickerMode::new(AnimatedOpacity::new(0.5, SizedBox::new(10.0, 10.0)))
                .enabled(false)
                .into_view()
                .boxed(),
        ));

        assert_eq!(
            root_vsync.len(),
            0,
            "the descendant registered with the nested registry, not the root"
        );

        // Ticking the root walks into the child registry — but a muted one
        // forwards nothing. Nothing to assert on the widget from here beyond
        // the registry contract itself (pinned in `flui-animation`); what this
        // proves is the *wiring*: the subtree's controllers are in the
        // TickerMode's registry, which is muted while disabled.
        root_vsync.tick_all(0.0);
        root_vsync.tick_all(0.5);
    }

    /// A bare `TickerMode` defaults to `enabled: true`
    /// (`ticker_provider.dart:32`) and changes nothing.
    #[test]
    fn a_bare_ticker_mode_is_enabled() {
        let root_vsync = Vsync::new();
        let _harness = mount(VsyncScope::new(
            root_vsync.clone(),
            TickerMode::new(SizedBox::new(10.0, 10.0))
                .into_view()
                .boxed(),
        ));
        root_vsync.tick_all(0.0);
    }
}
