use flui_interaction::gesture::GestureContest;
use flui_interaction::{
    CancelOutcome, GestureArenaMember, GestureRecognizer, MultiDragHandle,
    pointer::PointerDispatch,
};

struct ExternalRecognizer;

impl GestureArenaMember for ExternalRecognizer {
    fn accept_gesture(&self, _: GestureContest) {}

    fn reject_gesture(&self, _: GestureContest) {}
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
