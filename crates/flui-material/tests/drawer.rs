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
//! `crates/flui-material/src/drawer.rs`'s own test module — deterministic
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
//! happens. [`themed_animated`] wraps `vsync` in [`flui_widgets::VsyncScope`]
//! so the tree-side registration and the binding-side pump are the SAME
//! clock; plain [`themed`] (no `VsyncScope`) is for tests that only need
//! synchronous effects (a bare `set_value`, or a same-tick status flip) and
//! never call `pump_for`/`tick_all` expecting real animation progress.
//!
//! # Harness limitation: no pointer capture
//!
//! `LaidOut::dispatch_pointer_move` re-hit-tests at the NEW position on every
//! call (`crates/flui-material/tests/common/mod.rs`'s own doc: "no pointer
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

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out, lay_out_animated, tight};
use flui_animation::Vsync;
use flui_foundation::RenderId;
use flui_material::{Drawer, DrawerHandle, Scaffold, ScaffoldScope, Theme, ThemeData};
use flui_types::Color;
use flui_view::prelude::*;
use flui_widgets::{ColoredBox, GestureDetector, MediaQuery, MediaQueryData, SizedBox, VsyncScope};

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

/// The drawer panel's own width-forcing `RenderConstrainedBox`, disambiguated
/// from the scrim's `SizedBox::expand` (which also lowers to
/// `RenderConstrainedBox`, but reports the Stack's full width, not the
/// configured drawer width) by matching on `configured_width`.
fn find_panel(laid: &common::LaidOut, configured_width: f32) -> RenderId {
    laid.find_all_by_render_type("RenderConstrainedBox")
        .into_iter()
        .find(|&id| (laid.size(id).width.get() - configured_width).abs() < 1.0)
        .expect("the drawer's width-forcing ConstrainedBox must be mounted")
}

/// A tappable body marker filling the whole scaffold body area: increments
/// `taps` on a primary tap. `Scaffold`'s body slot is loosely constrained
/// (a body may legally be smaller than the available area — see
/// `crates/flui-material/src/scaffold.rs`'s module docs), so an explicit
/// `SizedBox` is what actually makes it cover every test coordinate below,
/// not `ColoredBox` alone (which would collapse to zero size under loose
/// constraints).
fn tap_counter(taps: Arc<AtomicUsize>) -> impl IntoView {
    SizedBox::new(400.0, 800.0).child(
        GestureDetector::new()
            .on_tap(move || {
                taps.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
    )
}

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
        *self.slot.borrow_mut() = Some(handle.clone());
        let on_tap = Rc::clone(&self.on_tap);
        GestureDetector::new()
            .on_tap(move || on_tap(&handle))
            .child(SizedBox::new(20.0, 20.0))
    }
}

// ============================================================================
// 1. Closed state: drawer child unmounted, body tappable inside AND outside
//    the edge strip, only the strip's own `RenderListener` is added.
// ============================================================================

/// Flutter parity: `_buildDrawer`'s dismissed branch (`drawer.dart`) mounts
/// only the translucent edge-strip `GestureDetector` — no `Drawer` content,
/// no scrim. `HitTestBehavior::translucent` means the body underneath stays
/// tappable both inside and outside the strip's bounds.
///
/// Red-check: swap the edge strip's `.behavior(HitTestBehavior::Translucent)`
/// for the default `DeferToChild` — the in-strip tap (which hits the
/// zero-content strip, not a further child) stops reaching the body, and
/// the first assertion fails.
#[test]
fn closed_drawer_mounts_only_the_edge_strip_and_the_body_stays_tappable_through_it() {
    let taps = Arc::new(AtomicUsize::new(0));
    let laid = lay_out(
        themed(
            Scaffold::new()
                .drawer(Drawer::new().child(SizedBox::new(1.0, 1.0)))
                .body(tap_counter(Arc::clone(&taps))),
        ),
        tight(400.0, 800.0),
    );

    // Only Scaffold's own Material — the Drawer's own Material never mounts
    // while dismissed.
    assert_eq!(
        laid.find_all_by_render_type("RenderPhysicalShape").len(),
        1,
        "the drawer's content must not mount while closed"
    );

    // A tap INSIDE the default 20px edge strip (x < 20) still reaches the
    // body underneath (translucent).
    laid.dispatch_pointer_down(5.0, 400.0);
    laid.dispatch_pointer_up(5.0, 400.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a tap inside the closed edge strip must still reach the body"
    );

    // A tap OUTSIDE the strip reaches the body too (nothing there to
    // intercept it at all).
    laid.dispatch_pointer_down(200.0, 400.0);
    laid.dispatch_pointer_up(200.0, 400.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        2,
        "a tap outside the closed edge strip must reach the body"
    );
}

