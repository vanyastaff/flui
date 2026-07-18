//! Interaction widgets — modify how a subtree participates in hit-testing and
//! visibility without changing its appearance. Each is a thin
//! [`RenderView`](flui_view::prelude::RenderView) over a `flui-objects` proxy.

mod absorb_pointer;
mod actions;
mod dismissible;
mod focus;
mod gesture_arena_scope;
mod gesture_detector;
mod ignore_pointer;
mod listener;
mod mouse_region;
mod offstage;
mod shortcuts;
mod visibility;

pub use absorb_pointer::AbsorbPointer;
pub use actions::{
    Action, ActionOutcome, Actions, CallbackAction, Intent, NextFocusAction, NextFocusIntent,
    PreviousFocusAction, PreviousFocusIntent,
};
pub use dismissible::{
    DismissDirection, DismissDirectionCallback, DismissUpdateCallback, DismissUpdateDetails,
    Dismissible, DismissibleState,
};
pub use focus::{ExcludeFocus, Focus, FocusChangeHandler, FocusScope, FocusScopeState, FocusState};
pub(crate) use focus::{enclosing_focus_parent, install_rect_provider};
pub use gesture_arena_scope::GestureArenaScope;
pub use gesture_detector::{GestureDetector, GestureDetectorState};
pub use ignore_pointer::IgnorePointer;
pub use listener::Listener;
pub use mouse_region::MouseRegion;
pub use offstage::Offstage;
pub use shortcuts::{CallbackShortcuts, ShortcutCallback, Shortcuts, SingleActivator};
pub use visibility::Visibility;
