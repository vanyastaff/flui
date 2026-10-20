//! `RendererBinding` lends render views through a closure and hands out no
//! lock: a view survives a panicking visitor, and a nested call cannot
//! deadlock on it.

use flui_rendering::{
    TextContextHandle,
    binding::RendererBinding,
    constraints::BoxConstraints,
    view::{RenderView, ViewConfiguration},
};
use flui_runtime::renderer_binding::RenderingBinding;

fn view_at_ratio(device_pixel_ratio: f64) -> RenderView {
    let constraints = BoxConstraints::tight_for_finite(100.0, 100.0);
    RenderView::with_configuration(ViewConfiguration::new(
        constraints,
        constraints,
        device_pixel_ratio,
    ))
}

fn ratio_of(binding: &RenderingBinding, view_id: u64) -> Option<f64> {
    let mut ratio = None;
    binding.with_render_view(view_id, &mut |view| {
        ratio = Some(view.configuration().device_pixel_ratio());
    });
    ratio
}

#[test]
fn a_view_lent_to_a_panicking_visitor_stays_in_the_binding() {
    let binding = RenderingBinding::new(TextContextHandle::standalone());
    binding.insert_render_view(1, view_at_ratio(2.0));

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        binding.with_render_view(1, &mut |_| panic!("visitor fails"));
    }));

    assert!(unwound.is_err());
    assert_eq!(ratio_of(&binding, 1), Some(2.0));
}

#[test]
fn a_nested_lend_of_the_same_view_finds_none_and_a_replacement_wins() {
    let binding = RenderingBinding::new(TextContextHandle::standalone());
    binding.insert_render_view(1, view_at_ratio(2.0));

    let mut nested = None;
    let lent = binding.with_render_view(1, &mut |_| {
        nested = Some(binding.with_render_view(1, &mut |_| {}));
        binding.insert_render_view(1, view_at_ratio(3.0));
    });

    assert!(lent);
    assert_eq!(nested, Some(false), "a lent view is not lent twice");
    assert_eq!(
        ratio_of(&binding, 1),
        Some(3.0),
        "the view inserted during the lend replaces the lent one"
    );
    assert!(!binding.with_render_view(2, &mut |_| {}));
}
