//! Integration tests for [`CupertinoTabScaffold`] — the `_TabSwitchingView`
//! contract: a tab's content is built lazily (only once visited) and its
//! state survives switching away and back (Offstage, not unmount), plus the
//! content-padding contract and the tab bar's own tap wiring.

mod common;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use common::{lay_out, lay_out_animated, tight};
use flui_animation::{Animation, AnimationController, Vsync, VsyncRegistration};
use flui_cupertino::{
    CupertinoTabBar, CupertinoTabBarItem, CupertinoTabController, CupertinoTabScaffold,
};
use flui_scheduler::Scheduler;
use flui_types::Color;
use flui_types::geometry::px;
use flui_view::prelude::*;
use flui_widgets::prelude::EdgeInsets;
use flui_widgets::{Icon, IconData, MediaQuery, MediaQueryData, SizedBox, VsyncScope};

/// Per-pump virtual-time step for the `TickerMode` animation test — half the
/// probe `AnimationController`'s own 1s duration, matching
/// `flui-widgets/tests/visibility.rs`'s identical animation-probe pattern.
const FRAME: Duration = Duration::from_millis(300);

fn two_tab_bar() -> CupertinoTabBar {
    CupertinoTabBar::new(vec![
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home"),
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A2))).label("Settings"),
    ])
}

/// A leaf whose `create_state` is counted — proves whether a tab's content
/// element survived a visibility toggle (Offstage) versus being torn down
/// and rebuilt from scratch.
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
/// `_TabSwitchingViewState.shouldBuildTab` (`tab_scaffold.dart`, oracle tag
/// `3.44.0`). Tab 1 must never be built while tab 0 stays active.
///
/// Red-check: drop the `should_build[index]` gate in `tab_scaffold.rs`'s
/// `build` (call `tab_builder` unconditionally for every index) — this
/// test's "tab 1 never called before it's visited" assertion fails on the
/// very first mount.
///
/// Note: once a tab *has* been visited, the oracle calls `tabBuilder` again
/// on every subsequent rebuild for it too (`shouldBuildTab[index] ?
/// widget.tabBuilder(...) : ...` re-evaluates every build) — Element
/// diffing, not a build-count cache, is what keeps its *state* stable (see
/// `an_inactive_tabs_state_survives_switching_away_and_back` below). This
/// test only proves the "not yet visited" half of the contract.
#[test]
fn a_tab_is_not_built_until_it_becomes_active() {
    let called_indices: Rc<Cell<Vec<usize>>> = Rc::new(Cell::new(Vec::new()));
    let called_for_closure = Rc::clone(&called_indices);

    let controller = CupertinoTabController::new(0);
    let scaffold =
        CupertinoTabScaffold::new(two_tab_bar(), controller.clone(), move |_ctx, index| {
            let mut called = called_for_closure.take();
            called.push(index);
            called_for_closure.set(called);
            SizedBox::new(10.0, 10.0).into_view().boxed()
        });

    let mut laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), scaffold),
        tight(400.0, 800.0),
    );
    laid.tick();

    assert!(
        called_indices.take().iter().all(|&i| i == 0),
        "tab 1 must never be called while tab 0 stays active"
    );

    controller.set_index(1);
    laid.tick();

    assert!(
        called_indices.take().contains(&1),
        "tab 1 must be called once it becomes active"
    );
}

/// An inactive tab's own element state survives a switch away and back —
/// `Offstage`, not unmount. Flutter parity: the same
/// `_TabSwitchingViewState` contract above, proven this time via a
/// `StatefulView`'s `create_state` count rather than a builder-call count.
///
/// Red-check: key each tab's `Offstage` subtree by `(index, current_index)`
/// instead of `index` alone (forcing a fresh element on every switch) — this
/// test's `created.get() == 1` assertion fails (would read `2`).
#[test]
fn an_inactive_tabs_state_survives_switching_away_and_back() {
    let created = Rc::new(Cell::new(0_u32));
    let created_for_closure = Rc::clone(&created);

    let controller = CupertinoTabController::new(0);
    let scaffold =
        CupertinoTabScaffold::new(two_tab_bar(), controller.clone(), move |_ctx, index| {
            if index == 0 {
                Probe(Rc::clone(&created_for_closure)).into_view().boxed()
            } else {
                SizedBox::new(10.0, 10.0).into_view().boxed()
            }
        });

    let mut laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), scaffold),
        tight(400.0, 800.0),
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
        "switching away to tab 1 and back to tab 0 must not recreate tab 0's state"
    );
}

