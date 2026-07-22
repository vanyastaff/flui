//! This test (US4 AS2) verifies that the `#[derive(StatelessView, ::bon::Builder)]`
//! stack compiles AND the resulting widget is usable through both
//! authoring surfaces.
//!
//! The catalog's most-common future authoring pattern for >3-field
//! widgets — a `bon` builder + the view derive on the same struct.
//! This test locks the contract that:
//!
//! 1. `#[derive(Clone, StatelessView, ::bon::Builder)]` parses and
//!    expands without conflict — neither macro intercepts the other's
//!    `[derive(...)]` slot.
//! 2. The `bon::Builder` call site `Card::builder().a(...).b(...).build()`
//!    produces a `Card` value with the same identity as the equivalent
//!    struct literal — the builder is purely a fluent constructor.
//! 3. The resulting `Card` is usable as a `View` through
//!    `View::create_element` — the derive's `impl View` block applies
//!    to the bon-generated `Card` shape, not just to hand-rolled
//!    structs.
//!
//! Spec: FR-009 (derive), FR-011 (`bon` builder convention), US4 AS2.

use bon::Builder;
use flui_view::context::BuildContext;
use flui_view::prelude::*;

#[derive(Clone, StatelessView, Builder)]
struct Card {
    /// Foreground content label — the only field the test asserts on,
    /// to verify the builder writes through to struct state.
    title: String,
    /// Body copy — present for arity > 3 (the threshold where
    /// FR-011's `bon` convention applies). Carries no test assertion.
    #[allow(
        dead_code,
        reason = "field arity drives FR-011 convention; not asserted"
    )]
    body: String,
    /// Material elevation level — same as `body`.
    #[allow(
        dead_code,
        reason = "field arity drives FR-011 convention; not asserted"
    )]
    elevation: u32,
    /// Optional callback — locks that `bon::Builder` supports
    /// `Option<_>`-fielded shapes alongside the view derive. Stored as
    /// `Option<()>` here to keep the test self-contained (no callback
    /// machinery in scope).
    #[allow(
        dead_code,
        reason = "field arity drives FR-011 convention; not asserted"
    )]
    on_tap: Option<()>,
}

impl StatelessView for Card {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Tail: return a stable BoxedView so the test does not need a
        // separate leaf type. The point of this test is the derive
        // stack, not the build body.
        BoxedView(Box::new(self.clone()))
    }
}

#[test]
fn bon_builder_constructs_a_view_struct() {
    let card = Card::builder()
        .title("Hello".to_string())
        .body("World".to_string())
        .elevation(2)
        .on_tap(())
        .build();
    assert_eq!(card.title, "Hello");
}

#[test]
fn bon_builder_struct_is_a_view_through_derive() {
    let card = Card::builder()
        .title("BuiltViaBon".to_string())
        .body(String::new())
        .elevation(0)
        .on_tap(())
        .build();

    // The `#[derive(StatelessView)]` emitted `impl View for Card { fn
    // create_element() }` — invoking it proves the derive's generated
    // code applies to the bon-built shape with no attribute-stacking
    // conflict.
    let element = View::create_element(&card);
    assert_eq!(element.element().lifecycle(), Lifecycle::Initial);
    assert_eq!(
        element.element().view_type_id(),
        std::any::TypeId::of::<Card>()
    );
}

#[test]
fn struct_literal_and_builder_yield_equivalent_views() {
    let literal = Card {
        title: "Same".to_string(),
        body: String::new(),
        elevation: 0,
        on_tap: None,
    };
    let built = Card::builder()
        .title("Same".to_string())
        .body(String::new())
        .elevation(0)
        // bon's optional-field accessor: passing `None` is equivalent
        // to omitting the call entirely; we use the explicit form so
        // the test reads symmetrically with the literal above.
        .maybe_on_tap(None)
        .build();
    assert_eq!(literal.title, built.title);
    assert_eq!(literal.elevation, built.elevation);
}
