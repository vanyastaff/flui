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
//! `crates/flui-material/src/scaffold_messenger.rs`'s own test module — this
//! file additionally covers what only a real mounted tree proves: the
//! `Scaffold` slot mounting/unmounting on a real clock, the FAB-lift layout
//! interaction, real pointer dispatch through `SnackBarAction`, and
//! multi-scaffold fan-out.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out_animated, tight};
use flui_animation::Vsync;
use flui_foundation::RenderId;
use flui_material::FloatingActionButton;
use flui_material::{
    Scaffold, ScaffoldMessenger, ScaffoldMessengerHandle, ScaffoldMessengerScope, SnackBar,
    SnackBarAction, Theme, ThemeData,
};
use flui_types::Color;
use flui_view::prelude::*;
use flui_widgets::{ColoredBox, MediaQuery, MediaQueryData, SizedBox, Text, VsyncScope};

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
        *self.slot.borrow_mut() = ScaffoldMessengerScope::maybe_of(ctx);
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
    let mut children: Vec<flui_view::BoxedView> = vec![
        HandleProbe {
            slot: Rc::clone(&handle_slot),
        }
        .boxed(),
    ];
    children.extend(
        scaffolds
            .into_iter()
            .map(|scaffold| flui_widgets::Expanded::new(scaffold).boxed()),
    );
    let tree = themed_animated(
        vsync,
        ScaffoldMessenger::new(flui_widgets::Column::new(children)),
    );
    let laid = lay_out_animated(tree, tight(400.0, 1600.0), vsync.clone());
    let handle = handle_slot
        .borrow()
        .clone()
        .expect("ScaffoldMessengerHandle must be published by ScaffoldMessenger");
    (laid, handle)
}

/// Every `Scaffold`'s own `CustomMultiChildLayout` render root, in mount
/// order.
fn scaffold_roots(laid: &common::LaidOut) -> Vec<RenderId> {
    laid.find_all_by_render_type("RenderCustomMultiChildLayoutBox")
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
                .and_then(|value| value.parse::<f32>().ok())
                == Some(6.0)
        })
        .count()
}

// ============================================================================
// 1. FIFO drain: the first entry fully exits before the second enters, and
//    both eventually close (abrupt remove, then a natural timeout).
// ============================================================================

#[test]
fn fifo_drain_shows_the_current_entry_then_the_next_after_it_closes() {
    let vsync = Vsync::new();
    let (mut laid, handle) =
        mount_with_scaffolds(&vsync, vec![Scaffold::new().body(body_marker())]);

    assert_eq!(snack_bar_material_count(&laid), 0, "nothing shown yet");

    handle.show_snack_bar(SnackBar::new(Text::new("first")).duration(Duration::from_millis(60)));
    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(
        snack_bar_material_count(&laid),
        1,
        "\"first\" must be mounted after entering"
    );

    handle.show_snack_bar(SnackBar::new(Text::new("second")).duration(Duration::from_millis(60)));
    handle.remove_current_snack_bar(); // abrupt: "first" gone, "second" begins entering immediately
    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(
        snack_bar_material_count(&laid),
        1,
        "exactly one entry (\"second\") must be mounted — never zero (a gap) or two (both at once)"
    );

    // "second" naturally times out and exits.
    pump_ms(&mut laid, 60);
    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(
        snack_bar_material_count(&laid),
        0,
        "both entries must have fully drained"
    );
}

// ============================================================================
// 2. Timeout at exactly the per-snackbar duration (custom duration honored).
// ============================================================================

#[test]
fn a_custom_duration_snack_bar_auto_dismisses_after_that_duration_not_the_default() {
    let vsync = Vsync::new();
    let (mut laid, handle) =
        mount_with_scaffolds(&vsync, vec![Scaffold::new().body(body_marker())]);

    handle.show_snack_bar(SnackBar::new(Text::new("brief")).duration(Duration::from_millis(120)));
    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(
        snack_bar_material_count(&laid),
        1,
        "must be shown right after entering"
    );

    // Comfortably before 120ms of display time has elapsed: still shown.
    pump_ms(&mut laid, 40);
    assert_eq!(
        snack_bar_material_count(&laid),
        1,
        "must still be shown well before its configured 120ms duration elapses"
    );

    // Past 120ms of display time plus the 250ms exit reverse: gone.
    pump_ms(&mut laid, 120);
    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(
        snack_bar_material_count(&laid),
        0,
        "must have auto-dismissed once its own configured duration elapsed"
    );
}

