//! [`TabBarView`] widget-level mount/interaction coverage — complements
//! `tab_bar_view.rs`'s own unit tests (the pure builder-field probes) with
//! end-to-end mount proof of the lazy-keep-alive switcher contract this
//! module docs cite `CupertinoTabScaffold`'s `_TabSwitchingView` mechanic
//! for: a not-yet-visited child is never built, a visited-then-hidden
//! child's own state survives (`Offstage`, not unmount), an inactive
//! child's animation is muted (`TickerMode`), a controller swap/unmount
//! removes the old listener, and an out-of-range controller index falls
//! through without panicking in release (proven here via the `debug_assert!`
//! itself, live in this crate's own debug test profile).

mod common;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use common::{lay_out, lay_out_animated, tight};
use flui_animation::{Animation, AnimationController, Vsync, VsyncRegistration};
use flui_material::{
    DefaultTabController, Tab, TabBar, TabBarView, TabController, Theme, ThemeData,
};
use flui_scheduler::Scheduler;
use flui_view::prelude::*;
use flui_widgets::{
    Column, CrossAxisAlignment, Expanded, MediaQuery, MediaQueryData, SizedBox, VsyncScope,
};

/// Per-pump virtual-time step for the `TickerMode` animation test — half the
/// probe `AnimationController`'s own 1s duration, matching
/// `flui-cupertino/tests/tab_scaffold.rs`'s identical animation-probe
/// pattern (`FRAME`).
const FRAME: Duration = Duration::from_millis(300);

/// A leaf whose `create_state` is counted — proves whether a tab's content
/// element survived a visibility toggle (`Offstage`) versus being torn down
/// and rebuilt from scratch. Same fixture shape as
/// `flui-cupertino/tests/tab_scaffold.rs`'s own `Probe` (this crate cannot
/// see that one — it's private to that test binary).
#[derive(Clone)]
struct Probe(Rc<Cell<u32>>);

impl View for Probe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for Probe {
    type State = ProbeState;

    fn create_state(&self) -> Self::State {
        self.0.set(self.0.get() + 1);
        ProbeState
    }
}

struct ProbeState;

impl ViewState<Probe> for ProbeState {
    fn build(&self, _view: &Probe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// A tab's content is only ever built the first time it becomes active —
/// Flutter parity (mechanism-adapted, see `tab_bar_view.rs`'s module docs):
/// `_TabSwitchingViewState.shouldBuildTab`. Tab 1's `Probe` must never be
/// constructed while tab 0 stays active.
///
/// Red-check: drop the `should_build[index]` gate in `tab_bar_view.rs`'s
/// `build` (clone every child unconditionally instead of substituting
/// `SizedBox::shrink()` for a not-yet-visited one) — tab 1's `Probe` would
/// then mount (and its `create_state` fire) on the very first build, and
/// this test's "must never be created while tab 0 is active" assertion
/// fails.
#[test]
fn a_tab_is_not_built_until_it_becomes_active() {
    let created = Rc::new(Cell::new(0_u32));
    let created_for_tab_1 = Rc::clone(&created);

    let controller = TabController::new(2, 0);
    let view = TabBarView::new(vec![
        SizedBox::new(10.0, 10.0).into_view().boxed(),
        Probe(created_for_tab_1).into_view().boxed(),
    ])
    .controller(controller.clone());

    let mut laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), view),
        tight(400.0, 400.0),
    );
    laid.tick();

    assert_eq!(
        created.get(),
        0,
        "tab 1's Probe must never be created while tab 0 stays active"
    );

    controller.set_index(1);
    laid.tick();

    assert_eq!(
        created.get(),
        1,
        "tab 1's Probe must be created once it becomes active"
    );
}

