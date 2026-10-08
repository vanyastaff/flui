// An `AsyncDriver` reaches its ui_runtime's owner-local task store, whose futures
// may hold `Rc` state: it cannot leave the owner thread. A worker reaches the
// ui_runtime through a task's `Waker` or a `FrameWaker` instead.
//
// The `Send + 'static` bound is `std::thread::spawn`'s, stated here so the
// diagnostic names no std source: without `rust-src` rustc omits std
// snippets, and the expected output would differ between toolchains.
use flui::view::AsyncDriver;

fn require_thread_spawnable<T: Send + 'static>(_: T) {}

fn hand_to_a_worker(driver: AsyncDriver) {
    require_thread_spawnable(driver);
}

fn main() {
    let _ = hand_to_a_worker;
}
