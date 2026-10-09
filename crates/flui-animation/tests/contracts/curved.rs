//! `CurvedAnimation` over a running parent.

use std::time::Duration;

use flui_animation::{Animation, AnimationController, Cubic, Curve, CurvedAnimation, Curves};

#[test]
fn reverse_curve_locked_to_run_entry_direction() {
    // A run that entered Forward keeps
    // the forward curve even if the parent's status flips to Reverse
    // mid-run; the reverse curve only applies to a run entered in
    // Reverse. Without the lock, a mid-run `reverse()` would swap curves
    // underneath the value and cause a visual jump.
    let controller =
        std::rc::Rc::new(AnimationController::builder(Duration::from_millis(100)).build());
    // Forward curve is the identity cubic; the reverse curve is strongly
    // sub-linear at t=0.5, so any curve swap is observable there.
    let curved = CurvedAnimation::new(
        controller.clone() as std::rc::Rc<dyn Animation<f64>>,
        Cubic::new(0.0, 0.0, 1.0, 1.0), // y(x) = x
    )
    .with_reverse_curve(Curves::EaseInQuint);

    controller.set_value(0.5);
    let _ = controller.forward();
    let during_forward = curved.value();

    // Flip direction mid-run: the captured Forward direction must keep
    // the (≈linear) forward curve active.
    let _ = controller.reverse();
    let during_flip = curved.value();
    assert!(
        (during_forward - during_flip).abs() < 1e-3,
        "mid-run direction flip must not swap curves (forward {during_forward} vs flipped {during_flip})"
    );

    // Settle the run, then start a fresh run in Reverse: now the reverse
    // curve applies from the start.
    controller.set_value(1.0); // Completed -> direction lock cleared
    let _ = controller.reverse();
    controller.set_value(0.5);
    let reverse_run = curved.value();
    let expected = Curves::EaseInQuint.transform(0.5);
    assert!(
        (reverse_run - expected).abs() < 1e-3,
        "a run entered in Reverse must use the reverse curve ({reverse_run} vs {expected})"
    );

    controller.dispose();
}
