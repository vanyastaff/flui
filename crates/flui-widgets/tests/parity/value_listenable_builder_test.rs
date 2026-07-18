//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/value_listenable_builder_test.dart`
//! (tag `3.44.0`) — 6 cases, all 6 ported here.
//!
//! Widget → render-object mapping: `ValueListenableBuilder<T>` → whatever
//! `builder` returns — a `SizedBox` sized by the observed `Option<String>`'s
//! length (`None` ⇒ side `1.0`), so every case asserts through plain geometry
//! (`LaidOut::size`), matching the `stateful_test.rs`/`stream_builder.rs`
//! precedent: no text shaping needed to prove a value flowed through.
//!
//! ## Rust-shape adaptation: a `Send + Sync`-shareable observed value
//!
//! Flutter's oracle mutates `valueListenable.value` directly from the test —
//! sound in Dart because every object is a GC'd reference, so the widget and
//! the test alias the very same notifier. `flui_foundation::ValueNotifier<T>`
//! is documented single-owner (`&mut self` mutation, `Clone` deep-copies
//! `value` — see `crates/flui-widgets/src/widget_state.rs`'s module doc for
//! the same observation about why `WidgetStatesController` doesn't reuse it),
//! so a bare `ValueNotifier<String>` cannot be mutated by test code while a
//! mounted widget concurrently holds the `Arc` the framework subscribed to.
//!
//! This port's `T` is instead [`SharedCell`] = `Arc<Mutex<Option<String>>>` —
//! an interior-mutable cell. `ValueNotifier<T>::value(&self)` hands back a
//! stable `&T`: the outer `Arc<Mutex<_>>` itself never changes identity after
//! construction, which is exactly what `ValueListenable::value(&self) -> &T`
//! needs (a reference that outlives `&self` — something a `Mutex` guarding
//! `T` itself could not give back). The test mutates the cell's *contents*
//! through the `Mutex` (`&self`-compatible) and calls
//! [`ValueNotifier::notify`] (also `&self`) to fire listeners — Flutter's
//! "mutate then notify" shape, reached without changing `flui_foundation`'s
//! existing (already-shipped, single-owner) `ValueNotifier` API.
//!
//! ## Ported cases
//! - `'Null value is ok'` → [`null_value_builds_the_none_sized_box`].
//! - `'Widget builds with initial value'` → [`widget_builds_with_initial_value`].
//! - `'Widget updates when value changes'` → [`widget_updates_when_value_changes`].
//! - `'Can change listenable'` → [`can_change_listenable`].
//! - `'Stops listening to old listenable after changing listenable'` →
//!   [`stops_listening_to_old_listenable_after_changing_listenable`].
//! - `'Self-cleans when removed'` → [`self_cleans_when_removed`].
//!
//! The last case swaps the widget under test out from under a stable
//! `Toggle` wrapper rather than replacing the harness's tree root directly:
//! `LaidOut::pump_widget` (`tests/common/mod.rs`) only supports a same-type
//! root swap — its only two precedent users, `FutureBuilder`/`StreamBuilder`,
//! always swap the same concrete type across a `pump_widget` call — so a
//! root-level type change (`ValueListenableBuilder` → a plain `SizedBox`)
//! would silently no-op instead of unmounting. Swapping the child of a
//! stable wrapper root exercises the ordinary reconciliation path
//! (`ElementCore::update_view`'s documented "on a type mismatch the caller
//! replaces the element") a real root swap would use, without new harness
//! plumbing.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::{ValueListenable, ValueNotifier};
use flui_widgets::prelude::*;
use flui_widgets::{ValueListenableBuilder, ValueWidgetBuilder};
use parking_lot::Mutex;

use crate::common::{lay_out, loose, size};

/// The shared, `&self`-mutable cell backing each test's observed value — see
/// the module doc's "Rust-shape adaptation" section.
type SharedCell = Arc<Mutex<Option<String>>>;

/// A fresh `ValueNotifier` over a shared cell, plus the cell itself so the
/// test can mutate its contents directly.
fn shared_listenable(initial: Option<&str>) -> (Arc<ValueNotifier<SharedCell>>, SharedCell) {
    let cell: SharedCell = Arc::new(Mutex::new(initial.map(str::to_owned)));
    let notifier = Arc::new(ValueNotifier::new(Arc::clone(&cell)));
    (notifier, cell)
}

