//! A type with no `RenderBox` impl where a box-protocol render object is
//! required. The error must name `RenderBox`/`RenderSliver` as the traits to
//! implement rather than suggesting `RenderObject<BoxProtocol>` itself.
use flui_rendering::{BoxProtocol, RenderObject};

struct NotABox;

fn requires_box<T: RenderObject<BoxProtocol>>(_render_object: T) {}

fn main() {
    requires_box(NotABox);
}
