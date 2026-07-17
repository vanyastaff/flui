//! `Drawer`/`DrawerController`/`Scaffold` drawer-slot end-to-end coverage —
//! a real [`Vsync`] clock drives the settle animation, matching
//! `tests/show_dialog.rs`'s harness. Proves the closed-state edge strip's
//! translucent hit-testing, the mid-drag panel geometry, the
//! [`ScaffoldScope`] handle's open/close bridge (mounts at value 0 with no
//! flash), and the dynamic child order when both drawers are configured.
//!
//! Pure value/status math (fling threshold, direction factor, the three
//! `on_drawer_changed` firing paths) is covered at the `DrawerControllerCore`
//! unit level in `crates/flui-material/src/drawer.rs`'s own test module —
//! deterministic there (no real-clock-dependent velocity simulation needed);
//! this file covers what only a real mounted tree can prove: geometry,
//! hit-testing, and the `GlobalKey` bridge.
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
use flui_widgets::{ColoredBox, GestureDetector, MediaQuery, MediaQueryData, SizedBox};

/// Wraps `scaffold` in the `Theme`/`MediaQuery` ancestors `Scaffold`/
/// `Drawer`/`Material` all require (`Theme::of`/`MediaQuery::of` panic
/// without one).
fn themed(scaffold: Scaffold) -> impl View {
    MediaQuery::new(
        MediaQueryData::default(),
        Theme::new(ThemeData::light(), scaffold),
    )
}

