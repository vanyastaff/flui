use flui::view::StateCell;

fn require_thread_spawnable<T: Send + 'static>(_: T) {}

fn owner_cell(cell: StateCell<u32>) {
    require_thread_spawnable(cell);
}

fn main() {
    let _ = owner_cell;
}
