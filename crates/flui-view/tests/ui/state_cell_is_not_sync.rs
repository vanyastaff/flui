use flui_view::StateCell;

fn require_sync<T: Sync>(_: T) {}

fn main() {
    require_sync(StateCell::new(0_u32));
}