// ============================================================================
// 2. Mid-drag panel geometry.
// ============================================================================

/// Flutter parity: `_move` positions the panel via the inner `Align`'s
/// `centerEnd` alignment inside a box `value * width` wide — the panel's
/// absolute x-offset resolves to `(value - 1) * width` (see
/// `crates/flui-material/src/drawer.rs`'s module docs' named-divergence note
/// for why `width` is the *configured*, not measured, panel width here).
///
/// Red-check: swap `_directionFactor`'s sign (make `Start` `-1.0`) — the
/// drag moves the value the wrong way and the offset assertion fails.
#[test]
fn mid_drag_panel_offset_follows_the_value_minus_one_times_width_formula() {
    let width = 280.0;
    let mut laid = lay_out(
        themed(
            Scaffold::new()
                .drawer(Drawer::new().width(width))
                // Widened so the whole drag path stays within the strip's
                // own hit-test bounds — see the module docs' "harness
                // limitation" note (no pointer capture in this harness).
                .drawer_edge_drag_width(width + 20.0),
        ),
        tight(400.0, 800.0),
    );

    // With no competing recognizer on the edge strip, closing the arena
    // after Down accepts the drag by default. `DragStartBehavior::Start`
    // therefore starts at the down position and the first move contributes
    // its full delta. Move past 0.5 so Scaffold's threshold callback rebuilds
    // the controller while the drag is live; `did_update_view` must preserve
    // the partial value rather than feeding `is_open=true` back as a command
    // to snap fully open.
    let value = 0.6;
    laid.dispatch_pointer_down(5.0, 400.0);
    laid.dispatch_pointer_move(5.0 + width * value, 400.0);

    // The value change (`set_value`) only *schedules* a rebuild; drain it so
    // the tree reflects the now-not-dismissed status (the open branch,
    // panel mounted) before reading its geometry.
    laid.tick();

    let panel = find_panel(&laid, width);
    let dx = laid.absolute_offset(panel).dx.get();

    // value ~= 0.6 => offset = (0.6 - 1) * width.
    let expected = (value - 1.0) * width;
    assert!(
        (dx - expected).abs() < 5.0,
        "mid-drag panel offset must track (value - 1) * width: got {dx}, expected ~{expected}"
    );
}

// ============================================================================
// 3. Scrim.
// ============================================================================

/// Flutter parity: the scrim is a tappable `ColoredBox`
/// (`RenderDecoratedBox`) that closes the drawer on tap
/// (`drawerBarrierDismissible`, default `true`), and stays gone once the
/// close fling settles — not just reported as closed via
/// [`DrawerHandle::is_drawer_open`] (the synchronous half, already pinned at
/// the `DrawerControllerCore` unit level by
/// `close_fires_on_open_changed_synchronously`), but actually unmounted, so
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
#[test]
fn scrim_mounts_when_open_and_a_tap_closes_the_drawer() {
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
        laid.find_by_render_type("RenderDecoratedBox").is_some(),
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
        laid.find_by_render_type("RenderDecoratedBox").is_none(),
        "once the close fling actually settles, the scrim must unmount — a stuck, \
         alpha-0-but-still-hit-testable scrim would otherwise eat every body tap forever"
    );
}

// ============================================================================
// 4. Handle: ScaffoldScope::of, open/close, no-flash mount, has_drawer.
// ============================================================================

