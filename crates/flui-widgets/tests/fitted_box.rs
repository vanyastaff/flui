//! Tests for `FittedBox`'s own sizing contract.
//!
//! `RenderFittedBox::perform_layout` (`crates/flui-objects/.../fitted_box.rs`)
//! computes its OWN size via `constrain_size_and_attempt_to_preserve_aspect_ratio`
//! for every `BoxFit` variant except `ScaleDown` -- `fit` only changes how the
//! child is later scaled/positioned *within* that size (a paint-time
//! transform this headless harness cannot introspect for a `RenderFittedBox`
//! node, unlike the dedicated `RenderTransform` accessor). These tests verify
//! the two size invariants that hold regardless of `fit`, plus that the
//! widget genuinely mounts `RenderFittedBox` -- the fit-dependent scale/
//! alignment math itself is already covered by `RenderFittedBox`'s own
//! harness tests in `flui-objects`.

mod common;

use common::{lay_out, loose, size, tight};
use flui_types::layout::BoxFit;
use flui_widgets::{FittedBox, SizedBox};

#[test]
fn fitted_box_without_a_child_sizes_to_the_smallest_constraint() {
    let laid = lay_out(FittedBox::new(), loose(500.0));
    assert_eq!(
        laid.size(laid.root()),
        size(0.0, 0.0),
        "no child -> smallest valid size under a loose-from-zero constraint",
    );
}

#[test]
fn fitted_box_mounts_a_render_fitted_box() {
    let laid = lay_out(
        FittedBox::new().child(SizedBox::new(50.0, 25.0)),
        loose(200.0),
    );
    let _ = laid.find_by_render_type("RenderFittedBox");
}

#[test]
fn fitted_box_with_tight_constraints_fills_them_regardless_of_fit() {
    // Tight constraints fix the size outright (`is_tight()` short-circuit in
    // `constrain_size_and_attempt_to_preserve_aspect_ratio`), independent of
    // the child's size or the `fit` value.
    for fit in [BoxFit::Contain, BoxFit::Fill, BoxFit::Cover, BoxFit::None] {
        let laid = lay_out(
            FittedBox::new().fit(fit).child(SizedBox::new(10.0, 400.0)),
            tight(200.0, 100.0),
        );
        assert_eq!(
            laid.size(laid.root()),
            size(200.0, 100.0),
            "fit={fit:?}: tight constraints must be filled exactly regardless of fit",
        );
    }
}

#[test]
fn fitted_box_takes_the_child_natural_size_when_it_already_fits_the_loose_bound() {
    // child (100, 50) already fits within the loose 0..200 x 0..200 bound on
    // both axes, so none of the four clamp-and-rescale branches in
    // `constrain_size_and_attempt_to_preserve_aspect_ratio` fire -- the
    // computed size is the child's own size, unchanged.
    let laid = lay_out(
        FittedBox::new().child(SizedBox::new(100.0, 50.0)),
        loose(200.0),
    );
    assert_eq!(laid.size(laid.root()), size(100.0, 50.0));
}
