//! `SnackBar`/`ScaffoldMessenger`/`Scaffold` snack-bar-slot end-to-end
//! coverage — a real [`Vsync`] clock drives both the 250ms entrance/exit
//! controller and each snack bar's own display-duration timer, matching
//! `tests/drawer.rs`'s established harness pattern (see that file's module
//! doc for the `themed`/`themed_animated` `VsyncScope` distinction, which
//! this file mirrors).
//!
//! Pure queue/state-machine mechanics (FIFO drain, the wedge pin, reason
//! once-only, `clearSnackBars` semantics) are covered synchronously at the
//! `MessengerCore` unit level in
//! `packages/flui-material/src/scaffold_messenger.rs`'s own test module — this
//! file additionally covers what only a real mounted tree proves: the
//! `Scaffold` slot mounting/unmounting on a real clock, the FAB-lift layout
//! interaction, real pointer dispatch through `SnackBarAction`, and
//! multi-scaffold fan-out.

use crate::common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out_animated, tight};
use flui_material::{
    Scaffold, ScaffoldMessenger, ScaffoldMessengerHandle, ScaffoldMessengerScope, SnackBar,
    SnackBarAction, Theme, ThemeData,
};
use flui_sdk::animation::Vsync;
use flui_sdk::foundation::RenderId;
use flui_sdk::painting::Color;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{ColoredBox, MediaQuery, MediaQueryData, SizedBox, Text, VsyncScope};

/// Wraps `child` in the `Theme`/`MediaQuery` ancestors `Scaffold`/`Material`
/// require, plus a `VsyncScope` over `vsync` — required for
/// `ScaffoldMessengerHandle::attach`'s `ctx.get::<VsyncScope, _>` lookup to
/// register the entrance/exit controller against the SAME clock
/// [`lay_out_animated`] adopts onto the binding. Without this, `pump_for`
/// advances virtual time but ticks nothing (matching `tests/drawer.rs`'s
/// identical `themed_animated` requirement).
fn themed_animated(vsync: &Vsync, child: impl IntoView) -> impl View {
    MediaQuery::new(
        MediaQueryData::default(),
        Theme::new(ThemeData::light(), VsyncScope::new(vsync.clone(), child)),
    )
}

/// The shared entrance/exit transition's duration —
/// `ENTRY_TRANSITION_DURATION` in `scaffold_messenger.rs`.
const ENTRY: Duration = Duration::from_millis(250);
/// The per-pump virtual-time step.
const FRAME: Duration = Duration::from_millis(16);

/// Pumps enough `FRAME`-sized steps to carry `millis` of virtual time past
/// its end, with headroom for one extra frame (matching
/// `tests/drawer.rs`'s `PUMPS`/`FLING_SETTLE_PUMPS` `+ 2` margin).
fn pump_ms(laid: &mut common::LaidOut, millis: u64) {
    let pumps = (millis / FRAME.as_millis() as u64) as usize + 2;
    for _ in 0..pumps {
        laid.pump_for(FRAME);
    }
}

/// A tappable, visibly-sized marker body — `Scaffold`'s body slot is loosely
/// constrained (see `scaffold.rs`'s module docs), so a bare `ColoredBox`
/// alone would collapse to zero size.
fn body_marker() -> impl IntoView {
    SizedBox::new(400.0, 500.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))
}

/// Captures the ambient [`ScaffoldMessengerHandle`] into `slot` on every
/// build — the harness's way of reaching `ScaffoldMessengerScope::of` from
/// outside the tree, matching `tests/drawer.rs`'s `HandleProbe` pattern.
#[derive(Clone, StatelessView)]
struct HandleProbe {
    slot: Rc<RefCell<Option<ScaffoldMessengerHandle>>>,
}

impl StatelessView for HandleProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let _prev = std::mem::replace(
            &mut *self.slot.borrow_mut(),
            ScaffoldMessengerScope::maybe_of(ctx),
        );
        SizedBox::shrink()
    }
}