/// `SizedBox` side length that encodes the observed `Option<String>` —
/// `None` -> `1.0`, `Some(s)` -> `s.len()` as pixels. Lets every case assert
/// through plain geometry (`LaidOut::size`), with no text shaping involved.
fn side_for(value: Option<&String>) -> f32 {
    value.map_or(1.0, |s| s.len() as f32)
}

/// A builder that reads the cell and returns a `SizedBox` sized by
/// [`side_for`] — shared by every case below. Mirrors the oracle's
/// `builderForValueListenable`: `Null` maps to a distinct render shape
/// (Flutter's `Placeholder`; here, the `None` side of [`side_for`]).
fn sized_box_builder() -> ValueWidgetBuilder<SharedCell> {
    Rc::new(|_ctx, cell: &SharedCell, _child| {
        let side = side_for(cell.lock().as_ref());
        SizedBox::square(side).boxed()
    })
}

/// Flutter parity: `'Null value is ok'` — a `null` initial value must not
/// panic the build. The oracle asserts a `Placeholder` renders; FLUI has no
/// `Placeholder`, so the port asserts through geometry instead — the
/// `None`-branch side (`1.0`) from [`side_for`].
#[test]
fn null_value_builds_the_none_sized_box() {
    let (notifier, _cell) = shared_listenable(None);
    let listenable: Arc<dyn ValueListenable<SharedCell>> = notifier;
    let view = ValueListenableBuilder::new(listenable, sized_box_builder());

    let laid = lay_out(view, loose(100.0));
    assert_eq!(
        laid.size(laid.root()),
        size(1.0, 1.0),
        "a None value must build the None-branch SizedBox, not panic"
    );
}

/// Flutter parity: `'Widget builds with initial value'` — the first build
/// reads the listenable's value at mount, before any notification.
#[test]
fn widget_builds_with_initial_value() {
    let (notifier, _cell) = shared_listenable(Some("Bachman"));
    let listenable: Arc<dyn ValueListenable<SharedCell>> = notifier;
    let view = ValueListenableBuilder::new(listenable, sized_box_builder());

    let laid = lay_out(view, loose(100.0));
    assert_eq!(
        laid.size(laid.root()),
        size(7.0, 7.0),
        "\"Bachman\" is 7 chars — the initial value must reach the first build"
    );
}

/// Flutter parity: `'Widget updates when value changes'` — two successive
/// notifications on the SAME listenable each rebuild to the latest value;
/// the previous value's rendered size must be gone, not merely superseded.
#[test]
fn widget_updates_when_value_changes() {
    let (notifier, cell) = shared_listenable(None);
    let listenable: Arc<dyn ValueListenable<SharedCell>> = notifier.clone();
    let view = ValueListenableBuilder::new(listenable, sized_box_builder());

    let mut laid = lay_out(view, loose(100.0));
    assert_eq!(
        laid.size(laid.root()),
        size(1.0, 1.0),
        "initial: None -> side 1.0"
    );

    *cell.lock() = Some("Gilfoyle".to_owned());
    notifier.notify();
    laid.tick();
    assert_eq!(
        laid.size(laid.root()),
        size(8.0, 8.0),
        "\"Gilfoyle\" is 8 chars"
    );

    *cell.lock() = Some("Dinesh".to_owned());
    notifier.notify();
    laid.tick();
    assert_eq!(
        laid.size(laid.root()),
        size(6.0, 6.0),
        "\"Dinesh\" is 6 chars; the stale \"Gilfoyle\" (8×8) size must be gone"
    );
}

/// Flutter parity: `'Can change listenable'` — a root rebuild that swaps in a
/// DIFFERENT listenable instance must show the new instance's current value
/// immediately.
#[test]
fn can_change_listenable() {
    let (first, first_cell) = shared_listenable(None);
    let first_listenable: Arc<dyn ValueListenable<SharedCell>> = first.clone();
    let view = ValueListenableBuilder::new(first_listenable, sized_box_builder());
    let mut laid = lay_out(view, loose(100.0));

    *first_cell.lock() = Some("Gilfoyle".to_owned());
    first.notify();
    laid.tick();
    assert_eq!(
        laid.size(laid.root()),
        size(8.0, 8.0),
        "\"Gilfoyle\" is 8 chars"
    );

    let (second, _second_cell) = shared_listenable(Some("Hendricks"));
    let second_listenable: Arc<dyn ValueListenable<SharedCell>> = second;
    laid.pump_widget(ValueListenableBuilder::new(
        second_listenable,
        sized_box_builder(),
    ));

    assert_eq!(
        laid.size(laid.root()),
        size(9.0, 9.0),
        "\"Hendricks\" is 9 chars — the new listenable's value must show immediately, \
         not the old \"Gilfoyle\" (8×8)"
    );
}

