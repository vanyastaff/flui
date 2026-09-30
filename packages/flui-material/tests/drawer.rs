//! `Drawer`/`DrawerController`/`Scaffold` drawer-slot end-to-end coverage —
//! a real [`Vsync`] clock drives the settle animation, matching
//! `tests/show_dialog.rs`'s harness. Sections, in file order: (1) closed-state
//! edge-strip translucent hit-testing, (2) mid-drag panel geometry, (3) the
//! scrim's mount, tap-to-close, and settle-to-unmount, (4) the
//! [`ScaffoldScope`] handle's data surface (`has_drawer`/`is_drawer_open`)
//! and the no-flash mount timing, (5) `on_drawer_changed`'s forward to the
//! app author, (6) the dynamic child order when both drawers are configured.
//!
//! Pure value/status math (fling threshold, direction factor, the three
//! `on_drawer_changed` firing paths) is ALSO covered at the
//! `DrawerControllerCore` unit level in
//! `packages/flui-material/src/drawer.rs`'s own test module — deterministic
//! there (no real-clock-dependent velocity simulation needed); this file
//! additionally covers what only a real mounted tree can prove: geometry,
//! hit-testing, the `GlobalKey` bridge, and `Scaffold`'s own relay of the
//! per-drawer callback.
//!
//! # `themed` vs `themed_animated` — the `VsyncScope` requirement
//!
//! [`DrawerControllerState::init_state`] resolves its `Vsync` via
//! `ctx.get::<VsyncScope, _>` — an ordinary ANCESTOR-WIDGET lookup, entirely
//! separate from [`common::lay_out_animated`]'s `vsync` parameter (which
//! only adopts a `Vsync` onto the *binding*, i.e. what `pump_for`/`tick_all`
//! iterate). A tree with no `VsyncScope` ancestor leaves
//! `DrawerControllerCore::vsync` `None`, so nothing ever registers with the
//! adopted `Vsync`, and `pump_for` ticks precisely zero controllers no
//! matter how large the budget — a `close()`/`open()` fling then never
//! progresses past its very first simulated value, and a mount/unmount that
//! depends on the fling actually *settling* (not just starting) never
//! happens. [`themed_animated`] wraps `vsync` in [`flui_sdk::widgets::VsyncScope`]
//! so the tree-side registration and the binding-side pump are the SAME
//! clock; plain [`themed`] (no `VsyncScope`) is for tests that only need
//! synchronous effects (a bare `set_value`, or a same-tick status flip) and
//! never call `pump_for`/`tick_all` expecting real animation progress.
//!
//! # Harness limitation: no pointer capture
//!
//! `LaidOut::dispatch_pointer_move` re-hit-tests at the NEW position on every
//! call (`packages/flui-material/tests/common/mod.rs`'s own doc: "no pointer
//! capture") — a real windowing backend instead keeps routing every
//! subsequent move/up to whoever captured the down, regardless of where the
//! pointer physically is now. The default closed-state edge strip is only
//! 20px wide (`_kEdgeDragWidth`) — a REALISTIC open-drag immediately carries
//! the pointer outside those 20px, so a headless re-hit-test at the new
//! position finds nothing there and the recognizer silently stops receiving
//! events. This is a genuine, structural harness gap (shared by both
//! `flui-widgets` and `flui-material`'s copies of the harness), not a
//! `Drawer`/`Scaffold` bug — production dispatch has real pointer capture.
//! Tests below that need a drag to travel more than ~18px (the default pan
//! slop) past its start use `Scaffold::drawer_edge_drag_width` to widen the
//! strip to cover the whole drag path, working around the harness gap
//! without touching production defaults.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::{lay_out_animated, tight};
use flui_material::{Drawer, DrawerHandle, Scaffold, ScaffoldScope, Theme, ThemeData};
use flui_sdk::animation::Vsync;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{GestureDetector, MediaQuery, MediaQueryData, SizedBox, VsyncScope};