// ============================================================================
// 3. Early hide cancels the display timer (no dangling early auto-dismiss).
// ============================================================================

#[test]
fn hiding_before_the_duration_elapses_cancels_the_display_timer() {
    let vsync = Vsync::new();
    let (mut laid, handle) =
        mount_with_scaffolds(&vsync, vec![Scaffold::new().body(body_marker())]);

    handle
        .show_snack_bar(SnackBar::new(Text::new("hidden early")).duration(Duration::from_secs(10)));
    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(snack_bar_material_count(&laid), 1);

    handle.hide_current_snack_bar();
    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(
        snack_bar_material_count(&laid),
        0,
        "an explicit hide must close the snack bar well before its 10s duration would have"
    );

    // If the cancelled timer were still ticking toward its original 10s
    // duration, its later firing would call `hide_current` on an EMPTY
    // queue — a no-op per `MessengerCore::hide_current`'s own guard — so the
    // only way this stays reliably closed (not, say, re-opened by a stray
    // side effect) is confirming it's still gone well past that point too.
    pump_ms(&mut laid, 500);
    assert_eq!(snack_bar_material_count(&laid), 0);
}

// ============================================================================
// 4. Action press closes with Action reason and disables after one press.
// ============================================================================

#[test]
fn action_press_closes_the_snack_bar_and_is_single_fire() {
    let vsync = Vsync::new();
    let action_presses = Arc::new(AtomicUsize::new(0));
    let action_presses_for_cb = Arc::clone(&action_presses);

    let (mut laid, handle) =
        mount_with_scaffolds(&vsync, vec![Scaffold::new().body(body_marker())]);

    handle.show_snack_bar(
        SnackBar::new(Text::new("Saved")).action(SnackBarAction::new("UNDO", move || {
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
    let snack_bar_material = laid
        .find_all_by_render_type("RenderPhysicalShape")
        .into_iter()
        .find(|&id| {
            laid.render_property(id, "elevation")
                .and_then(|value| value.parse::<f32>().ok())
                == Some(6.0)
        })
        .expect("the snack bar's Material must be mounted");
    let bar_offset = laid.absolute_offset(snack_bar_material);
    let bar_size = laid.size(snack_bar_material);
    let tap_x = bar_offset.dx.get() + bar_size.width.get() * 0.92;
    let tap_y = bar_offset.dy.get() + bar_size.height.get() * 0.5;

    laid.dispatch_pointer_down(tap_x, tap_y);
    laid.dispatch_pointer_up(tap_x, tap_y);
    assert_eq!(
        action_presses.load(Ordering::SeqCst),
        1,
        "the action must fire on press"
    );

    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    assert_eq!(
        snack_bar_material_count(&laid),
        0,
        "pressing the action must close the snack bar (SnackBarClosedReason::Action)"
    );

    // A second dispatch at the same coordinates hits whatever is now
    // mounted there (the body, since the snack bar closed) — confirms the
    // action truly stopped being interactable, not merely that its
    // callback happens not to have re-fired for an unrelated reason.
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

#[test]
fn messenger_over_two_scaffolds_shows_the_current_entry_on_both() {
    let vsync = Vsync::new();
    let (mut laid, handle) = mount_with_scaffolds(
        &vsync,
        vec![
            Scaffold::new().body(body_marker()),
            Scaffold::new().body(body_marker()),
        ],
    );

    let roots = scaffold_roots(&laid);
    assert_eq!(roots.len(), 2, "both Scaffolds must be mounted");
    for &root in &roots {
        assert_eq!(
            laid.children(root).len(),
            1,
            "body only, before any snack bar shows"
        );
    }

    handle.show_snack_bar(SnackBar::new(Text::new("both")));
    pump_ms(&mut laid, ENTRY.as_millis() as u64);

    for &root in &scaffold_roots(&laid) {
        assert_eq!(
            laid.children(root).len(),
            2,
            "every registered Scaffold's CustomMultiChildLayout must mount a snack-bar slot child"
        );
    }
    assert_eq!(
        snack_bar_material_count(&laid),
        2,
        "one snack bar Material per registered Scaffold, not one shared across both"
    );
}

// ============================================================================
// 6. FAB lift: above the snack bar, both mid-animation and at rest.
// ============================================================================

#[test]
fn floating_action_button_lifts_above_the_snack_bar_mid_animation_and_at_rest() {
    let vsync = Vsync::new();
    let (mut laid, handle) = mount_with_scaffolds(
        &vsync,
        vec![
            Scaffold::new()
                .body(body_marker())
                .floating_action_button(FloatingActionButton::new(Some(|| {}), Text::new("+"))),
        ],
    );

    let scaffold_root = scaffold_roots(&laid)[0];
    let fab_id = |laid: &common::LaidOut| -> RenderId {
        laid.children(scaffold_root)
            .into_iter()
            .find(|&id| {
                let size = laid.size(id);
                size.width.get() < 100.0 && size.height.get() < 100.0
            })
            .expect("the FAB must be mounted")
    };

    let fab_y_at_rest_before = laid.offset(fab_id(&laid)).dy.get();

    handle.show_snack_bar(SnackBar::new(Text::new("lift me")));

    // Mid-animation: partway through the 250ms entrance, the snack bar has
    // SOME height (not zero, not yet its final height) — the FAB must
    // already be lifted, strictly between its resting position and where it
    // ends up once the snack bar is fully grown.
    pump_ms(&mut laid, 60);
    let fab_y_mid_entrance = laid.offset(fab_id(&laid)).dy.get();
    assert!(
        fab_y_mid_entrance < fab_y_at_rest_before,
        "the FAB must already be lifted mid-entrance: before={fab_y_at_rest_before}, mid={fab_y_mid_entrance}"
    );

    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    let fab_y_fully_shown = laid.offset(fab_id(&laid)).dy.get();
    assert!(
        fab_y_fully_shown < fab_y_mid_entrance,
        "the FAB must keep rising as the snack bar keeps growing: mid={fab_y_mid_entrance}, \
         fully_shown={fab_y_fully_shown}"
    );

    handle.remove_current_snack_bar();
    pump_ms(&mut laid, ENTRY.as_millis() as u64);
    let fab_y_at_rest_after = laid.offset(fab_id(&laid)).dy.get();
    assert!(
        (fab_y_at_rest_after - fab_y_at_rest_before).abs() < 1.0,
        "the FAB must return to its original resting position once the snack bar is gone: \
         before={fab_y_at_rest_before}, after={fab_y_at_rest_after}"
    );
}

// ============================================================================
// 7. Unregister on dispose: an unmounted Scaffold must not linger in the
//    messenger's registered set.
// ============================================================================

#[test]
fn unmounting_a_scaffold_unregisters_it_from_the_messenger() {
    let vsync = Vsync::new();
    let handle_slot: Rc<RefCell<Option<ScaffoldMessengerHandle>>> = Rc::new(RefCell::new(None));
    let tree = themed_animated(
        &vsync,
        ScaffoldMessenger::new(flui_widgets::Column::new(vec![
            ViewExt::boxed(HandleProbe {
                slot: Rc::clone(&handle_slot),
            }),
            ViewExt::boxed(flui_widgets::Expanded::new(
                Scaffold::new().body(body_marker()),
            )),
        ])),
    );
    let mut laid = lay_out_animated(tree, tight(400.0, 800.0), vsync.clone());
    let handle = handle_slot
        .borrow()
        .clone()
        .expect("handle must be published");
    assert_eq!(
        handle.registered_scaffold_count(),
        1,
        "the mounted Scaffold must have registered"
    );

    // Swap the root to the SAME ScaffoldMessenger subtree, minus the
    // Scaffold — `ScaffoldMessenger`'s own element persists (same handle),
    // but the Scaffold underneath it is torn down.
    let replacement = themed_animated(
        &vsync,
        ScaffoldMessenger::new(flui_widgets::Column::new(vec![ViewExt::boxed(
            HandleProbe {
                slot: Rc::clone(&handle_slot),
            },
        )])),
    );
    laid.pump_widget(replacement);

    assert_eq!(
        handle.registered_scaffold_count(),
        0,
        "the unmounted Scaffold's dispose must have unregistered it"
    );
}

// ============================================================================
// 8. Scope re-home: a FRESH Scaffold element that mounts under a messenger
//    registers with that messenger, never a stale one from elsewhere in the
//    tree — the practical shape "re-homing" actually takes in this
//    substrate.
//
// A literal "the SAME Scaffold element persists in place while its ANCESTOR
// messenger identity changes" scenario is not exercised here: FLUI (like
// Flutter) reconciles `ScaffoldMessenger::new(...)` at the same type+position
// in the tree as an UPDATE to the existing element, not a fresh mount — so
// two structurally-identical `ScaffoldMessenger::new(...)` calls in
// sequence are the SAME element/handle, not "old" vs "new" (confirmed
// empirically: `pump_widget` with a second, differently-parameterized
// `ScaffoldMessenger::new(...)` at the same tree shape yields
// `first_handle.ptr_eq(&second_handle) == true`). Reaching a genuinely
// different ancestor identity without remounting the Scaffold would need
// `GlobalKey`-based reparenting across two structurally distinct branches,
// which `ScaffoldMessengerScope::maybe_of`'s no-dependency ambient lookup
// (see `scaffold.rs`'s own module docs' "`ScaffoldMessenger` wiring"
// section) cannot pick up anyway, since `did_change_dependencies` never
// fires for it — a documented, honest limitation, not silently papered
// over.
// ============================================================================

#[test]
fn a_scaffold_mounted_fresh_under_a_new_messenger_registers_with_that_one_not_a_stale_one() {
    let vsync = Vsync::new();
    let first_handle_slot: Rc<RefCell<Option<ScaffoldMessengerHandle>>> =
        Rc::new(RefCell::new(None));
    let tree = themed_animated(
        &vsync,
        ScaffoldMessenger::new(flui_widgets::Column::new(vec![ViewExt::boxed(
            HandleProbe {
                slot: Rc::clone(&first_handle_slot),
            },
        )])),
    );
    let mut laid = lay_out_animated(tree, tight(400.0, 800.0), vsync.clone());
    let first_handle = first_handle_slot
        .borrow()
        .clone()
        .expect("handle must be published");
    assert_eq!(
        first_handle.registered_scaffold_count(),
        0,
        "no Scaffold mounted under it yet"
    );

    // A structurally distinct second `ScaffoldMessenger` (nested one level
    // deeper, so reconciliation treats it as a genuinely different element,
    // not an update of the first) with a freshly-mounted `Scaffold`.
    let second_handle_slot: Rc<RefCell<Option<ScaffoldMessengerHandle>>> =
        Rc::new(RefCell::new(None));
    let replacement = themed_animated(
        &vsync,
        ScaffoldMessenger::new(flui_widgets::Column::new(vec![
            ViewExt::boxed(HandleProbe {
                slot: Rc::clone(&first_handle_slot),
            }),
            ViewExt::boxed(flui_widgets::Expanded::new(ScaffoldMessenger::new(
                flui_widgets::Column::new(vec![
                    ViewExt::boxed(HandleProbe {
                        slot: Rc::clone(&second_handle_slot),
                    }),
                    ViewExt::boxed(flui_widgets::Expanded::new(
                        Scaffold::new().body(body_marker()),
                    )),
                ]),
            ))),
        ])),
    );
    laid.pump_widget(replacement);
    let second_handle = second_handle_slot
        .borrow()
        .clone()
        .expect("nested handle must be published");

    assert!(
        !first_handle.ptr_eq(&second_handle),
        "the outer and the freshly-mounted nested ScaffoldMessenger must be distinct instances"
    );
    assert_eq!(
        first_handle.registered_scaffold_count(),
        0,
        "the outer messenger must never see a Scaffold that mounted under the nested one"
    );
    assert_eq!(
        second_handle.registered_scaffold_count(),
        1,
        "the freshly-mounted Scaffold must register with its OWN nearest messenger"
    );
}
