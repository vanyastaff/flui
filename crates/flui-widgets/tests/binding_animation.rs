//! End-to-end animation through [`HeadlessBinding::pump_frame`]: a
//! `FadeTransition` over an [`AnimationController`] *registered with the
//! binding* advances its opacity frame-to-frame as virtual time is pumped — no
//! `set_value`, no `thread::sleep`.
//!
//! This exercises the full Phase-1b chain inside a single `pump_frame`: the
//! binding ticks the registered controller (`tick_at`) → the controller
//! notifies its listeners → the `FadeTransition`'s subscription marks it dirty
//! into the build inbox → `build_scope` drains it and rebuilds → `run_frame`
//! re-reads the new value into the `Opacity` render object. It would fail if the
//! controller were not ticked inside the frame, or if the tick ran *after*
//! `build_scope` (the dirty entry would miss this frame's drain — a one-frame
//! lag).

use std::time::Duration;

use crate::common::tight;
use flui_animation::{Animation, AnimationController, AnimationStatus};
use flui_widgets::{FadeTransition, SizedBox};

pub(crate) fn fade_source_replacement_retries_after_status_failure() {
    use flui_animation::{StatusObserver, StatusSubscription};
    use flui_foundation::{Listenable, ListenerId, ListenerObserver};
    use std::{cell::Cell, rc::Rc};

    #[derive(Debug)]
    struct RefusedOnce {
        controller: AnimationController,
        armed: Cell<bool>,
    }
    impl Listenable for RefusedOnce {
        fn add_observer(&self, observer: ListenerObserver) -> ListenerId {
            self.controller.add_observer(observer)
        }
        fn remove_listener(&self, id: ListenerId) {
            self.controller.remove_listener(id);
        }
        fn remove_all_listeners(&self) {
            self.controller.remove_all_listeners();
        }
    }
    impl Animation<f64> for RefusedOnce {
        fn value(&self) -> f64 {
            self.controller.value()
        }
        fn status(&self) -> AnimationStatus {
            assert!(!self.armed.replace(false), "first replacement status read");
            self.controller.status()
        }
        fn is_animating(&self) -> bool {
            self.controller.is_animating()
        }
        fn observe_status(&self, observer: StatusObserver) -> StatusSubscription {
            self.controller.observe_status(observer)
        }
    }
    let first = AnimationController::builder(Duration::from_millis(100)).build();
    first.set_value(0.25).expect("no build is running");
    let second = Rc::new(RefusedOnce {
        controller: AnimationController::builder(Duration::from_millis(100)).build(),
        armed: Cell::new(true),
    });
    second
        .controller
        .set_value(0.75)
        .expect("no build is running");
    let view = |animation: Rc<dyn Animation<f64>>| {
        FadeTransition::new(animation, SizedBox::new(40.0, 40.0))
    };
    let mut laid = crate::common::lay_out(view(Rc::new(first)), tight(40.0, 40.0));
    let nodes = laid.find_all_by_render_type("RenderAnimatedOpacity");
    assert_eq!(nodes.len(), 1);
    assert_eq!(laid.opacity(nodes[0]), 0.25);
    laid.pump_widget(view(second.clone()));
    assert!(
        !second.armed.get(),
        "the source's one-shot refusal was reached and contained"
    );
    laid.pump_widget(view(second.clone()));
    let nodes = laid.find_all_by_render_type("RenderAnimatedOpacity");
    assert_eq!(nodes.len(), 1);
    assert_eq!(
        laid.opacity(nodes[0]),
        0.75,
        "the next update retries the source that failed before commitment"
    );
    second
        .controller
        .set_value(0.5)
        .expect("no build is running");
    laid.tick();
    assert_eq!(
        laid.opacity(nodes[0]),
        0.5,
        "the accepted replacement remains subscribed"
    );
}

/// A registered, running controller drives a `FadeTransition`'s opacity upward
/// frame-to-frame as `pump_for` advances virtual time.
pub(crate) fn registered_controller_advances_fade_opacity_frame_to_frame() {
    let vsync = flui_animation::Vsync::new();
    let owner = AnimationController::builder(Duration::from_millis(100)).build_on(Some(&vsync));
    let controller = owner.controller();
    // The `Arc<dyn Animation>` handed to the FadeTransition and the clone
    // registered with the binding share the same inner notifier, so a binding
    // tick notifies the transition's listener.
    let opacity: std::rc::Rc<dyn Animation<f64>> = std::rc::Rc::new(controller.clone());

    let mut laid = flui_testing::widgets::lay_out_animated(
        FadeTransition::new(opacity, SizedBox::new(100.0, 50.0)),
        tight(100.0, 50.0),
        vsync,
    );
    let render_opacity = laid.root();

    // Register before starting, then start: the binding re-anchors this run's
    // `t = 0` on the first pump that observes the new run-generation.
    controller.forward().expect("a fresh controller forwards");

    // The detection frame (first pump after `forward`) holds the run-start value
    // — a ticker's first tick delivers elapsed 0. Movement begins next pump.
    laid.pump_for(Duration::from_millis(20));
    assert!(
        laid.opacity(render_opacity).abs() < 1e-4,
        "first pump holds the run-start opacity (0.0), got {}",
        laid.opacity(render_opacity),
    );

    // Five more 20ms pumps over a 100ms run: opacity climbs 0.2, 0.4, 0.6, 0.8,
    // 1.0 (the controller's default tween is linear).
    let mut samples = Vec::new();
    for _ in 0..5 {
        laid.pump_for(Duration::from_millis(20));
        samples.push(laid.opacity(render_opacity));
    }

    // Strictly increasing — proves the value is re-read each frame, not stuck.
    for pair in samples.windows(2) {
        assert!(
            pair[1] > pair[0] - 1e-6,
            "opacity must not regress across pumps: {samples:?}",
        );
    }
    // An intermediate frame sits strictly between the endpoints (~0.6 here),
    // proving the in-between frames render partial opacity, not a snap.
    let intermediate = samples[2];
    assert!(
        intermediate > 0.05 && intermediate < 0.95,
        "an intermediate frame shows partial opacity, got {intermediate}",
    );
    // The run completes at full opacity.
    let final_opacity = samples[4];
    assert!(
        (final_opacity - 1.0).abs() < 1e-4,
        "the run ends at full opacity, got {final_opacity}",
    );
    assert_eq!(
        controller.status(),
        AnimationStatus::ForwardSettled,
        "a fully pumped forward run completes",
    );
}