/// Wraps `scaffold` in the `Theme`/`MediaQuery` ancestors `Scaffold`/
/// `Drawer`/`Material` all require (`Theme::of`/`MediaQuery::of` panic
/// without one). No `VsyncScope` — for tests driven by [`lay_out`] alone,
/// where nothing needs to tick a real animation forward (a bare `set_value`
/// drag or a same-tick status flip is synchronous). Tests that pump virtual
/// frames to settle a fling use [`themed_animated`] instead.
fn themed(scaffold: Scaffold) -> impl View {
    MediaQuery::new(
        MediaQueryData::default(),
        Theme::new(ThemeData::light(), scaffold),
    )
}

/// [`themed`], plus a `VsyncScope` wrapping `vsync` — required for
/// `DrawerControllerState::init_state`'s `ctx.get::<VsyncScope, _>` lookup
/// to find an ancestor and register the controller at all. Without this,
/// `LaidOut::pump_for`/`tick` advance virtual time but tick *nothing*: the
/// controller's fling/spring simulation never progresses, so a `close()`/
/// `open()` that must actually *settle* (not just flip status) never will,
/// no matter how large the pump budget — this is exactly the gap
/// `lay_out_animated` (which only adopts `vsync` onto the *binding*, driving
/// whatever the tree registers with it) exists to close, and every test
/// below that calls `lay_out_animated` needs the tree-side registration
/// `VsyncScope` provides too, not just the binding-side adoption.
fn themed_animated(scaffold: Scaffold, vsync: &Vsync) -> impl View {
    VsyncScope::new(vsync.clone(), themed(scaffold))
}

/// `_kBaseSettleDuration` (`drawer.dart`, oracle tag `3.44.0`) — the
/// duration a plain `forward()`/`reverse()` run takes. A `fling()` (what
/// `open()`/`close()` actually call) drives a spring simulation instead,
/// whose own settling time is independent of this constant and, for a
/// full-distance run, measurably longer — see [`FLING_SETTLE_PUMPS`].
const SETTLE: Duration = Duration::from_millis(246);
/// The per-pump virtual-time step.
const FRAME: Duration = Duration::from_millis(16);
/// Enough pumps to carry `SETTLE` past its end — matching
/// `flui-material/tests/show_dialog.rs`'s identical `+ 2` budget. Sufficient
/// for a fling from a drag-shortened distance (most of this file's tests),
/// but NOT for a full 1.0 -> 0.0 (or 0.0 -> 1.0) fling — see
/// [`FLING_SETTLE_PUMPS`].
const PUMPS: usize = (SETTLE.as_millis() / FRAME.as_millis()) as usize + 2;

/// A fling's settling time depends on the spring simulation, not `SETTLE` —
/// a full-distance fling (e.g. `close()` from a fully open, fully rested
/// drawer) measured empirically at 36 `FRAME`-sized ticks (~576ms) to reach
/// `AnimationStatus::Dismissed`. This budget carries generous margin over
/// that measurement. Tests that fling across the drawer's *entire* range
/// (not a drag-shortened one) and need to observe the *settled* result
/// (mount/unmount, not just the synchronous `on_open_changed` report) use
/// this instead of [`PUMPS`].
const FLING_SETTLE_PUMPS: usize = 60;

const _: () = assert!(
    (PUMPS as u128) * FRAME.as_millis() > SETTLE.as_millis(),
    "PUMPS * FRAME must carry the settle animation past its end"
);

/// Captures the ambient [`DrawerHandle`] into `slot` on every build, and
/// renders a tappable marker that calls `on_tap` with the handle — the
/// harness's way of driving [`ScaffoldScope::of`] through a real gesture
/// dispatch (`GlobalKey` resolution needs the owner-thread registry active,
/// which only `LaidOut::dispatch_pointer_*`'s `enter_owner_scope` provides).
#[derive(Clone, StatelessView)]
struct HandleProbe {
    slot: Rc<RefCell<Option<DrawerHandle>>>,
    on_tap: Rc<dyn Fn(&DrawerHandle)>,
}

impl StatelessView for HandleProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let handle = ScaffoldScope::of(ctx);
        let _prev = self.slot.borrow_mut().replace(handle.clone());
        let on_tap = Rc::clone(&self.on_tap);
        GestureDetector::new()
            .on_tap(move |_cx| on_tap(&handle))
            .child(SizedBox::new(20.0, 20.0))
    }
}