/// Registers `controller` against the ambient `VsyncScope` in `init_state`
/// and unregisters it in `dispose` — mirrors
/// `flui-widgets/tests/visibility.rs`'s identical `AnimationProbe` fixture,
/// used there to prove `Visibility`'s own `TickerMode` wiring mutes a hidden
/// subtree's animation.
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

/// An inactive tab's animation genuinely stops advancing — Flutter's own
/// doc comment on `_TabSwitchingView`: "Off stage tabs' animations are
/// stopped" (`tab_scaffold.dart`, oracle tag `3.44.0`). Proven by registering
/// a real `AnimationController` against the ambient `VsyncScope` from inside
/// tab 0's content, then switching to tab 1 and confirming the controller's
/// value freezes.
///
/// Red-check: drop the `TickerMode::new(content).enabled(active)` wrap in
/// `tab_scaffold.rs`'s `build` (go back to `Offstage` alone) — this test's
/// final `assert_eq!` fails, since the controller keeps advancing while tab 0
/// is merely offstage, not ticker-muted.
#[test]
fn an_inactive_tabs_animation_is_muted_by_ticker_mode() {
    let vsync = Vsync::new();
    let controller = AnimationController::new(Duration::from_secs(1), Arc::new(Scheduler::new()));
    let tab_controller = CupertinoTabController::new(0);

    let probe_controller = controller.clone();
    let scaffold =
        CupertinoTabScaffold::new(two_tab_bar(), tab_controller.clone(), move |_ctx, index| {
            if index == 0 {
                AnimationProbe {
                    controller: probe_controller.clone(),
                }
                .into_view()
                .boxed()
            } else {
                SizedBox::new(10.0, 10.0).into_view().boxed()
            }
        });
    let root = VsyncScope::new(
        vsync.clone(),
        MediaQuery::new(MediaQueryData::default(), scaffold),
    );
    let mut laid = lay_out_animated(root, tight(400.0, 800.0), vsync);

    controller.forward().expect("animation should start");
    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert!(
        controller.value() > 0.0,
        "tab 0 is active on mount; its registered animation must advance"
    );

    tab_controller.set_index(1);
    laid.tick();
    let value_when_switched_away = controller.value();

    laid.pump_for(FRAME);
    laid.pump_for(FRAME);
    assert_eq!(
        controller.value(),
        value_when_switched_away,
        "an inactive tab's animation must not advance once TickerMode disables it"
    );
    controller.dispose();
}

