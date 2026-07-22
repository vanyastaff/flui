//! `BoxedView` conditional return compiles and the
//! author-side overhead is bounded by `.boxed()` per branch.
//!
//! Edge Case (spec): a `build()` body whose arms have different
//! concrete types (`if x { Text(...) } else { Padding {...} }`)
//! cannot return `impl IntoView` directly — each arm produces a
//! distinct opaque type, and rustc rejects `if`/`else` arms of
//! incompatible types. The documented authoring escape hatch is
//! `.boxed()` per branch, landing on `BoxedView` (which itself
//! implements `View`, hence `IntoView` via the blanket
//! `impl<V: View> IntoView for V`).
//!
//! The author-side overhead is bounded at **≤ 2 tokens per
//! branch** (`.boxed()` itself is one identifier + one `()` call,
//! so 4 token-positions per branch in `proc_macro2` accounting;
//! the "2 tokens" framing collapses the `(` and `)` into the
//! identifier as one author-visible action).
//!
//! This file pins:
//!
//! 1. The pattern compiles for two distinct concrete View types
//!    in two `if` arms.
//! 2. The pattern compiles for the >2-arm `match` shape that
//!    `view_match!` (a deferred ergonomics helper, plan "Open
//!    Questions") would otherwise sugar.
//! 3. The pattern compiles when an arm returns the bare concrete
//!    type and the other arm returns `.boxed()` — `.boxed()` on
//!    a `BoxedView` is idempotent (still a `View`), so a mixed-
//!    arm shape still type-checks AS LONG AS both arms land on
//!    the same opaque type. The bare-arm case requires `.boxed()`
//!    too — leaving one arm un-boxed produces `if T1 else T2`
//!    which fails E0308. We test the canonical "every arm
//!    `.boxed()`" shape only.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::used_underscore_items)]

use flui_view::context::BuildContext;
use flui_view::prelude::*;

#[derive(Clone)]
struct LeafA;
impl StatelessView for LeafA {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        LeafA
    }
}
impl View for LeafA {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct LeafB;
impl StatelessView for LeafB {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        LeafB
    }
}
impl View for LeafB {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct LeafC;
impl StatelessView for LeafC {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        LeafC
    }
}
impl View for LeafC {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct ConditionalRoot {
    branch: bool,
}

impl StatelessView for ConditionalRoot {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Canonical authoring shape: each arm explicitly
        // `.boxed()` so both land on `BoxedView`. The author-side
        // overhead vs the trivial `impl IntoView` return is
        // `.boxed()` per branch — within the token budget above.
        if self.branch {
            LeafA.boxed()
        } else {
            LeafB.boxed()
        }
    }
}

impl View for ConditionalRoot {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[derive(Clone)]
struct ThreeWayRoot {
    arm: u8,
}

impl StatelessView for ThreeWayRoot {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // >2-arm pattern still compiles with `.boxed()` per arm.
        match self.arm {
            0 => LeafA.boxed(),
            1 => LeafB.boxed(),
            _ => LeafC.boxed(),
        }
    }
}

impl View for ThreeWayRoot {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

#[test]
fn two_arm_conditional_compiles_and_is_a_view() {
    let v = ConditionalRoot { branch: true };
    let element = View::create_element(&v);
    assert_eq!(element.lifecycle(), Lifecycle::Initial);
}

#[test]
fn three_arm_match_compiles_and_is_a_view() {
    let v = ThreeWayRoot { arm: 1 };
    let element = View::create_element(&v);
    assert_eq!(element.lifecycle(), Lifecycle::Initial);
}

#[test]
fn boxed_view_is_a_view() {
    // `BoxedView` itself satisfies the `View` trait — the
    // `IntoView` blanket then makes it a valid `impl IntoView`
    // return. This is what makes the per-branch `.boxed()` shape
    // work.
    fn _takes_view<V: View>(_: V) {}
    let bv: BoxedView = LeafA.boxed();
    _takes_view(bv);
}
