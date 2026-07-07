//! Phase 3 §U23: smoke test for `#[derive(StatelessView)]` and
//! `#[derive(StatefulView)]`.
//!
//! Locks the canonical authoring shape — author writes the struct,
//! the `impl StatelessView for X { fn build() -> impl IntoView }`
//! block (or the `StatefulView`/`ViewState` pair), and the derive
//! provides the `impl View` block. No `Box::new`, no `impl View for X`
//! hand-written boilerplate, no `impl_stateless_view!` invocation,
//! no `.into_view()` at the build call site.
//!
//! Covers:
//! - FR-009 (`#[derive(StatelessView)]` + `#[derive(StatefulView)]`)
//! - the dependency edge `flui-view → flui-macros` is exercised
//!   through real authoring code (not just a placeholder smoke linkage)
//! - generic widget compilation
//! - the typed `View::create_element` returns a valid `ElementBase`
//!   for both stateless and stateful authoring shapes

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::no_effect_underscore_binding, clippy::used_underscore_items)]

use std::sync::atomic::{AtomicUsize, Ordering};

use flui_view::context::BuildContext;
use flui_view::prelude::*;

// ----------------------------------------------------------------------------
// Stateless authoring shape
// ----------------------------------------------------------------------------

#[derive(Clone, StatelessView)]
struct Greeting {
    #[allow(dead_code, reason = "exercised by the derive's create_element + Clone")]
    name: String,
}

impl StatelessView for Greeting {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Authoring shape: return a concrete View (or BoxedView) — NOT
        // `Box::new(self.clone())`. We return the wrapper view so the
        // test has a self-contained inner View; in real authoring this
        // would be `Text::new(...)` or similar.
        WrappedLeaf
    }
}

#[derive(Clone, StatelessView)]
struct WrappedLeaf;

impl StatelessView for WrappedLeaf {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Tail of the recursion — return a stable BoxedView that
        // implements both `IntoView` (blanket) and `View` (explicit).
        WrappedLeaf
    }
}

// ----------------------------------------------------------------------------
// Stateful authoring shape
// ----------------------------------------------------------------------------

#[derive(Clone, StatefulView)]
struct Counter {
    initial: u32,
}

struct CounterState {
    count: u32,
    build_count: AtomicUsize,
}

impl flui_view::view::StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> CounterState {
        CounterState {
            count: self.initial,
            build_count: AtomicUsize::new(0),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, _view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
        self.build_count.fetch_add(1, Ordering::SeqCst);
        // Touch `self.count` so the field participates in the build —
        // mirrors the real authoring shape where the state value
        // typically flows into the returned view.
        let _ = self.count;
        WrappedLeaf
    }
}

// ----------------------------------------------------------------------------
// Generic widget — verifies the derive forwards type parameters
// ----------------------------------------------------------------------------

#[derive(Clone, StatelessView)]
struct PaddedHolder<C: View + Clone> {
    inset: f32,
    child: C,
}

impl<C: View + Clone> StatelessView for PaddedHolder<C> {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // We do not use `self.child` to build a render tree here — that
        // would pull in geometry types this test deliberately doesn't
        // touch. The point of the generic case is that the derive
        // forwards `<C: View + Clone>` to the `impl View for
        // PaddedHolder<C>` block; `C` must satisfy `Self::Clone` (the
        // outer derive's `Clone` requirement) plus `View`. We hand the
        // child clone back through the recursion edge.
        let _inset = self.inset;
        self.child.clone()
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[test]
fn stateless_derive_emits_a_view_impl() {
    let view = Greeting {
        name: "World".to_string(),
    };
    // Reach for the trait method the derive synthesizes. If the derive
    // failed to emit `impl View`, the test would not compile.
    let element = View::create_element(&view);
    assert_eq!(element.lifecycle(), Lifecycle::Initial);
    // `view_type_id` falls back to the trait's default, which returns
    // `TypeId::of::<Self>()` — confirms the derived impl carries the
    // correct `Self`.
    assert_eq!(element.view_type_id(), std::any::TypeId::of::<Greeting>());
}

#[test]
fn stateful_derive_emits_a_view_impl() {
    let view = Counter { initial: 7 };
    let element = View::create_element(&view);
    assert_eq!(element.lifecycle(), Lifecycle::Initial);
    assert_eq!(element.view_type_id(), std::any::TypeId::of::<Counter>());
}

#[test]
fn generic_widget_compiles_through_derive() {
    let view = PaddedHolder {
        inset: 8.0,
        child: WrappedLeaf,
    };
    let element = View::create_element(&view);
    assert_eq!(element.lifecycle(), Lifecycle::Initial);
    assert_eq!(
        element.view_type_id(),
        std::any::TypeId::of::<PaddedHolder<WrappedLeaf>>()
    );
}

#[test]
fn stateless_derive_keyless_by_default() {
    // The derive emits no `fn key()`, so the View trait's default
    // returns `None`. Authors who need a keyed widget drop the derive
    // and write the `impl View` block manually (documented on the
    // `#[derive(StatelessView)]` macro).
    let view = Greeting {
        name: "Keyless".to_string(),
    };
    assert!(view.key().is_none());
}

#[test]
fn into_view_blanket_covers_derived_view() {
    // Sanity check: a `#[derive(StatelessView)]` type satisfies
    // `IntoView` through the `impl<V: View> IntoView for V` blanket,
    // not through a separate derive. This means returning a derived
    // type directly from another `build()` works without `.boxed()`.
    fn _takes_into_view<T: IntoView>(_: T) {}

    let view = Greeting {
        name: "Blanket".to_string(),
    };
    _takes_into_view(view);
}
