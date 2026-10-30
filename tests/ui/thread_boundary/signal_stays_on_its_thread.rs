use flui::view::Signal;

fn require_thread_spawnable<T: Send + 'static>(_: T) {}

fn owner_signal(count: Signal<u32>) {
    require_thread_spawnable(count);
}

fn main() {
    let _ = owner_signal;
}