/// An inactive tab's own element state survives a switch away and back —
/// `Offstage`, not unmount. Same `_TabSwitchingViewState` contract as above,
/// proven this time via a `create_state` COUNT staying at `1` across a
/// round trip rather than merely becoming nonzero.
///
/// Red-check: key each tab's `Offstage` layer by `(index, current_index)`
/// instead of `index` alone (forcing a fresh element identity on every
/// switch) — `created.get()` would read `2` instead of `1` after the round
/// trip below.
#[test]
fn an_inactive_tabs_state_survives_switching_away_and_back() {
    let created = Rc::new(Cell::new(0_u32));
    let created_for_tab_0 = Rc::clone(&created);

    let controller = TabController::new(2, 0);
    let view = TabBarView::new(vec![
        Probe(created_for_tab_0).into_view().boxed(),
        SizedBox::new(10.0, 10.0).into_view().boxed(),
    ])
    .controller(controller.clone());

    let mut laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), view),
        tight(400.0, 400.0),
    );
    laid.tick();
    assert_eq!(
        created.get(),
        1,
        "tab 0's Probe must have been created once"
    );

    controller.set_index(1);
    laid.tick();
    controller.set_index(0);
    laid.tick();

    assert_eq!(
        created.get(),
        1,
        "switching away to tab 1 and back to tab 0 must not recreate tab 0's Probe state"
    );
}

/// Registers `controller` against the ambient `VsyncScope` in `init_state`
/// and unregisters it in `dispose` — the same `AnimationProbe` fixture
/// `flui-cupertino/tests/tab_scaffold.rs` uses to prove its own
/// `TickerMode` wiring.
#[derive(Clone, StatefulView)]
struct AnimationProbe {
    controller: AnimationController,
}

struct AnimationProbeState {
    controller: AnimationController,
    registration: Option<(Vsync, VsyncRegistration)>,
}

impl StatefulView for AnimationProbe {
    type State = AnimationProbeState;

    fn create_state(&self) -> Self::State {
        AnimationProbeState {
            controller: self.controller.clone(),
            registration: None,
        }
    }
}

impl ViewState<AnimationProbe> for AnimationProbeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            let registration = vsync.register(self.controller.clone());
            self.registration = Some((vsync, registration));
        }
    }

    fn dispose(&mut self) {
        if let Some((vsync, registration)) = self.registration.take() {
            vsync.unregister(registration);
        }
    }

    fn build(&self, _view: &AnimationProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// An inactive tab's animation genuinely stops advancing — the same "Off
/// stage tabs' animations are stopped" contract
/// `flui-cupertino/tests/tab_scaffold.rs`'s
/// `an_inactive_tabs_animation_is_muted_by_ticker_mode` proves for
/// `CupertinoTabScaffold`. Proven by registering a real `AnimationController`
/// against the ambient `VsyncScope` from inside tab 0's content, then
/// switching to tab 1 and confirming the controller's value freezes.
///
/// Red-check: drop the `TickerMode::new(content).enabled(active)` wrap in
/// `tab_bar_view.rs`'s `build` (go back to bare `Offstage`) — this test's
/// final `assert_eq!` fails, since the controller keeps advancing while tab
/// 0 is merely offstage, not ticker-muted.
#[test]
fn an_inactive_tabs_animation_is_muted_by_ticker_mode() {
    let vsync = Vsync::new();
    let animation = AnimationController::new(Duration::from_secs(1), Arc::new(Scheduler::new()));
    let tab_controller = TabController::new(2, 0);

    let probe_controller = animation.clone();
    let view = TabBarView::new(vec![
        AnimationProbe {
            controller: probe_controller,
        }
        .into_view()
        .boxed(),
        SizedBox::new(10.0, 10.0).into_view().boxed(),
    ])
    .controller(tab_controller.clone());
    let root = VsyncScope::new(
        vsync.clone(),
        MediaQuery::new(MediaQueryData::default(), view),
    );
    let mut laid = lay_out_animated(root, tight(400.0, 400.0), vsync);

    animation.forward().expect("animation should start");
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert!(
        animation.value() > 0.0,
        "tab 0 is active on mount; its registered animation must advance"
    );

    tab_controller.set_index(1);
    laid.tick();
    let value_when_switched_away = animation.value();

    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert_eq!(
        animation.value(),
        value_when_switched_away,
        "an inactive tab's animation must not advance once TickerMode disables it"
    );
    animation.dispose();
}

/// Unmounting a `TabBarView` removes its listener from the (outliving)
/// `TabController` it was subscribed to — a controller that outlives the
/// view must not keep firing a dead `Rc` closure against an unmounted
/// element's `RebuildHandle`. Same "count seam" pattern as
/// `crates/flui-material/tests/tabs.rs`'s
/// `unmounting_a_tab_bar_removes_its_listener_from_the_controller`, now
/// proven for `TabBarView`'s own `dispose`.
#[test]
fn unmounting_a_tab_bar_view_removes_its_listener_from_the_controller() {
    let controller = TabController::new(2, 0);

    let before_mount = controller.listener_count();
    let mut laid = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            TabBarView::new(vec![
                SizedBox::new(10.0, 10.0).into_view().boxed(),
                SizedBox::new(20.0, 20.0).into_view().boxed(),
            ])
            .controller(controller.clone()),
        ),
        tight(400.0, 400.0),
    );
    let while_mounted = controller.listener_count();
    assert!(
        while_mounted > before_mount,
        "mounting a TabBarView must register its own listener on the controller"
    );

    // Root-swap to an unrelated tree — `TabBarViewState::dispose` must fire
    // for the removed `TabBarView`.
    laid.pump_widget(MediaQuery::new(
        MediaQueryData::default(),
        SizedBox::shrink(),
    ));

    let after_removal = controller.listener_count();
    assert_eq!(
        after_removal, before_mount,
        "removing a TabBarView from the tree must remove its listener from the controller, \
         not leak it"
    );
}