/// Flutter parity: `'Stops listening to old listenable after changing
/// listenable'`. Beyond the oracle's own assertion (the old listenable's
/// later mutation must not reach the widget), this also asserts the
/// underlying registration count directly — the AnimatedSwitcher-lesson
/// rigor the oracle's `find.text` checks alone don't give: a leaked listener
/// doesn't panic, so the observable must be an explicit count, not
/// must-not-panic.
#[test]
fn stops_listening_to_old_listenable_after_changing_listenable() {
    let (first, first_cell) = shared_listenable(None);
    let first_listenable: Arc<dyn ValueListenable<SharedCell>> = first.clone();
    let view = ValueListenableBuilder::new(first_listenable, sized_box_builder());
    let mut laid = lay_out(view, loose(100.0));

    *first_cell.lock() = Some("Gilfoyle".to_owned());
    first.notify();
    laid.tick();
    assert_eq!(laid.size(laid.root()), size(8.0, 8.0));

    let (second, _second_cell) = shared_listenable(Some("Hendricks"));
    let second_listenable: Arc<dyn ValueListenable<SharedCell>> = second;
    laid.pump_widget(ValueListenableBuilder::new(
        second_listenable,
        sized_box_builder(),
    ));
    assert_eq!(laid.size(laid.root()), size(9.0, 9.0));

    assert!(
        !first.has_listeners(),
        "did_update_view must remove the listener from the OLD listenable when the \
         instance changes — a registration-count assert, not a must-not-panic one"
    );

    // The old (now disconnected) listenable fires, but must not reach the
    // widget: no schedule, no rebuild, no size change.
    *first_cell.lock() = Some("Big Head".to_owned());
    first.notify();
    laid.tick();

    assert_eq!(
        laid.size(laid.root()),
        size(9.0, 9.0),
        "a notification from the disconnected listenable must not rebuild the \
         widget — it must still show \"Hendricks\" (9×9), not \"Big Head\" (8×8)"
    );
}

/// A single-child switch: shows `child` while `show` is `true`, a distinct
/// leaf otherwise — see the module doc's "Ported cases" note on why the
/// widget under test is swapped one level below the harness root rather than
/// AT the root.
#[derive(Clone, StatefulView)]
struct Toggle {
    show: Arc<AtomicBool>,
    child: ValueListenableBuilder<SharedCell>,
}

struct ToggleState {
    show: Arc<AtomicBool>,
    child: ValueListenableBuilder<SharedCell>,
}

impl StatefulView for Toggle {
    type State = ToggleState;

    fn create_state(&self) -> Self::State {
        ToggleState {
            show: Arc::clone(&self.show),
            child: self.child.clone(),
        }
    }
}

impl ViewState<Toggle> for ToggleState {
    fn build(&self, _view: &Toggle, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.show.load(Ordering::Relaxed) {
            self.child.clone().boxed()
        } else {
            SizedBox::shrink().boxed()
        }
    }
}

/// Flutter parity: `'Self-cleans when removed'` — removing the widget from
/// the tree unsubscribes it. Asserted directly via `has_listeners()` (a
/// registration-count check), matching the oracle's own
/// `SpyStringValueNotifier.hasListeners == false` assertion — not a
/// must-not-panic proxy.
#[test]
fn self_cleans_when_removed() {
    let (notifier, cell) = shared_listenable(None);
    let listenable: Arc<dyn ValueListenable<SharedCell>> = notifier.clone();
    let child = ValueListenableBuilder::new(listenable, sized_box_builder());
    let show = Arc::new(AtomicBool::new(true));

    let mut laid = lay_out(
        Toggle {
            show: Arc::clone(&show),
            child,
        },
        loose(100.0),
    );

    *cell.lock() = Some("Gilfoyle".to_owned());
    notifier.notify();
    laid.tick();
    assert_eq!(laid.size(laid.root()), size(8.0, 8.0));
    assert!(
        notifier.has_listeners(),
        "sanity: still subscribed before removal"
    );

    show.store(false, Ordering::Relaxed);
    laid.pump();

    assert!(
        !notifier.has_listeners(),
        "dispose() must remove the listener once the widget leaves the tree"
    );
}
