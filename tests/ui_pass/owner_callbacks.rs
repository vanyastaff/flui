use std::{cell::Cell, rc::Rc, time::Duration};

use flui::animation::{Animation, AnimationController, AnimationStatus, Vsync};
use flui::foundation::{ChangeNotifier, Listenable};
use flui::view::{FrameWaker, PostFrameHandle};

fn require_thread_spawnable<T: Send + Sync + 'static>(_: T) {}

fn main() {
    let binding = flui_testing::HeadlessBinding::new();
    let scheduler = binding.scheduler();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    PostFrameHandle::new(scheduler).schedule(move |_| observed.set(observed.get() + 1))
        .expect("live owner");

    let notifier = ChangeNotifier::new();
    let observed = calls.clone();
    notifier.add_listener(Rc::new(move || observed.set(observed.get() + 1)));
    notifier.notify_listeners();

    let vsync = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_millis(100)).build_on(Some(&vsync));
    let controller = owner.controller().clone();
    let observed = calls.clone();
    controller.add_status_listener(Rc::new(move |status| {
        if status == AnimationStatus::Completed { observed.set(observed.get() + 1); }
    }));
    let outcome = controller.forward().expect("live controller");
    let observed = calls.clone();
    outcome.when_complete_or_cancel(move |result| {
        assert!(result.is_ok());
        observed.set(observed.get() + 1);
    });
    vsync.tick_all(&flui::animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)));
    vsync.tick_all(&flui::animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.1)));
    assert_eq!(calls.get(), 3);
    owner.dispose();
    assert!(vsync.is_empty());
    assert!(matches!(controller.forward(), Err(flui::animation::AnimationError::Disposed)));

    let wake: FrameWaker = scheduler.frame_waker();
    require_thread_spawnable(wake);
}