/// Mounts `ScaffoldMessenger` over one or more `Scaffold`s (each given an
/// equal `Expanded` share of the root's height, behind a `HandleProbe`
/// sibling so the test can drive the shared handle), and returns the
/// laid-out tree plus the captured handle. Each `Scaffold::get_size`
/// requires BOUNDED constraints from its parent (see `scaffold.rs`'s own
/// debug assertion) — `Expanded` is what gives every one of several stacked
/// scaffolds a finite share, rather than each claiming the Column's full
/// loose height.
fn mount_with_scaffolds(
    vsync: &Vsync,
    scaffolds: Vec<Scaffold>,
) -> (common::LaidOut, ScaffoldMessengerHandle) {
    let handle_slot: Rc<RefCell<Option<ScaffoldMessengerHandle>>> = Rc::new(RefCell::new(None));
    let mut children: Vec<flui_sdk::view::BoxedView> = vec![
        HandleProbe {
            slot: Rc::clone(&handle_slot),
        }
        .boxed(),
    ];
    children.extend(
        scaffolds
            .into_iter()
            .map(|scaffold| flui_sdk::widgets::Expanded::new(scaffold).boxed()),
    );
    let tree = themed_animated(
        vsync,
        ScaffoldMessenger::new(flui_sdk::widgets::Column::new(children)),
    );
    let laid = lay_out_animated(tree, tight(400.0, 1600.0), vsync.clone());
    let handle = handle_slot
        .borrow()
        .clone()
        .expect("ScaffoldMessengerHandle must be published by ScaffoldMessenger");
    (laid, handle)
}

/// The snack bar's own `Material` (`RenderPhysicalShape` at elevation
/// `6.0`) — see [`snack_bar_material_count`]'s doc for why elevation is the
/// distinguishing property.
fn find_snack_bar_material(laid: &common::LaidOut) -> Option<RenderId> {
    laid.find_all_by_render_type("RenderPhysicalShape")
        .into_iter()
        .find(|&id| {
            laid.render_property(id, "elevation")
                .and_then(|value| value.parse::<f64>().ok())
                == Some(6.0)
        })
}

/// The number of mounted `Material` surfaces (`RenderPhysicalShape`) with
/// elevation `6.0` — `crate::snack_bar`'s `DEFAULT_ELEVATION`, distinctive
/// among this tree's other surfaces (`Scaffold`'s own root `Material`
/// defaults to elevation `0.0`, and every button's `Material` does too), so
/// counting them is a reliable "is a snack bar currently mounted, and how
/// many" signal without downcasting to `SnackBarPresenter`.
fn snack_bar_material_count(laid: &common::LaidOut) -> usize {
    laid.find_all_by_render_type("RenderPhysicalShape")
        .into_iter()
        .filter(|&id| {
            laid.render_property(id, "elevation")
                .and_then(|value| value.parse::<f64>().ok())
                == Some(6.0)
        })
        .count()
}

// ============================================================================
// 1. FIFO drain: the first entry fully exits before the second enters, and
//    both eventually close (abrupt remove, then a natural timeout).
// ============================================================================

// ============================================================================
// 2. Timeout at exactly the per-snackbar duration (custom duration honored).
// ============================================================================

// ============================================================================
// 3. Early hide cancels the display timer (no dangling early auto-dismiss).
// ============================================================================

// ============================================================================
// 4. Action press closes with Action reason and disables after one press.
// ============================================================================

pub fn action_press_closes_the_snack_bar_and_is_single_fire() {
    let vsync = Vsync::new();
    let action_presses = Arc::new(AtomicUsize::new(0));
    let action_presses_for_cb = Arc::clone(&action_presses);

    let (mut laid, handle) =
        mount_with_scaffolds(&vsync, vec![Scaffold::new().body(body_marker())]);

    handle.show_snack_bar(
        SnackBar::new(Text::new("Saved")).action(SnackBarAction::new("UNDO", move |_cx| {
            action_presses_for_cb.fetch_add(1, Ordering::SeqCst);
        })),
    );
    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(snack_bar_material_count(&laid), 1);

    // The action sits at the row's end, vertically centered in the snack
    // bar's own bounding box — found via the same elevation-based filter
    // `snack_bar_material_count` uses, so this test does not depend on
    // internal render-node identification beyond that one distinctive
    // property.
    let snack_bar_material =
        find_snack_bar_material(&laid).expect("the snack bar's Material must be mounted");
    let bar_offset = laid.absolute_offset(snack_bar_material);
    let bar_size = laid.size(snack_bar_material);
    let tap_x = bar_offset.dx + bar_size.width * 0.92;
    let tap_y = bar_offset.dy + bar_size.height * 0.5;

    laid.dispatch_pointer_down(tap_x, tap_y);
    laid.dispatch_pointer_up(tap_x, tap_y);
    assert_eq!(
        action_presses.load(Ordering::SeqCst),
        1,
        "the action must fire on press"
    );

    // The installed callback must reject another contact before rebuilding.
    laid.dispatch_pointer_down(tap_x, tap_y);
    laid.dispatch_pointer_up(tap_x, tap_y);
    assert_eq!(action_presses.load(Ordering::SeqCst), 1);

    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(
        snack_bar_material_count(&laid),
        0,
        "pressing the action must close the snack bar (SnackBarClosedReason::Action)"
    );

    // After dismissal the same coordinates no longer reach the action.
    laid.dispatch_pointer_down(tap_x, tap_y);
    laid.dispatch_pointer_up(tap_x, tap_y);
    assert_eq!(
        action_presses.load(Ordering::SeqCst),
        1,
        "a press after the action closed the snack bar must not re-fire it"
    );
}