/// `ScaffoldScope::of` resolves without a `Scaffold` panic and reflects the
/// configured slot immediately — the data half of the [`DrawerHandle`]
/// bridge, independent of the `GlobalKey` half `open_drawer`/`close_drawer`
/// need (see the note below).
///
/// **Coverage note**: `DrawerHandle::open_drawer`/`close_drawer`'s
/// `GlobalKey::with_current_state` resolution is NOT exercised end-to-end
/// here. `HeadlessBinding` (this crate's test harness) never installs the
/// owner-thread `GlobalKey` registry — only `UiRealm::enter` (production) or
/// `flui_view::test_only_set_global_key_registry` (a lower-level hook this
/// harness doesn't wire up) activate it — so a call through
/// `ScaffoldScope::of(ctx).open_drawer()` in a headless test silently
/// resolves to "no element registered" and no-ops (proven safe, not proven
/// *effective*, by `crate::drawer::tests::drawer_handle_open_drawer_on_an_unmounted_scaffold_is_a_no_op`
/// in `src/drawer.rs`). Wiring the registry into `HeadlessBinding` is
/// headless-harness infrastructure outside this feature's scope. The
/// underlying open/close/mount-timing behavior `open_drawer`/`close_drawer`
/// delegate to is fully covered without the `GlobalKey` hop: `DrawerControllerCore`'s
/// own `open`/`close` unit tests below prove the immediate `on_open_changed`
/// firing and pre-settle value, and
/// `mounts_immediately_on_the_first_forward_tick_with_no_flash` (next) proves
/// the same status-driven rebuild timing end-to-end via a real drag.
#[test]
fn scaffold_scope_of_reflects_has_drawer_without_a_scaffold_panic() {
    let handle_slot: Rc<RefCell<Option<DrawerHandle>>> = Rc::new(RefCell::new(None));
    let probe = HandleProbe {
        slot: Rc::clone(&handle_slot),
        on_tap: Rc::new(|_handle: &DrawerHandle| {}),
    };

    let _laid = lay_out(
        themed(Scaffold::new().drawer(Drawer::new()).body(probe)),
        tight(400.0, 800.0),
    );

    let handle = handle_slot
        .borrow()
        .clone()
        .expect("HandleProbe captures the handle on its first build");
    assert!(
        handle.has_drawer(),
        "has_drawer must reflect the configured drawer slot"
    );
    assert!(
        !handle.has_end_drawer(),
        "has_end_drawer must stay false when no end_drawer is configured"
    );
    assert!(
        !handle.is_drawer_open(),
        "a freshly-mounted drawer starts closed"
    );
}

/// Flutter parity: the panel mounts (transitions off the dismissed branch)
/// the instant the controller leaves `AnimationStatus::Dismissed` — driven
/// here by the FIRST real drag update, which is exactly the
/// same status-driven rebuild path `open()`'s fling takes (both change
/// status via the controller, and `DrawerControllerState`'s status listener
/// reschedules the build regardless of which call changed it) — see the
/// coverage note on the previous test for why this substitutes for a direct
/// `open()` call in this headless harness.
///
/// Red-check: in `DrawerControllerState::init_state`, drop the
/// `add_status_listener` registration (keep only the value listener) — a
/// value-only change from `Dismissed` (0.0) is still a *value* change too,
/// so this particular red-check does not actually distinguish the two
/// listeners; the real regression it guards is structural (removing either
/// listener removes a rebuild trigger this test's single `tick()` relies on
/// to see the mount at all).
#[test]
fn mounts_immediately_on_the_first_forward_tick_with_no_flash() {
    let width = 304.0;
    let mut laid = lay_out(
        themed(
            Scaffold::new()
                .drawer(Drawer::new())
                // Widened for consistency with the longer drag cases in this
                // file; the one-pixel probe below also fits the default strip.
                .drawer_edge_drag_width(100.0),
        ),
        tight(400.0, 800.0),
    );

    assert_eq!(
        laid.find_all_by_render_type("RenderPhysicalShape").len(),
        1,
        "closed: only Scaffold's own Material"
    );

    // A lone recognizer wins by default when the Down arena closes, so the
    // first one-pixel move is already a real update. The value barely moves
    // off 0, but the status leaves Dismissed, which is what the mount gates
    // on (not the value itself).
    laid.dispatch_pointer_down(5.0, 400.0);
    laid.dispatch_pointer_move(6.0, 400.0);
    laid.tick();

    assert_eq!(
        laid.find_all_by_render_type("RenderPhysicalShape").len(),
        2,
        "leaving Dismissed must mount the drawer's content on the very same tick"
    );
    let panel = find_panel(&laid, width);
    let dx_at_mount = laid.absolute_offset(panel).dx.get();
    assert!(
        (dx_at_mount - (-width)).abs() < 10.0,
        "no flash: the panel must mount close to fully off-screen (value near 0), \
         not already open: dx={dx_at_mount}"
    );
}

