use flui_interaction::TapGestureRecognizer;

fn assert_send<T: Send>() {}

fn main() {
    assert_send::<TapGestureRecognizer>();
}