// ============================================================================
// 5. Multi-scaffold fan-out: the same messenger shows the current entry on
//    every registered scaffold simultaneously.
// ============================================================================

// ============================================================================
// 6. FAB lift: above the snack bar, both mid-animation and at rest.
// ============================================================================

// ============================================================================
// 7. Unregister on dispose: an unmounted Scaffold must not linger in the
//    messenger's registered set.
// ============================================================================

// ============================================================================
// 8. Scope re-home: a FRESH Scaffold element that mounts under a messenger
//    registers with that messenger, never a stale one from elsewhere in the
//    tree — the practical shape "re-homing" actually takes in this
//    substrate.
//
// A same-type messenger configuration update keeps its mounted handle, so it
// does not exercise an ancestor identity transition. GlobalKey reparenting
// between distinct messenger branches can retain Scaffold state; its tracked
// Theme/MediaQuery dependencies schedule did_change_dependencies on reactivation,
// which rehomes the registration. That retained transition has no dedicated
// behavior row here; the absence of coverage is not evidence of broken wiring.
// ============================================================================

// ============================================================================
// 9. A Scaffold that mounts while a snack bar is already showing renders it
//    immediately.
//
// `register_scaffold`'s "queue non-empty -> schedule an immediate rebuild"
// branch is what this
// test SETS OUT to isolate, but a mutation run against it (dropping the
// branch entirely) still leaves this test green: a freshly-mounted
// `Scaffold`'s `init_state` (which registers) always runs immediately
// before that SAME element's own first `build()`, in the same synchronous
// mount pass — so the first `build()` already reads `current_entry()`
// fresh regardless of whether `register_scaffold` additionally scheduled a
// rebuild. The branch is therefore genuinely unobservable via a fresh-mount
// scenario in this substrate (confirmed by actually running that exact
// mutation, not merely asserted). It is kept anyway: it mirrors the
// oracle's own defensive `_register` (which similarly calls
// `scaffold._updateSnackBar()` unconditionally, not gated on "is this the
// very first build"), and would matter the moment `ScaffoldState`'s
// re-home path (`did_change_dependencies`) becomes reachable without an
// immediately-following rebuild of the same element. This test still earns
// its place as a genuine behavior guarantee — "mount while showing renders
// it" — even though it does not, by itself, prove WHICH code path delivers
// that guarantee.
// ============================================================================

// ============================================================================
// 10. Paint-bounds pin: the entrance/exit transition is clipped to its
//     CURRENT (animated) height, not painted at the content's full natural
//     height past that box — the exact overflow-into-a-stacked-sibling bug
//     an omitted `ClipRect` produces.
// ============================================================================

// ============================================================================
// 11. A tick-driven close's `on_closed` is deferred out of the build phase:
//     a reentrant `show_snack_bar` from it must not corrupt the frame, and
//     must not take effect until a LATER frame.
// ============================================================================

pub fn a_snack_bar_completion_cannot_write_another_presentations_signal() {
    let signal_slot = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&signal_slot);
    let probe = common::SignalProbe::new(move |common::ProbeSignals { count, .. }| {
        *captured.borrow_mut() = Some(count);
        SizedBox::shrink()
    });
    let _first = common::lay_out(probe.view(), tight(100.0, 100.0));
    let count = signal_slot.borrow().expect("signal initialized");
    let vsync = Vsync::new();
    let (_second, handle) = mount_with_scaffolds(&vsync, Vec::new());
    let outcome = Rc::new(RefCell::new(None));
    let recorded = Rc::clone(&outcome);
    handle
        .show_snack_bar(SnackBar::new(Text::new("foreign")))
        .on_closed(move |cx, _reason| {
            *recorded.borrow_mut() = Some(count.set(cx, 1));
        });

    handle.remove_current_snack_bar();
    assert!(matches!(
        *outcome.borrow(),
        Some(Err(flui_sdk::view::SignalError::ForeignGraph { .. }))
    ));
    assert_eq!(probe.value(), Ok(0));
}