/// A full open-then-close drag round trip, reading [`DrawerHandle::is_drawer_open`]
/// after each — proves `Scaffold`'s `on_open_changed` wiring keeps the
/// published handle's tracked state in sync with the real controller.
#[test]
fn drag_open_then_close_round_trip_updates_the_handles_tracked_state() {
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
                // The CLOSING drag below needs no such widening: it drags
                // the already-open panel, whose own gesture detector spans
                // the whole scaffold.
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

    // Open: drag most of the way across, release, let it settle. Both this
    // drag and the closing one below leave the fling only a SHORT distance
    // to cover (the drag itself already carried the value most of the way),
    // so the smaller `PUMPS` budget — not `FLING_SETTLE_PUMPS` — is enough.
    laid.dispatch_pointer_down(5.0, 400.0);
    laid.dispatch_pointer_move(30.0, 400.0);
    laid.dispatch_pointer_move(395.0, 400.0);
    laid.dispatch_pointer_up(395.0, 400.0);
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert!(
        handle.is_drawer_open(),
        "after settling open, the handle must report it open"
    );

    // Close: drag from the (now open) panel back toward the left edge.
    laid.dispatch_pointer_down(390.0, 400.0);
    laid.dispatch_pointer_move(360.0, 400.0);
    laid.dispatch_pointer_move(5.0, 400.0);
    laid.dispatch_pointer_up(5.0, 400.0);
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert!(
        !handle.is_drawer_open(),
        "after settling closed, the handle must report it closed"
    );
    assert_eq!(
        laid.find_all_by_render_type("RenderPhysicalShape").len(),
        1,
        "closed again: the drawer's content must unmount"
    );
}

// ============================================================================
// 5. on_drawer_changed: the app author's callback forwards through Scaffold.
// ============================================================================

/// Flutter parity: `Scaffold.onDrawerChanged` — `ScaffoldState._drawerOpenedCallback`
/// forwards to `widget.onDrawerChanged` whenever the drawer's opened bool
/// actually changes (`build_drawer_controller`'s `on_open_changed` closure,
/// wired alongside `DrawerHandle::set_drawer_opened` and the rebuild). Only
/// pinned at the `DrawerControllerCore` unit level until now (the 0.5-crossing
/// firing path is covered there), never through `Scaffold`'s own relay — a
/// dropped forward would leave every OTHER assertion in this file green,
/// since none of them read the app-author callback.
///
/// Asserts the recorded VALUES and ORDER, not just "fired at least once":
/// crossing 0.5 while dragging open must forward `true` exactly once: no
/// double-fire, no fire-before-crossing.
///
/// Red-check: delete the `if let Some(callback) = &on_changed { callback(opened); }`
/// line from `ScaffoldState::build_drawer_controller`'s `on_open_changed`
/// closure — `events` stays empty and the first assertion after the drag
/// fails.
#[test]
fn on_drawer_changed_forwards_to_the_app_authors_callback() {
    let events: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let events_for_callback = Rc::clone(&events);

    let laid = lay_out(
        themed(
            Scaffold::new()
                .drawer(Drawer::new())
                // Widened so the whole drag path stays within the strip's
                // own hit-test bounds — see the module docs' "harness
                // limitation" note.
                .drawer_edge_drag_width(400.0)
                .on_drawer_changed(move |opened| {
                    events_for_callback.borrow_mut().push(opened);
                }),
        ),
        tight(400.0, 800.0),
    );

    assert!(
        events.borrow().is_empty(),
        "no callback before any interaction"
    );

    // Drag past 0.5. The first move already reports a delta because the
    // closed edge strip has no competing recognizer.
    laid.dispatch_pointer_down(5.0, 400.0);
    laid.dispatch_pointer_move(185.0, 400.0);
    assert_eq!(
        *events.borrow(),
        vec![true],
        "crossing 0.5 while opening must forward on_drawer_changed(true) exactly once"
    );

    // Drag back below 0.5, same contact still in progress.
    laid.dispatch_pointer_move(30.0, 400.0);
    assert_eq!(
        *events.borrow(),
        vec![true, false],
        "crossing back below 0.5 must forward on_drawer_changed(false)"
    );
}