/// A `TabBarView` with neither an explicit `controller` nor a
/// `DefaultTabController` ancestor panics loudly (Flutter parity:
/// `_updateTabController`'s `FlutterError`) instead of silently rendering
/// with no active tab — same documented panic-boundary mechanism as
/// `crates/flui-material/tests/tabs.rs`'s
/// `a_tab_bar_with_no_controller_and_no_default_tab_controller_ancestor_panics`
/// and `flui-cupertino/tests/tab_scaffold.rs`'s
/// `out_of_range_controller_index_panics_instead_of_silently_hiding_every_tab`.
#[test]
#[should_panic(expected = "render root")]
fn a_tab_bar_view_with_no_controller_and_no_default_tab_controller_ancestor_panics() {
    let _ = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            TabBarView::new(vec![SizedBox::new(10.0, 10.0).into_view().boxed()]),
        ),
        tight(400.0, 400.0),
    );
}

/// The `children.len() != controller.length()` mismatch `debug_assert!` is
/// live in this crate's own (debug-profile) test build — a length mismatch
/// panics inside `build`, caught by the framework's build-error boundary,
/// which leaves nothing to render (same mechanism the previous test's doc
/// comment cites).
///
/// Red-check: delete the `debug_assert!` in `tab_bar_view.rs`'s `build` —
/// this test stops panicking; the view instead mounts with tab index 2
/// (out of range for the 2-child `Vec`) simply never matching any child, so
/// every child renders `Offstage`-hidden — the documented release
/// fall-through this test's sibling assertion (module docs) describes,
/// silently reached in a build where the assert should have fired instead.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "render root")]
fn a_children_count_mismatched_with_the_controllers_length_panics() {
    let controller = TabController::new(3, 2);
    let _ = lay_out(
        MediaQuery::new(
            MediaQueryData::default(),
            TabBarView::new(vec![
                SizedBox::new(10.0, 10.0).into_view().boxed(),
                SizedBox::new(20.0, 20.0).into_view().boxed(),
            ])
            .controller(controller),
        ),
        tight(400.0, 400.0),
    );
}

/// `RenderDiagnostics::add_flag` (`flui-foundation`'s `debug_fill_properties`
/// helper) omits a flag property entirely when it's `false` — so
/// `render_property(id, "offstage")` is `Some(_)` exactly when that
/// `Offstage` layer IS offstage, `None` when it's on-stage. `TabBarView`
/// mounts each tab's `Offstage` layer in `view.children` order (index 0
/// first), so `laid.children(stack)[index]` is that tab's own layer.
fn is_offstage(laid: &common::LaidOut, id: flui_foundation::RenderId) -> bool {
    laid.render_property(id, "offstage").is_some()
}

