use flui_native::HeadlessHostWindow;

fn assert_send<T: Send>() {}

fn main() {
    // A window stays on its owner thread: another thread asks it for a frame
    // through its redraw requester.
    assert_send::<HeadlessHostWindow>();
}