// ============================================================================
// 6. End-drawer mirror + dynamic ordering with both drawers configured.
// ============================================================================

/// `id`'s full descendant set (itself included) — a small DFS over the
/// harness's `children()` helper, used below to attribute a leaf render node
/// (e.g. a specific drawer's edge-strip `RenderAlign`) to one of two
/// sibling subtrees.
fn subtree(laid: &common::LaidOut, root: RenderId, out: &mut Vec<RenderId>) {
    out.push(root);
    for child in laid.children(root) {
        subtree(laid, child, out);
    }
}

/// Both drawers' edge-drag width in the ordering tests below — wide enough
/// that a drag reaching value `> 0.5` (crossing the [`DrawerAlignment`]'s own
/// direction-mirrored threshold) stays entirely within its OWN strip's
/// bounds (`0..200` for the start drawer, `200..400` for the end drawer —
/// disjoint, so the drag can only ever hit the strip it started on). See the
/// module docs' "harness limitation" note.
const ORDERING_EDGE_DRAG_WIDTH: f32 = 200.0;

/// Shared by both arms of the dynamic-order pin below: mounts a `Scaffold`
/// with both `drawer` and `end_drawer` configured, opens whichever side
/// `open_is_end` selects via a REAL drag past `0.5` on that side's own edge
/// strip (crossing `0.5` fires `on_open_changed` synchronously — see
/// `move_by_fires_on_open_changed_exactly_once_when_crossing_half` in
/// `src/drawer.rs`'s own test module — so no fling/tick is needed to
/// observe the dynamic-order effect), and asserts that side's slot ends up
/// LAST among the scaffold's direct children. Flutter parity:
/// `Scaffold.build`'s `if (_endDrawerOpened.value) { buildDrawer,
/// buildEndDrawer } else { buildEndDrawer, buildDrawer }`
/// (`scaffold.dart:3211-3217`) — the LAST-added slot paints on top of, and
/// hit-tests before, the other (still-closed) drawer's edge strip.
///
/// Not driven through [`DrawerHandle::open_drawer`]/`open_end_drawer`
/// (unlike a prior version of this helper): that path resolves via
/// `GlobalKey::with_current_state`, which this harness's
/// `HeadlessBinding` never activates (see
/// `scaffold_scope_of_reflects_has_drawer_without_a_scaffold_panic`'s
/// coverage note) — calling it here silently no-ops, leaving both drawers
/// closed and giving every geometry lookup below nothing genuine to
/// distinguish (an earlier version of this test passed against exactly that
/// no-op, because its "any `RenderAlign` present" check could not tell a
/// still-closed one-`Align` edge strip from a genuinely two-`Align` open
/// panel — fixed below by requiring the opened side's subtree to carry
/// TWO `RenderAlign`s, not merely at least one).
///
/// Both arms are needed: `ScaffoldState::build`'s actual `is_end_drawer_open`
/// branch already pushes `[drawer, end_drawer]` whenever the end drawer is
/// the one open — the SAME order a hardcoded `[drawer, end_drawer]` (i.e.
/// dropping the `is_end_drawer_open` check entirely) would also produce. A
/// single "end drawer open" test therefore cannot distinguish the real
/// branch from that hardcoded mutant; only the *other* arm (start drawer
/// open, which correctly needs `[end_drawer, drawer]`) does.
///
/// Red-check: hardcode the child-push order in `ScaffoldState::build` to
/// always be `[drawer, end_drawer]` (drop the `is_end_drawer_open` branch
/// entirely) — `with_end_drawer_open_its_slot_is_the_last_scaffold_child`
/// (below) keeps passing (that IS the order the hardcoded mutant produces
/// too), but `with_start_drawer_open_its_slot_is_the_last_scaffold_child`
/// fails (it needs `[end_drawer, drawer]`, which the mutant never produces).
fn assert_opened_drawer_slot_is_last(open_is_end: bool) {
    let laid_width = 400.0_f32;
    let mut laid = lay_out(
        themed(
            Scaffold::new()
                .drawer(Drawer::new())
                .end_drawer(Drawer::new())
                .drawer_edge_drag_width(ORDERING_EDGE_DRAG_WIDTH),
        ),
        tight(laid_width, 800.0),
    );

    // Both closed: exactly two edge-strip `RenderAlign`s, each with exactly
    // one `RenderAlign` in its subtree (an OPEN panel has two — outer and
    // inner — see the doc above on why "any `RenderAlign`" isn't enough).
    assert_eq!(
        laid.find_all_by_render_type("RenderAlign").len(),
        2,
        "both drawers mount a closed edge strip"
    );

    // A drag past 0.5, entirely within the OWN strip's bounds (see
    // `ORDERING_EDGE_DRAG_WIDTH`'s doc): rightward from the left edge opens
    // the start drawer; leftward from the right edge opens the end drawer
    // (direction factor -1 mirrors the sign — see
    // `settle_fling_direction_is_mirrored_for_an_end_drawer`'s equivalent
    // pin for `settle`).
    if open_is_end {
        laid.dispatch_pointer_down(laid_width - 5.0, 400.0);
        laid.dispatch_pointer_move(laid_width - 30.0, 400.0); // crosses slop
        laid.dispatch_pointer_move(laid_width - 30.0 - 155.0, 400.0); // past 0.5
    } else {
        laid.dispatch_pointer_down(5.0, 400.0);
        laid.dispatch_pointer_move(30.0, 400.0); // crosses slop
        laid.dispatch_pointer_move(30.0 + 155.0, 400.0); // past 0.5
    }
    laid.tick();

    let scaffold_layout = laid
        .find_by_render_type("RenderCustomMultiChildLayoutBox")
        .expect("Scaffold's own multi-child layout must be mounted");
    let top_level_children = laid.children(scaffold_layout);

    // Each top-level child's full descendant set: the OPENED drawer's slot
    // is whichever one now carries TWO `RenderAlign`s (the open panel's
    // outer + inner `Align`); the still-closed one carries exactly one (its
    // edge strip).
    let subtrees: Vec<Vec<RenderId>> = top_level_children
        .iter()
        .map(|&top_child| {
            let mut nodes = Vec::new();
            subtree(&laid, top_child, &mut nodes);
            nodes
        })
        .collect();
    let all_aligns: std::collections::HashSet<RenderId> = laid
        .find_all_by_render_type("RenderAlign")
        .into_iter()
        .collect();
    let align_counts: Vec<usize> = subtrees
        .iter()
        .map(|nodes| nodes.iter().filter(|id| all_aligns.contains(id)).count())
        .collect();

    let opened_index = align_counts
        .iter()
        .position(|&count| count == 2)
        .unwrap_or_else(|| {
            panic!(
                "one top-level child's subtree must carry the opened drawer's two Aligns \
                 (outer + inner) — align_counts={align_counts:?}; if every count is 1, the \
                 drag never actually opened anything"
            )
        });
    let closed_index = align_counts
        .iter()
        .position(|&count| count == 1)
        .expect("one top-level child's subtree must carry the still-closed drawer's one Align");

    assert!(
        opened_index > closed_index,
        "the OPENED drawer's slot must be the LAST scaffold child (paints on top of, \
         hit-tests before, the still-closed drawer): open_is_end={open_is_end}, \
         closed_index={closed_index}, opened_index={opened_index}, align_counts={align_counts:?}"
    );
}

#[test]
fn with_end_drawer_open_its_slot_is_the_last_scaffold_child() {
    assert_opened_drawer_slot_is_last(true);
}

#[test]
fn with_start_drawer_open_its_slot_is_the_last_scaffold_child() {
    assert_opened_drawer_slot_is_last(false);
}
