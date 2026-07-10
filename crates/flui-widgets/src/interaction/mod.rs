//! Interaction widgets — modify how a subtree participates in hit-testing and
//! visibility without changing its appearance. Each is a thin
//! [`RenderView`](flui_view::prelude::RenderView) over a `flui-objects` proxy.

mod absorb_pointer;
mod actions;
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
pub use actions::{Action, Actions, CallbackAction, Intent};
pub(crate) use focus::enclosing_focus_parent;
pub use focus::{Focus, FocusChangeHandler, FocusScope, FocusScopeState, FocusState};
pub use gesture_arena_scope::GestureArenaScope;
pub use gesture_detector::{GestureDetector, GestureDetectorState};
pub use ignore_pointer::IgnorePointer;
pub use listener::Listener;
pub use mouse_region::MouseRegion;
pub use offstage::Offstage;
pub use shortcuts::{CallbackShortcuts, ShortcutCallback, Shortcuts, SingleActivator};
pub use visibility::Visibility;
