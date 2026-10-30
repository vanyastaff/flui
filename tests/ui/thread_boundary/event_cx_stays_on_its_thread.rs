use flui::view::EventCx;

// An `EventCx` exists only for one dispatch; the bound is checked on the type.
fn require_send<T: Send>() {}

fn main() {
    require_send::<EventCx<'static>>();
}
