use flui::view::Writer;

// `Writer` has no public constructor: application code only borrows one, so
// the bound is checked on the type.
fn require_thread_spawnable<T: Send + 'static>() {}

fn main() {
    require_thread_spawnable::<Writer>();
}
