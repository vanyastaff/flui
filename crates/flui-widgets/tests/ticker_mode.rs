//! [`TickerMode`] against a mounted tree: a disabled mode freezes its
//! subtree's animations, an absent ambient driver is not swallowed, and nesting
//! follows the widget tree.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController, Vsync};
use flui_view::ViewExt;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::SizedBox;
use flui_widgets::animated::{TickerMode, VsyncScope};
use parking_lot::Mutex;

use crate::common::harness::mount;

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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
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
    AnimationController::without_ticker(Duration::from_secs(1))
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
