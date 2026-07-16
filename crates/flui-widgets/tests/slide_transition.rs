//! End-to-end wiring proof for `SlideTransitionState::build` — the
//! `FractionalTranslation` it constructs actually carries the transition's
//! *current* animated offset and its `transform_hit_tests` flag, not a
//! hardcoded snapshot or the render object's own default.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out, loose};
use flui_animation::ext::AnimatableExt;
use flui_animation::{Animation, AnimationController, Tween};
use flui_objects::TranslationFraction;
use flui_scheduler::Scheduler;
use flui_types::Color;
use flui_widgets::{ColoredBox, GestureDetector, SizedBox, SlideTransition};

fn position_animation(
    begin: TranslationFraction,
    end: TranslationFraction,
) -> (AnimationController, Arc<dyn Animation<TranslationFraction>>) {
    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    let parent: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
    let animation: Arc<dyn Animation<TranslationFraction>> =
        Arc::new(Tween::new(begin, end).animate(parent));
    (controller, animation)
}

/// The built `FractionalTranslation` carries the animation's *current*
/// `TranslationFraction` — not a hardcoded `(0.0, 0.0)`. `FractionalTranslation`
/// applies its offset purely at paint/hit-test time (layout "passes through
/// untouched" — see that type's doc), so there is no parent-relative layout
/// offset to read back; hit-testing at the paint-shifted location is the
/// observable proof the value actually reached it. Under the default
/// `transform_hit_tests(true)`, a tap at the visually-shifted location must
/// hit; a mutant hardcoding the offset to `(0.0, 0.0)` would leave the
/// child's hit region at its original, unshifted position and this tap
/// would miss.
#[test]
fn build_wires_the_animations_current_offset_into_fractional_translation() {
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
                .on_tap(move || {
                    in_cb.fetch_add(1, Ordering::SeqCst);
                })
                .child(SizedBox::new(50.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30)))),
        ),
        loose(200.0),
    );

    // Default `transform_hit_tests(true)`: hit-testing follows the paint
    // shift, so a tap at the visually-shifted location (x=75, one full
    // 50px child-width to the right of the original 0..50 span) must hit.
    laid.dispatch_pointer_down(75.0, 25.0);
    laid.dispatch_pointer_up(75.0, 25.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "the animated dx=1.0 offset should have reached FractionalTranslation, moving the \
         hit-testable region to the shifted location",
    );

    controller.dispose();
}

/// `SlideTransition::transform_hit_tests(false)` must reach the built
/// `FractionalTranslation` — a mutant dropping the
/// `.transform_hit_tests(view.transform_hit_tests)` call would leave
/// `FractionalTranslation`'s own default (`true`) in effect regardless of
/// what the caller requested, flipping which tap location fires.
#[test]
fn build_wires_transform_hit_tests_false_into_fractional_translation() {
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
                .on_tap(move || {
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
