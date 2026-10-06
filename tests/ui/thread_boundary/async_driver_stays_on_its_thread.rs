// An `AsyncDriver` reaches its realm's owner-local task store, whose futures
// may hold `Rc` state: it cannot leave the owner thread. A worker reaches the
// realm through a task's `Waker` or a `FrameWaker` instead.
use flui::view::AsyncDriver;

fn hand_to_a_worker(driver: AsyncDriver) {
    std::thread::spawn(move || drop(driver));
}

fn main() {
    let _ = hand_to_a_worker;
}
