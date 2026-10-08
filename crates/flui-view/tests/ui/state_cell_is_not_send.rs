use flui_view::StateCell;

fn require_send<T: Send>(_: T) {}

fn main() {
    require_send(StateCell::new(0_u32));
}
