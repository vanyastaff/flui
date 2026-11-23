use flui::animation::{AnimationController, AnimationRunFuture, Vsync};
use flui::foundation::ChangeNotifier;
use flui::scheduler::PostFrameHandle;

fn require_thread_spawnable<T: Send + 'static>(_: T) {}

fn owner_capabilities(
    laid: &flui_testing::widgets::LaidOut,
    post_frame: PostFrameHandle,
    run: AnimationRunFuture,
    controller: AnimationController,
    vsync: Vsync,
    notifier: ChangeNotifier,
) {
    require_thread_spawnable(laid.scheduler());
    require_thread_spawnable(post_frame);
    require_thread_spawnable(run);
    require_thread_spawnable(controller);
    require_thread_spawnable(vsync);
    require_thread_spawnable(notifier);
}

fn main() {
    let _ = owner_capabilities;
}