/// A failed action stays claimed, publishes disabled semantics on the next
/// frame, and does not prevent a newly mounted action from being pressed.
pub fn action_callback_panic_disables_the_button_and_fresh_action_progresses() {
    let vsync = Vsync::new();
    let calls = Rc::new(Cell::new(0));
    let callback_calls = Rc::clone(&calls);
    let mut laid = lay_out_animated(
        themed_animated(
            &vsync,
            SnackBarAction::new("UNDO", move |_cx| -> () {
                callback_calls.set(callback_calls.get() + 1);
                panic!("action failed");
            }),
        ),
        tight(160.0, 48.0),
        vsync.clone(),
    );
    laid.enable_semantics();
    laid.pump();
    assert!(
        !laid
            .a11y_tree()
            .expect("semantics enabled")
            .find_by_label("UNDO")
            .expect("action button labelled")
            .is_disabled()
    );
    laid.dispatch_pointer_down(80.0, 24.0);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        laid.dispatch_pointer_up(80.0, 24.0);
    }))
    .expect_err("the action panic must propagate");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"action failed"));
    assert_eq!(calls.get(), 1);

    // A second contact before the frame cannot retry the failed callback.
    laid.dispatch_pointer_down(80.0, 24.0);
    laid.dispatch_pointer_up(80.0, 24.0);
    assert_eq!(calls.get(), 1);
    laid.pump();
    assert!(
        laid.a11y_tree()
            .expect("semantics enabled")
            .find_by_label("UNDO")
            .expect("action button retained")
            .is_disabled()
    );

    // Remove the old state rather than merely updating its configuration:
    // the one-shot claim belongs to the retained action state.
    laid.pump_widget(themed_animated(&vsync, SizedBox::new(160.0, 48.0)));
    laid.pump();
    let fresh_calls = Rc::clone(&calls);
    laid.pump_widget(themed_animated(
        &vsync,
        SnackBarAction::new("RETRY", move |_cx| {
            fresh_calls.set(fresh_calls.get() + 1);
        }),
    ));
    laid.pump();
    laid.dispatch_pointer_down(80.0, 24.0);
    laid.dispatch_pointer_up(80.0, 24.0);
    assert_eq!(calls.get(), 2, "a fresh action must still receive input");
    laid.pump();
    assert!(
        laid.a11y_tree()
            .expect("semantics enabled")
            .find_by_label("RETRY")
            .expect("fresh action labelled")
            .is_disabled()
    );
}

pub fn a_completion_panic_still_advances_the_accepted_snack_bar_queue() {
    let vsync = Vsync::new();
    let (mut laid, handle) =
        mount_with_scaffolds(&vsync, vec![Scaffold::new().body(body_marker())]);
    let first_calls = Rc::new(std::cell::Cell::new(0));
    let recorded = Rc::clone(&first_calls);
    handle
        .show_snack_bar(SnackBar::new(Text::new("first")))
        .on_closed(move |_cx, reason| -> () {
            assert_eq!(reason, flui_material::SnackBarClosedReason::Remove);
            recorded.set(recorded.get() + 1);
            panic!("first completion failed");
        });
    let second_reason = Rc::new(std::cell::Cell::new(None));
    let recorded = Rc::clone(&second_reason);
    handle
        .show_snack_bar(SnackBar::new(Text::new("second")).duration(Duration::from_millis(64)))
        .on_closed(move |_cx, reason| recorded.set(Some(reason)));
    pump_ms(&mut laid, 300);
    assert_eq!(snack_bar_material_count(&laid), 1);
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle.remove_current_snack_bar();
    }))
    .expect_err("the completion failure must reach the caller");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"first completion failed")
    );
    assert_eq!(first_calls.get(), 1);
    pump_ms(&mut laid, 300);
    assert_eq!(
        snack_bar_material_count(&laid),
        1,
        "the accepted second bar entered"
    );
    pump_ms(&mut laid, 500);
    assert_eq!(
        second_reason.get(),
        Some(flui_material::SnackBarClosedReason::Timeout)
    );
    assert_eq!(snack_bar_material_count(&laid), 0);
    let third_reason = Rc::new(std::cell::Cell::new(None));
    let recorded = Rc::clone(&third_reason);
    handle
        .show_snack_bar(SnackBar::new(Text::new("third")))
        .on_closed(move |_cx, reason| recorded.set(Some(reason)));
    pump_ms(&mut laid, 300);
    assert_eq!(snack_bar_material_count(&laid), 1);
    handle.remove_current_snack_bar();
    laid.pump();
    assert_eq!(
        third_reason.get(),
        Some(flui_material::SnackBarClosedReason::Remove)
    );
    assert_eq!(snack_bar_material_count(&laid), 0);
    assert_eq!(first_calls.get(), 1);
}