/// With an **opaque** tab bar background, content is pushed up by exactly
/// the tab bar's `preferred_size().height` plus `MediaQuery.padding.bottom`
/// — `tab_scaffold.dart`'s `bottomPadding`, the `tabBar.opaque(context)`
/// branch (oracle tag `3.44.0`).
///
/// Red-check: hardcode the `opaque(ctx)` branch to always take the "opaque"
/// arm in `tab_scaffold.rs` — this test still passes (nothing distinguishes
/// it from the always-opaque bug the previous version of this code had),
/// but [`translucent_tab_bar_does_not_pad_content_and_hints_via_media_query`]
/// below then fails, since it specifically requires the *translucent* arm.
#[test]
fn opaque_tab_bar_pads_content_above_it_plus_the_bottom_inset() {
    let media = MediaQueryData {
        padding: EdgeInsets::new(px(0.0), px(0.0), px(20.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let controller = CupertinoTabController::new(0);
    let opaque_bar = two_tab_bar().background_color(Color::rgba(0, 0, 0, 255));
    let scaffold = CupertinoTabScaffold::new(opaque_bar, controller, |_ctx, _index| {
        SizedBox::new(60.0, 30.0).into_view().boxed()
    });

    let laid = lay_out(MediaQuery::new(media, scaffold), tight(400.0, 800.0));

    // The tab bar's own build wraps its row (and each item) in further
    // `Padding`s — the content's own is the only one carrying this exact
    // bottom value.
    let matching_padding = laid
        .find_all_by_render_type("RenderPadding")
        .into_iter()
        .filter_map(|id| laid.render_property(id, "padding"))
        .find(|padding| padding.contains("bottom: 70px"));
    assert!(
        matching_padding.is_some(),
        "some Padding must carry the content's 50.0 tab bar height + 20.0 bottom inset = 70.0"
    );
}

/// With the **default, translucent** tab bar background (the oracle's own
/// default `barBackgroundColor` is `0xF0`-alpha, not fully opaque), content
/// is **not** shifted — it may draw behind the bar — and the obstruction is
/// hinted via the republished `MediaQuery.padding.bottom` instead
/// (`tab_scaffold.dart`'s `else` branch, oracle tag `3.44.0`).
///
/// Red-check: flip the `if view.tab_bar.opaque(ctx)` condition in
/// `tab_scaffold.rs` — this test's "no Padding carries 70px" assertion fails.
#[test]
fn translucent_tab_bar_does_not_pad_content_and_hints_via_media_query() {
    let controller = CupertinoTabController::new(0);
    // `two_tab_bar()`'s default background resolves to the theme's
    // `bar_background_color`, whose default alpha is `0xF0` — translucent.
    // `MediaQueryData::default()` has `padding.bottom == 0`, so the wrong
    // (mutated) behavior would pad content by exactly `tab_bar_height` —
    // 50px, not 0 — which is what the assertion below pins.
    let scaffold = CupertinoTabScaffold::new(two_tab_bar(), controller, |_ctx, _index| {
        SizedBox::new(60.0, 30.0).into_view().boxed()
    });

    let laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), scaffold),
        tight(400.0, 800.0),
    );

    let has_50px_padding = laid
        .find_all_by_render_type("RenderPadding")
        .into_iter()
        .filter_map(|id| laid.render_property(id, "padding"))
        .any(|padding| padding.contains("bottom: 50px"));
    assert!(
        !has_50px_padding,
        "a translucent tab bar must not shift content by its own height"
    );
}

