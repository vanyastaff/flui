//! ADR-0305 §2: a `SurfaceFactory` stays on the thread that bootstrapped its
//! context (a window system creates surfaces on its own thread), so handing it
//! to another thread does not type-check. The surfaces it makes cross to the
//! executor; the factory does not.

// A local bound rather than `std::thread::spawn`: the note chain then stays
// out of std's sources, whose lines rustc quotes only when `rust-src` is
// installed (see `raster_backend_requires_send.rs`).
fn crosses_threads<T: Send>(_value: T) {}

fn hand_off(factory: flui_engine::SurfaceFactory) {
    crosses_threads(factory);
}

fn main() {}