/// A `DefaultTabController` ancestor is reachable by (and sufficient for) a
/// descendant `TabBarView` with NO explicit controller — the fallback half
/// of the "explicit, else `DefaultTabController`" contract
/// `a_tab_bar_view_with_no_controller_and_no_default_tab_controller_ancestor_panics`
/// proves the negative of.
///
/// Neither the co-mounted `TabBar` nor the `TabBarView` below is ever given
/// an explicit `.controller(...)` — an earlier version of this test passed
/// one to `TabBarView` directly, which (per `resolve_controller`'s "explicit
/// wins" precedence) made the `DefaultTabController` ancestor inert and
/// proved nothing about the fallback path. This version drives the switch
/// the only way that actually exercises `DefaultTabController::maybe_of`:
/// a real pointer tap on the co-mounted `TabBar`, mirroring
/// `crates/flui-material/tests/tabs.rs`'s
/// `default_tab_controller_is_reachable_by_a_descendant_tab_bar_and_drives_its_indicator`.
/// Both widgets resolving to the SAME ancestor-owned `TabController` is
/// exactly what makes the tap on one observable through the other's
/// `Offstage` layers below.
///
/// Sizes are NOT the probe here — `Stack::fit(StackFit::Expand)` (matching
/// `CupertinoTabScaffold`'s own choice) forces every layer, on- or
/// off-stage, to the `Stack`'s own tight size, so a size comparison cannot
/// tell them apart; the `offstage` diagnostics flag can.
///
/// Mutation red-check (run and reverted, see the review evidence): remove
/// the `.or_else(|| DefaultTabController::maybe_of(ctx))` arm from
/// `tab_bar_view.rs`'s `resolve_controller` — `build` then panics on mount
/// (no explicit controller, no fallback), and this test fails loudly
/// instead of silently passing.
#[test]
fn default_tab_controller_ancestor_drives_the_active_child_through_offstage() {
    let tabs = vec![Tab::new().text("One"), Tab::new().text("Two")];
    let mut laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                DefaultTabController::new(
                    2,
                    Column::new(vec![
                        TabBar::secondary(tabs).boxed(),
                        Expanded::new(TabBarView::new(vec![
                            SizedBox::new(30.0, 30.0).into_view().boxed(),
                            SizedBox::new(40.0, 40.0).into_view().boxed(),
                        ]))
                        .boxed(),
                    ])
                    .cross_axis_alignment(CrossAxisAlignment::Stretch),
                ),
            ),
        ),
        tight(400.0, 400.0),
    );
    laid.tick();

    // The co-mounted `TabBar` mounts its OWN `RenderStack` internally (the
    // divider layer + tab row, see `tabs.rs`'s module docs) — disambiguate
    // `TabBarView`'s own `Stack` structurally, as the one whose direct
    // children are ALL `RenderOffstage` layers (the `TabBar`'s stack's
    // children are not).
    let offstage_ids: std::collections::HashSet<_> = laid
        .find_all_by_render_type("RenderOffstage")
        .into_iter()
        .collect();
    let stack = laid
        .find_all_by_render_type("RenderStack")
        .into_iter()
        .find(|&id| {
            let kids = laid.children(id);
            !kids.is_empty() && kids.iter().all(|kid| offstage_ids.contains(kid))
        })
        .expect("TabBarView must mount a Stack of per-tab Offstage layers");
    let layers = laid.children(stack);
    assert_eq!(layers.len(), 2, "one Offstage layer per tab");

    assert!(
        !is_offstage(&laid, layers[0]),
        "tab 0 is the initial active index — its Offstage layer must be on-stage"
    );
    assert!(
        is_offstage(&laid, layers[1]),
        "tab 1 is inactive on mount — its Offstage layer must be offstage"
    );

    // Tap the second (of two, equal-width) tabs in the co-mounted TabBar —
    // matching `tests/tabs.rs`'s `tap_sets_the_controller_index_through_real_pointer_dispatch`
    // geometry, scaled to this test's 400px-wide bar (two 200px cells; the
    // second spans x in [200, 400), 48px tall).
    laid.dispatch_pointer_down(300.0, 24.0);
    laid.dispatch_pointer_up(300.0, 24.0);
    laid.tick();

    let layers_after = laid.children(stack);
    assert!(
        is_offstage(&laid, layers_after[0]),
        "after tapping the TabBar's second tab, TabBarView's tab 0 layer must become offstage"
    );
    assert!(
        !is_offstage(&laid, layers_after[1]),
        "after tapping the TabBar's second tab, TabBarView's tab 1 layer must become on-stage"
    );
}
