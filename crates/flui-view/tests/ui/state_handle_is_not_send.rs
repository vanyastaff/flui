use flui_view::StateHandle;

fn require_send<T: Send>(_: T) {}

fn main() {
    require_send(StateHandle::new(String::new()));
}
