use flui_native::Platform;
use std::sync::Arc;

fn assert_send<T: Send>() {}

fn main() {
    // A platform stays on the thread that made it: another thread reaches it
    // through its `PlatformProxy`.
    assert_send::<Arc<dyn Platform>>();
}
