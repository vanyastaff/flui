use flui_interaction::gesture::TapGestureRecognizer;

fn assert_send<T: Send>() {}

fn main() {
    assert_send::<TapGestureRecognizer>();
}
