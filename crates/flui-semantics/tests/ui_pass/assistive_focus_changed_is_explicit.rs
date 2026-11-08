use accesskit::{ActionRequest, NodeId};
use flui_semantics::platform::AccessibilityHandler;

struct ExplicitAssistiveFocusChanged;

impl AccessibilityHandler for ExplicitAssistiveFocusChanged {
    fn activation_changed(&self, _: bool) {}

    fn action_requested(&self, request: ActionRequest) {
        drop(request);
    }

    fn assistive_focus_changed(&self, _: Option<NodeId>) {}
}

fn main() {}