/// When the on-screen keyboard (`view_insets.bottom`) is already taller than
/// the tab bar itself, content is padded by the **keyboard inset alone** —
/// the tab-bar-height contribution is skipped entirely, not added on top
/// (`tab_scaffold.dart`'s `tabBar.preferredSize.height >
/// existingMediaQuery.viewInsets.bottom` guard, oracle tag `3.44.0`; "don't
/// double pad" is a real edge case here, not a simplification).
///
/// Red-check: flip `tab_bar_height > media.view_insets.bottom` to `<` in
/// `tab_scaffold.rs` — the "no 50px contribution" assertion below fails
/// (the tab-bar-height branch would wrongly trigger despite the keyboard
/// already being taller).
#[test]
fn keyboard_taller_than_the_tab_bar_pads_content_by_the_keyboard_inset_alone() {
    let media = MediaQueryData {
        view_insets: EdgeInsets::new(px(0.0), px(0.0), px(300.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let controller = CupertinoTabController::new(0);
    // Opaque, matching `opaque_tab_bar_pads_content_above_it_plus_the_bottom_inset`:
    // this test exercises the tab-bar-height-vs-keyboard-inset guard itself,
    // not the opaque/translucent branch `opaque(ctx)` already covers above —
    // a translucent bar would route this same 300px value through
    // `reduced.padding.bottom` instead of `content_padding_bottom`, which
    // would defeat this test's `RenderPadding` probe regardless of the guard.
    let opaque_bar = two_tab_bar().background_color(Color::rgba(0, 0, 0, 255));
    let scaffold = CupertinoTabScaffold::new(opaque_bar, controller, |_ctx, _index| {
        SizedBox::new(60.0, 30.0).into_view().boxed()
    });

    let laid = lay_out(MediaQuery::new(media, scaffold), tight(400.0, 800.0));
    let paddings: Vec<String> = laid
        .find_all_by_render_type("RenderPadding")
        .into_iter()
        .filter_map(|id| laid.render_property(id, "padding"))
        .collect();

    assert!(
        paddings
            .iter()
            .any(|padding| padding.contains("bottom: 300px")),
        "the 300px keyboard inset (taller than the 50px tab bar) must become content padding \
         directly: {paddings:?}"
    );
    assert!(
        !paddings
            .iter()
            .any(|padding| padding.contains("bottom: 50px")),
        "the tab-bar-height contribution must be skipped entirely when the keyboard is \
         already taller than it: {paddings:?}"
    );
}

/// Tapping a tab bar item advances the shared controller's index, which
/// rebuilds the scaffold's active tab — an end-to-end proof that
/// `CupertinoTabScaffold` actually wires the bar's `on_tap`, not just that
/// `CupertinoTabController::set_index` compiles.
#[test]
fn tapping_a_tab_item_switches_the_active_tab() {
    let controller = CupertinoTabController::new(0);
    let scaffold = CupertinoTabScaffold::new(two_tab_bar(), controller.clone(), |_ctx, index| {
        if index == 0 {
            SizedBox::new(11.0, 11.0).into_view().boxed()
        } else {
            SizedBox::new(22.0, 22.0).into_view().boxed()
        }
    });

    let mut laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), scaffold),
        tight(400.0, 800.0),
    );
    laid.tick();
    assert_eq!(controller.index(), 0);

    // Tab bar sits at the bottom, split into two equal-width items; tap
    // squarely inside the second (Settings) item.
    laid.dispatch_pointer_down(300.0, 790.0);
    laid.dispatch_pointer_up(300.0, 790.0);
    laid.tick();

    assert_eq!(
        controller.index(),
        1,
        "tapping the second tab item must advance the controller to index 1"
    );
}

/// An out-of-range controller index must fail loudly, not silently render
/// every tab `Offstage` with `tab_builder` never invoked. Flutter parity:
/// `_onCurrentIndexChange`'s `assert(_controller.index >= 0 &&
/// _controller.index < widget.tabBar.items.length, ...)` (`tab_scaffold.dart`,
/// oracle tag `3.44.0`) — a debug-only `assert`, matched here by the
/// `debug_assert!` in `tab_scaffold.rs`'s `build` (test binaries build in
/// debug profile, so it is live).
///
/// This test asserts the panic message from `lay_out`'s own "the mounted
/// subtree should have a render root" check, **not** the `debug_assert!`'s
/// own message: this crate's build-error boundary
/// (`flui_view::element::behavior_commons::build_or_recover`) catches a
/// panicking `build()` and substitutes a render-less `ErrorView` for the
/// whole subtree — the same recovery `flui-material/tests/theme.rs` and
/// `flui-widgets/tests/visibility.rs` document for exactly this reason
/// ("a panic inside build() is caught by the framework's build-error
/// boundary... so #[should_panic] around the harness would not observe
/// it"). With `CupertinoTabScaffold` mounted as the sole root here, that
/// substitution leaves nothing at all to render, so `lay_out` itself panics
/// — a loud, unmistakable mount failure, not the old silent
/// every-tab-hidden mis-render.
///
/// Red-check: delete the `debug_assert!` in `CupertinoTabScaffoldState::build`
/// — this test stops panicking entirely (the scaffold instead mounts
/// successfully with every tab hidden and `tab_builder` uncalled for index 5).
#[test]
#[should_panic(expected = "render root")]
fn out_of_range_controller_index_panics_instead_of_silently_hiding_every_tab() {
    let controller = CupertinoTabController::new(5);
    let scaffold = CupertinoTabScaffold::new(two_tab_bar(), controller, |_ctx, _index| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    });

    let _ = lay_out(
        MediaQuery::new(MediaQueryData::default(), scaffold),
        tight(400.0, 800.0),
    );
}
