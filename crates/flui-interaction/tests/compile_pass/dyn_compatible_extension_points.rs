use flui_interaction::{
    CancelOutcome, GestureArenaMember, GestureRecognizer, MultiDragHandle, PointerId,
    routing::PointerDispatch,
};

struct ExternalRecognizer;

impl GestureArenaMember for ExternalRecognizer {
    fn accept_gesture(&self, _pointer: PointerId) {}

    fn reject_gesture(&self, _pointer: PointerId) {}
}

impl GestureRecognizer for ExternalRecognizer {
    fn add_pointer(&self, _down: PointerDispatch<'_>) {}

    fn handle_event(&self, _dispatch: PointerDispatch<'_>) {}

    fn cancel(&self) -> CancelOutcome {
        CancelOutcome::Idle
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