// ============================================================================
// 1. Closed state: drawer child unmounted, body tappable inside AND outside
//    the edge strip, only the strip's own `RenderListener` is added.
// ============================================================================

// ============================================================================
// 2. Mid-drag panel geometry.
// ============================================================================

// ============================================================================
// 3. Scrim.
// ============================================================================

/// The scrim is a tappable `ColoredBox`
/// (`RenderDecoratedBox`) that closes the drawer on tap
/// (`drawerBarrierDismissible`, default `true`), and stays gone once the
/// close fling settles — not just reported as closed via
/// [`DrawerHandle::is_drawer_open`] (the synchronous half, already pinned at
/// the `DrawerControllerCore` unit level by
/// `open_fires_on_open_changed_synchronously`), but actually unmounted, so
/// a stale alpha-0 scrim can never sit there eating every body tap after the
/// drawer visually looks closed.
///
/// This requires driving the fling to completion, which needs the
/// controller's `AnimationController` to actually be ticked — see
/// [`themed_animated`]'s doc for why a bare [`themed`] tree silently never
/// ticks it at all (a prior version of this test used exactly that gap to
/// justify only checking `is_drawer_open`, not the mount; the mount check
/// below is what would have caught it). [`FLING_SETTLE_PUMPS`] budgets for
/// the FULL 1.0 -> 0.0 fling distance a close from a fully-open, fully-rested
/// drawer takes (a `close()` fired from a drag-shortened distance, as in
/// `drag_open_then_close_round_trip_updates_the_handles_tracked_state`, has
/// less distance to cover and settles inside the smaller [`PUMPS`] budget;
/// this test's full-distance close does not).
///
/// Red-check: drop the `.on_tap(move || close_core.close())` wiring from
/// `open_panel`'s scrim detector in `drawer.rs` — the scrim still mounts
/// (this test's first two assertions still pass) but the tap does nothing,
/// so the final mount-check assertion fails (the scrim never unmounts).
pub fn scrim_mounts_when_open_and_a_tap_closes_the_drawer() {
    let vsync = Vsync::new();
    let handle_slot: Rc<RefCell<Option<DrawerHandle>>> = Rc::new(RefCell::new(None));
    let probe = HandleProbe {
        slot: Rc::clone(&handle_slot),
        on_tap: Rc::new(|_handle: &DrawerHandle| {}),
    };
    let mut laid = lay_out_animated(
        themed_animated(
            Scaffold::new()
                .drawer(Drawer::new())
                // Widened so the opening drag stays within the strip's own
                // hit-test bounds — see the module docs' "harness
                // limitation" note (no pointer capture in this harness).
                .drawer_edge_drag_width(400.0)
                .body(probe),
            &vsync,
        ),
        tight(400.0, 800.0),
        vsync,
    );
    let handle = handle_slot
        .borrow()
        .clone()
        .expect("HandleProbe captures the handle on its first build");

    // Open via a full-width drag past the fling-free threshold (value > 0.5).
    // Two moves: the first crosses the recognizer's slop (see
    // `mid_drag_panel_offset_follows_the_value_minus_one_times_width_formula`'s
    // comment for why), the second carries the value well past 0.5 so
    // `_settle` opens regardless of whatever velocity this near-instantaneous
    // dispatch sequence happens to compute.
    laid.dispatch_pointer_down(5.0, 400.0);
    laid.dispatch_pointer_move(30.0, 400.0);
    laid.dispatch_pointer_move(395.0, 400.0);
    laid.dispatch_pointer_up(395.0, 400.0);
    for _ in 0..FLING_SETTLE_PUMPS {
        laid.pump_for(FRAME);
    }

    assert!(
        laid.try_find_by_render_type("RenderDecoratedBox").is_some(),
        "the scrim must mount once the drawer is open"
    );
    assert!(
        handle.is_drawer_open(),
        "settling the open drag must report the drawer open"
    );

    // Tap the scrim, away from the panel itself (panel occupies the left
    // `width` pixels once open; tap near the right edge, clear of it).
    laid.dispatch_pointer_down(390.0, 400.0);
    laid.dispatch_pointer_up(390.0, 400.0);

    assert!(
        !handle.is_drawer_open(),
        "the scrim tap must call close(), which reports the drawer closed immediately \
         (before any fling animation runs)"
    );

    for _ in 0..FLING_SETTLE_PUMPS {
        laid.pump_for(FRAME);
    }
    assert!(
        laid.try_find_by_render_type("RenderDecoratedBox").is_none(),
        "once the close fling actually settles, the scrim must unmount — a stuck, \
         alpha-0-but-still-hit-testable scrim would otherwise eat every body tap forever"
    );
}

