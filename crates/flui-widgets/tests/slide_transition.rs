//! End-to-end wiring proof for `SlideTransitionState::build` — the
//! `FractionalTranslation` it constructs actually carries the transition's
//! *current* animated offset and its `transform_hit_tests` flag, not a
//! hardcoded snapshot or the render object's own default.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::common::{lay_out, loose};
use flui_animation::ext::AnimatableExt;
use flui_animation::{Animation, AnimationController, Tween};
use flui_objects::TranslationFraction;
use flui_painting::styling::Color;
use flui_widgets::{ColoredBox, GestureDetector, SizedBox, SlideTransition};

fn position_animation(
    begin: TranslationFraction,
    end: TranslationFraction,
) -> (AnimationController, Arc<dyn Animation<TranslationFraction>>) {
    let controller = AnimationController::without_ticker(Duration::from_millis(300));
    let parent: Arc<dyn Animation<f64>> = Arc::new(controller.clone());
    let animation: Arc<dyn Animation<TranslationFraction>> =
        Arc::new(Tween::new(begin, end).animate(parent));
    (controller, animation)
}

/// `SlideTransition::transform_hit_tests(false)` must reach the built
/// `FractionalTranslation` — a mutant dropping the
/// `.transform_hit_tests(view.transform_hit_tests)` call would leave
/// `FractionalTranslation`'s own default (`true`) in effect regardless of
/// what the caller requested, flipping which tap location fires.
pub(crate) fn build_wires_transform_hit_tests_false_into_fractional_translation() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let (controller, position) = position_animation(
        TranslationFraction::ZERO,
        TranslationFraction::new(1.0, 0.0),
    );
    // Fully shift the child one full width to the right.
    controller.set_value(1.0);

    let laid = lay_out(
        SlideTransition::new(
            position,
            GestureDetector::new()
                .on_tap(move |_cx| {
                    in_cb.fetch_add(1, Ordering::SeqCst);
                })
                .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
        )
        .transform_hit_tests(false),
        loose(200.0),
    );

    // `transform_hit_tests(false)`: hit-testing must stay at the child's
    // ORIGINAL (unshifted) layout position — the visually-shifted location
    // (x=75, one full 50px child-width to the right of the original 0..50
    // span) must NOT register a tap.
    laid.dispatch_pointer_down(75.0, 25.0);
    laid.dispatch_pointer_up(75.0, 25.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "transform_hit_tests(false) must not follow the paint shift — a tap at the \
         visually-shifted location should miss",
    );

    // The original (unshifted) location must still register — proving the
    // flag reached `FractionalTranslation` rather than being silently
    // dropped mid-`build`.
    laid.dispatch_pointer_down(25.0, 25.0);
    laid.dispatch_pointer_up(25.0, 25.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "transform_hit_tests(false) leaves hit-testing at the child's original layout position",
    );

    controller.dispose();
}
