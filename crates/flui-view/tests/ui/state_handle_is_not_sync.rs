use flui_view::StateHandle;

fn require_sync<T: Sync>(_: T) {}

fn main() {
    require_sync(StateHandle::new(String::new()));
}