// ============================================================================
// 4. Handle: ScaffoldScope::of, open/close, no-flash mount, has_drawer.
// ============================================================================

/// The settle logic's two branches disagree here, which is the whole point: the drag
/// is released **below** the halfway mark, so the position branch would close
/// the drawer, while a qualifying opening velocity flings it open. Velocity
/// is checked first, so open is the correct
/// outcome — and observing it proves the release velocity this harness feeds
/// the recognizer is real.
///
/// Every other drag test in this file deliberately releases *past* 0.5, where
/// both branches agree and the measured velocity is unobservable — see
/// `scrim_mounts_when_open_and_a_tap_closes_the_drawer`, whose comment says so
/// outright. That is why the harness could feed degenerate sample timestamps
/// for as long as it did without a single test noticing.
///
/// Red-check: drop the `clock().advance(POINTER_SAMPLE_INTERVAL)` from
/// `tests/common::LaidOut::dispatch_pointer_move`. Every velocity sample then
/// lands on the same instant, the tracker's zero-span guard reports zero
/// velocity, `_settle` falls through to the position branch, and the drawer
/// closes instead — failing the assertion below.
pub fn a_fast_release_below_halfway_flings_the_drawer_open_rather_than_snapping_shut() {
    let vsync = Vsync::new();
    let handle_slot: Rc<RefCell<Option<DrawerHandle>>> = Rc::new(RefCell::new(None));
    let probe = HandleProbe {
        slot: Rc::clone(&handle_slot),
        on_tap: Rc::new(|_handle: &DrawerHandle| {}),
    };

    let mut laid = lay_out_animated(
        themed_animated(
            Scaffold::new()
                .drawer(Drawer::new())
                // Widened so the whole drag path stays within the strip's own
                // hit-test bounds — see the module docs' "harness limitation"
                // note (no pointer capture in this harness).
                .drawer_edge_drag_width(400.0)
                .body(probe),
            &vsync,
        ),
        tight(400.0, 800.0),
        vsync,
    );
    let handle = handle_slot
        .borrow()
        .clone()
        .expect("HandleProbe captures the handle on its first build");

    // Three moves so the least-squares fit gets its `MIN_SAMPLE_SIZE` (3)
    // samples; the first also spends the recognizer's slop. The last lands at
    // x=100, i.e. value ~= 100/304 ~= 0.33 — comfortably below the 0.5 the
    // position branch snaps on. At the harness's 8ms sample spacing the
    // release velocity is on the order of 10^4 px/s, far above the drawer's
    // 365 px/s `_kMinFlingVelocity`.
    laid.dispatch_pointer_down(5.0, 400.0);
    laid.dispatch_pointer_move(40.0, 400.0);
    laid.dispatch_pointer_move(70.0, 400.0);
    laid.dispatch_pointer_move(100.0, 400.0);
    laid.dispatch_pointer_up(100.0, 400.0);

    for _ in 0..FLING_SETTLE_PUMPS {
        laid.pump_for(FRAME);
    }

    assert!(
        handle.is_drawer_open(),
        "a fast opening release must fling the drawer open even though it was \
         let go below halfway; reporting closed means the release velocity \
         reaching `_settle` was zero"
    );
}

// ============================================================================
// 5. on_drawer_changed: the app author's callback forwards through Scaffold.
// ============================================================================

// ============================================================================
// 6. End-drawer mirror + dynamic ordering with both drawers configured.
// ============================================================================
