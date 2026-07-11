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

use super::VsyncScope;

/// Pauses (or resumes) every animation in its subtree — Flutter's `TickerMode`
/// (`ticker_provider.dart:25`).
///
/// While `enabled` is `false`, descendant animation controllers receive no
/// ticks. **The clock keeps running** — this is a mute button, not a pause
/// button: a re-enabled subtree lands where the wall clock says it should be,
/// not where it stopped (Flutter's `Ticker.muted`, `ticker.dart:102-104`).
/// Nesting composes as an AND — a `TickerMode` inside a disabled one cannot
/// re-enable its subtree.
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
            parent: None,
        }
    }
}

/// The state behind [`TickerMode`]: the subtree's nested [`Vsync`] registry.
/// `pub` because `StatefulView::State` requires it; re-exported with the rest of
/// the crate's widget states.
pub struct TickerModeState {
    registry: Vsync,
    /// The ambient registry this one is nested under, and the id it holds
    /// there. `None` when no `VsyncScope` is above — then this registry has no
    /// driver, and `build` deliberately does **not** hand it to the subtree.
    parent: Option<(Vsync, VsyncRegistration)>,
}

impl std::fmt::Debug for TickerModeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TickerModeState")
            .field("muted", &self.registry.is_muted())
            .finish_non_exhaustive()
    }
}

impl TickerModeState {
    /// Re-derive the ambient registry and move this one under it. Idempotent:
    /// re-nesting under the same parent is a no-op.
    fn renest(&mut self, ctx: &dyn BuildContext) {
        let ambient = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());
        let unchanged = self
            .parent
            .as_ref()
            .zip(ambient.as_ref())
            .is_some_and(|((held, _), ambient)| held.is_same(ambient));
        if unchanged {
            return;
        }
        self.unnest();
        let Some(parent) = ambient else {
            return;
        };
        if let Some(id) = parent.attach_child(&self.registry) {
            self.parent = Some((parent, id));
        }
    }

    fn unnest(&mut self) {
        if let Some((parent, id)) = self.parent.take() {
            parent.detach_child(id);
        }
    }
}

