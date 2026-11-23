use std::{cell::Cell, rc::Rc, time::Duration};

use flui::animation::{Animation, AnimationController, AnimationStatus, Vsync};
use flui::foundation::ChangeNotifier;
use flui::scheduler::{FrameWaker, PostFrameHandle};
use flui::widgets::SizedBox;
use flui_testing::widgets::{lay_out, tight};

fn require_thread_spawnable<T: Send + Sync + 'static>(_: T) {}

fn main() {
    let mut laid = lay_out(SizedBox::new(10.0, 10.0), tight(10.0, 10.0));
    let scheduler = laid.scheduler();
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    PostFrameHandle::new(&scheduler)
        .schedule(move |_| observed.set(observed.get() + 1))
        .expect("live owner");
    laid.pump_for(Duration::from_millis(16));
    assert_eq!(calls.get(), 1, "the UI runtime's next frame runs the post-frame callback");

    let notifier = ChangeNotifier::new();
    let observed = calls.clone();
    let _listening = notifier.listen(Rc::new(move || observed.set(observed.get() + 1)));
    notifier.notify_listeners();

    let vsync = Vsync::new();
    let mut owner = AnimationController::builder(Duration::from_millis(100)).build_on(Some(&vsync));
    let controller = owner.controller().clone();
    let observed = calls.clone();
    controller
        .listen_status(Rc::new(move |status| {
            if status == AnimationStatus::ForwardSettled {
                observed.set(observed.get() + 1);
            }
        }))
        .forget();
    let outcome = controller.forward().expect("live controller");
    let observed = calls.clone();
    outcome.when_complete_or_cancel(move |result| {
        assert!(result.is_ok());
        observed.set(observed.get() + 1);
    });
    // An application never ticks a registry: the test window's frames drive the one it nests.
    laid.adopt_vsync(vsync.clone()).expect("the registry nests under the window's");
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(100));
    assert_eq!(calls.get(), 4);
    owner.dispose();
    assert!(vsync.is_empty());
    assert!(matches!(
        controller.forward(),
        Err(flui::animation::AnimationError::Disposed)
    ));

    let wake: FrameWaker = scheduler.frame_waker();
    require_thread_spawnable(wake);
}
