use accesskit::ActionRequest;
use flui_semantics::platform::AccessibilityHandler;

struct MissingAssistiveFocusChanged;

impl AccessibilityHandler for MissingAssistiveFocusChanged {
    fn activation_changed(&self, active: bool) {
        drop(active);
    }

    fn action_requested(&self, request: ActionRequest) {
        drop(request);
    }
}

fn main() {}