impl ViewState<TickerMode> for TickerModeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.renest(ctx);
    }

    /// The ambient registry changed (an ancestor swapped its `Vsync`, or this
    /// subtree moved): move the nesting with it. Without this the registry-tree
    /// edge, fixed at mount, would outlive the widget-tree relationship it
    /// mirrors — ticked, or starved, by the wrong ancestor forever.
    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        self.renest(ctx);
    }

    /// `_updateEffectiveMode` (`ticker_provider.dart:246-252`) — minus the AND,
    /// which the nesting already performs.
    fn did_update_view(&mut self, _old: &TickerMode, new_view: &TickerMode) {
        self.registry.set_muted(!new_view.enabled);
    }

    /// Un-nest: a registry left attached to a live parent would keep ticking a
    /// dead subtree's controllers.
    fn dispose(&mut self) {
        self.unnest();
    }

    /// The subtree sees **this** registry as its ambient `Vsync`, so every
    /// controller a descendant registers lands here and is muted with it.
    ///
    /// **Unless nothing would drive it.** With no ambient `VsyncScope` above,
    /// this registry is nested under nobody and would never be ticked; handing
    /// it down would turn descendants that fall back to their own wall-clock
    /// ticker into *frozen* ones — a widget documented as changing nothing
    /// would silently kill the animations it wraps. So the child passes through
    /// bare and the fallback keeps working.
    fn build(&self, view: &TickerMode, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.parent.is_none() {
            return view.child.clone();
        }
        VsyncScope::new(self.registry.clone(), view.child.clone())
            .into_view()
            .boxed()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use flui_animation::{Animation, AnimationController, Vsync};
    use flui_scheduler::Scheduler;
    use flui_view::ViewExt;
    use parking_lot::Mutex;

    use super::*;
    use crate::SizedBox;
    use crate::test_harness::mount;

    /// Registers `controller` with whatever ambient registry it finds, and
    /// records **whether it found one** — the two facts a `TickerMode` decides.
    /// This is exactly what every animated widget does (`animated_opacity.rs`).
    #[derive(Clone)]
    struct Probe {
        controller: AnimationController,
        found_ambient: Arc<Mutex<Option<bool>>>,
    }

    impl View for Probe {
        fn create_element(&self) -> ElementKind {
            ElementKind::stateful(self)
        }
    }

    impl StatefulView for Probe {
        type State = ProbeState;

        fn create_state(&self) -> Self::State {
            ProbeState {
                controller: self.controller.clone(),
                found_ambient: Arc::clone(&self.found_ambient),
            }
        }
    }

    struct ProbeState {
        controller: AnimationController,
        found_ambient: Arc<Mutex<Option<bool>>>,
    }

    impl std::fmt::Debug for ProbeState {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ProbeState").finish_non_exhaustive()
        }
    }

    impl ViewState<Probe> for ProbeState {
        fn init_state(&mut self, ctx: &dyn BuildContext) {
            let ambient = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());
            *self.found_ambient.lock() = Some(ambient.is_some());
            if let Some(vsync) = ambient {
                let _registration = vsync.register(self.controller.clone());
            }
        }

        fn build(&self, _view: &Probe, _ctx: &dyn BuildContext) -> impl IntoView {
            SizedBox::new(10.0, 10.0)
        }
    }

    fn probe(controller: &AnimationController) -> (Probe, Arc<Mutex<Option<bool>>>) {
        let found = Arc::new(Mutex::new(None));
        (
            Probe {
                controller: controller.clone(),
                found_ambient: Arc::clone(&found),
            },
            found,
        )
    }

    fn controller() -> AnimationController {
        AnimationController::new(Duration::from_secs(1), Arc::new(Scheduler::new()))
    }

    /// A **disabled** `TickerMode` freezes the animations in its subtree and an
    /// **enabled** one lets them run — the whole point of the widget, observed
    /// on a real controller a descendant registered with the ambient registry
    /// (`ticker_provider.dart:397`).
    ///
    /// Red-check (verified): delete the `set_muted(!enabled)` calls — the
    /// disabled subtree animates and the freeze case fails.
    #[test]
    fn a_disabled_ticker_mode_freezes_its_subtree_and_an_enabled_one_does_not() {
        for (enabled, expect_motion) in [(false, false), (true, true)] {
            let root = Vsync::new();
            let animation = controller();
            let (probe, found) = probe(&animation);

            let _harness = mount(VsyncScope::new(
                root.clone(),
                TickerMode::new(probe).enabled(enabled).into_view().boxed(),
            ));
            assert_eq!(
                *found.lock(),
                Some(true),
                "the descendant found the TickerMode's registry"
            );

            let _ = animation.forward();
            root.tick_all(0.0);
            root.tick_all(0.5);

            assert_eq!(
                animation.value() > 0.0,
                expect_motion,
                "TickerMode(enabled = {enabled}) should {} its subtree (value {})",
                if expect_motion { "run" } else { "freeze" },
                animation.value()
            );
            animation.dispose();
        }
    }

    /// **A `TickerMode` with no ambient `VsyncScope` above must not swallow its
    /// subtree's registration.** Its registry would hang under nobody and never
    /// be ticked, so handing it down would turn descendants that fall back to
    /// their own wall-clock ticker into frozen ones — a widget documented as
    /// changing nothing, silently killing the animations it wraps.
    ///
    /// Red-check (verified): make `build` always provide the registry — the
    /// probe reports it found an ambient scope, and its controller is now
    /// registered with a registry nothing drives.
    #[test]
    fn a_ticker_mode_without_an_ambient_scope_leaves_the_subtree_alone() {
        let animation = controller();
        let (probe, found) = probe(&animation);

        let _harness = mount(TickerMode::new(probe).into_view().boxed());

        assert_eq!(
            *found.lock(),
            Some(false),
            "with no driver above, the TickerMode must not hand its subtree an \
             undriven registry — the wall-clock fallback has to stay reachable"
        );
        animation.dispose();
    }

    /// The nesting follows the widget tree: a `TickerMode` under a *disabled*
    /// one is starved even when it is itself enabled (Flutter's
    /// `_updateEffectiveMode` AND, `ticker_provider.dart:246-252`) — observed
    /// end to end through two widget layers, not just on the registries.
    #[test]
    fn a_nested_enabled_ticker_mode_cannot_re_enable_a_disabled_ancestor() {
        let root = Vsync::new();
        let animation = controller();
        let (probe, _found) = probe(&animation);

        let _harness = mount(VsyncScope::new(
            root.clone(),
            TickerMode::new(TickerMode::new(probe).enabled(true))
                .enabled(false)
                .into_view()
                .boxed(),
        ));

        let _ = animation.forward();
        root.tick_all(0.0);
        root.tick_all(0.5);
        assert_eq!(
            animation.value(),
            0.0,
            "a disabled ancestor starves the enabled descendant"
        );
        animation.dispose();
    }
}
