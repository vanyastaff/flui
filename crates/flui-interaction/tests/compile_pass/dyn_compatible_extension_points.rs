use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_interaction::{
    CustomGestureRecognizer, GestureArenaMember, GestureRecognizer, MultiDragHandle, PointerId,
    routing::PointerDispatch,
};

struct ExternalRecognizer;

impl CustomGestureRecognizer for ExternalRecognizer {
    fn on_arena_accept(&self, _pointer: PointerId) {}

    fn on_arena_reject(&self, _pointer: PointerId) {}
}

impl GestureRecognizer for ExternalRecognizer {
    fn add_pointer(
        self: &Arc<Self>,
        _pointer: PointerId,
        _position: Offset<f64>,
        _global_position: Offset<f64>,
    ) {
    }

    fn handle_event(&self, _dispatch: PointerDispatch<'_>) {}

    fn dispose(&self) {}

    fn primary_pointer(&self) -> Option<PointerId> {
        None
    }
}

fn extension_points(
    _recognizer: &dyn GestureRecognizer,
    _member: &dyn GestureArenaMember,
    _drag: Option<&dyn MultiDragHandle>,
) {
}

fn main() {
    let recognizer = ExternalRecognizer;
    extension_points(&recognizer, &recognizer, None);
}