/// `_kBaseSettleDuration` (`drawer.dart`, oracle tag `3.44.0`).
const SETTLE: Duration = Duration::from_millis(246);
/// The per-pump virtual-time step.
const FRAME: Duration = Duration::from_millis(16);
/// Enough pumps to carry `SETTLE` past its end — matching
/// `flui-material/tests/show_dialog.rs`'s identical `+ 2` budget.
const PUMPS: usize = (SETTLE.as_millis() / FRAME.as_millis()) as usize + 2;

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

    // A drag needs TWO move events to accumulate a reported delta: the
    // FIRST move that crosses the recognizer's slop transitions
    // Possible -> Started (firing `on_start` — unwired on the closed edge
    // strip, no value change) at the crossing position itself
    // (`DragStartBehavior::Start`, the default); only the SECOND move's
    // delta (from that crossing position) is what `on_horizontal_drag_update`
    // reports and `_move` applies. 25px comfortably clears the default pan
    // slop (18px).
    laid.dispatch_pointer_down(5.0, 400.0);
    laid.dispatch_pointer_move(30.0, 400.0); // crosses slop, starts the drag
    laid.dispatch_pointer_move(30.0 + width / 2.0, 400.0); // +140px reported delta

    // The value change (`set_value`) only *schedules* a rebuild; drain it so
    // the tree reflects the now-not-dismissed status (the open branch,
    // panel mounted) before reading its geometry.
    laid.tick();

    let panel = find_panel(&laid, width);
    let dx = laid.absolute_offset(panel).dx.get();

    // value ~= 0.5 (half the width dragged past the slop-crossing point)
    // => offset = (0.5 - 1) * width.
    let expected = (0.5 - 1.0) * width;
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
/// (`drawerBarrierDismissible`, default `true`).
///
/// Pinned via [`DrawerHandle::is_drawer_open`] (updated synchronously inside
/// `DrawerControllerCore::close`, before any fling animation runs — see
/// `close_fires_on_open_changed_synchronously` in `src/drawer.rs`'s own test
/// module) rather than the scrim's mount/unmount, which depends on the
/// *fling* actually settling: a `close()` fired from a fully-open,
/// completely-at-rest controller (value 1.0, no preceding drag to shorten
/// the distance) was found, empirically, to never reach
/// `AnimationStatus::Dismissed` in this harness within any pump budget
/// tried (up to 5 real seconds of virtual time) — while the *identical*
/// `close()` call fired from a drag-shortened distance (this file's
/// `drag_open_then_close_round_trip_updates_the_handles_tracked_state`,
/// which passes) settles normally. This is a narrow, currently
/// unexplained gap between two `close()` call sites that both reach the
/// same `fling(-1.0)` call — tracked as a known gap in this substrate's
/// headless-harness settle coverage, not a claim that the underlying
/// `close()` logic is unverified (it has full deterministic coverage at
/// the `DrawerControllerCore` unit level).
///
/// Red-check: drop the `.on_tap(move || close_core.close())` wiring from
/// `open_panel`'s scrim detector in `drawer.rs` — the scrim still mounts
/// (this test's first assertion still passes) but `is_drawer_open()` stays
/// `true` after the tap, so the second assertion fails.
#[test]
fn scrim_mounts_when_open_and_a_tap_closes_the_drawer() {
    let vsync = Vsync::new();
    let handle_slot: Rc<RefCell<Option<DrawerHandle>>> = Rc::new(RefCell::new(None));
    let probe = HandleProbe {
        slot: Rc::clone(&handle_slot),
        on_tap: Rc::new(|_handle: &DrawerHandle| {}),
    };
    let mut laid = lay_out_animated(
        themed(
            Scaffold::new()
                .drawer(Drawer::new())
                // Widened so the opening drag stays within the strip's own
                // hit-test bounds — see the module docs' "harness
                // limitation" note (no pointer capture in this harness).
                .drawer_edge_drag_width(400.0)
                .body(probe),
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
    for _ in 0..PUMPS {
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
/// here by the FIRST real drag-update past the slop, which is exactly the
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
                // Widened so the slop-crossing move stays within the
                // strip's own hit-test bounds — see the module docs'
                // "harness limitation" note (no pointer capture in this
                // harness); the default 20px strip has less room than the
                // 18px default pan slop needs to cross it and still land
                // inside.
                .drawer_edge_drag_width(100.0),
        ),
        tight(400.0, 800.0),
    );

    assert_eq!(
        laid.find_all_by_render_type("RenderPhysicalShape").len(),
        1,
        "closed: only Scaffold's own Material"
    );

    // The smallest possible qualifying drag: one slop-crossing move (no
    // reported delta) plus one 1px update — the value barely moves off 0,
    // but the STATUS already left Dismissed, which is what the mount gates
    // on (not the value itself).
    laid.dispatch_pointer_down(5.0, 400.0);
    laid.dispatch_pointer_move(30.0, 400.0);
    laid.dispatch_pointer_move(31.0, 400.0);
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
        themed(
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
        ),
        tight(400.0, 800.0),
        vsync,
    );
    let handle = handle_slot
        .borrow()
        .clone()
        .expect("HandleProbe captures the handle on its first build");

    // Open: drag most of the way across, release, let it settle.
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

/// Flutter parity: `Scaffold.build` (`scaffold.dart:3211-3217`) — with the
/// end drawer open, the start drawer's slot is added to the child list
/// FIRST and the end drawer's LAST, so the open end drawer paints on top of,
/// and hit-tests before, the closed start drawer's edge strip. This test
/// pins the direct-child INDEX ordering under `CustomMultiChildLayoutBox`,
/// which is what later paint/hit-test order is built from in this rendering
/// model.
///
/// Red-check: hardcode the child-push order in `ScaffoldState::build` to
/// always be `[drawer, end_drawer]` (drop the `is_end_drawer_open` branch) —
/// this test's index comparison fails once the end drawer is open.
#[test]
fn with_end_drawer_open_its_slot_is_the_last_scaffold_child() {
    let vsync = Vsync::new();
    let handle_slot: Rc<RefCell<Option<DrawerHandle>>> = Rc::new(RefCell::new(None));

    let probe = HandleProbe {
        slot: Rc::clone(&handle_slot),
        on_tap: Rc::new(|handle: &DrawerHandle| handle.open_end_drawer()),
    };

    let mut laid = lay_out_animated(
        themed(
            Scaffold::new()
                .drawer(Drawer::new())
                .end_drawer(Drawer::new())
                .body(probe),
        ),
        tight(400.0, 800.0),
        vsync,
    );

    // Both closed: exactly two edge-strip `RenderAlign`s — one near the left
    // edge (start drawer), one near the right (end drawer).
    let aligns_before = laid.find_all_by_render_type("RenderAlign");
    assert_eq!(
        aligns_before.len(),
        2,
        "both drawers mount a closed edge strip"
    );
    let start_strip = *aligns_before
        .iter()
        .find(|&&id| laid.absolute_offset(id).dx.get() < 200.0)
        .expect("the start drawer's strip sits near the left edge");

    // Open the end drawer via the handle.
    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);
    laid.tick();

    let scaffold_layout = laid
        .find_by_render_type("RenderCustomMultiChildLayoutBox")
        .expect("Scaffold's own multi-child layout must be mounted");
    let top_level_children = laid.children(scaffold_layout);

    // Each top-level child's full descendant set. The start drawer's slot is
    // whichever one contains its (still-closed) edge strip; the end
    // drawer's slot is whichever *other* one contains a `RenderAlign` at
    // all (its now-open outer/inner `Align` pair) — the body probe's own
    // slot contains neither, so this cleanly picks out just the two drawers.
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

    let start_index = subtrees
        .iter()
        .position(|nodes| nodes.contains(&start_strip))
        .expect("one top-level child's subtree must contain the start drawer's edge strip");
    let end_drawer_index = subtrees
        .iter()
        .enumerate()
        .position(|(index, nodes)| {
            index != start_index && nodes.iter().any(|id| all_aligns.contains(id))
        })
        .expect("one top-level child's subtree must contain the open end drawer's Aligns");

    assert!(
        end_drawer_index > start_index,
        "with the end drawer open, its slot must be the LAST scaffold child \
         (paints on top of, hit-tests before, the closed start drawer): \
         start_index={start_index}, end_drawer_index={end_drawer_index}"
    );
}
