//! Interaction widgets — modify how a subtree participates in hit-testing and
//! visibility without changing its appearance. Each is a thin
//! [`RenderView`](flui_view::prelude::RenderView) over a `flui-objects` proxy.

mod absorb_pointer;
mod gesture_arena_scope;
mod gesture_detector;
mod ignore_pointer;
mod listener;
mod mouse_region;
mod offstage;
mod visibility;

pub use absorb_pointer::AbsorbPointer;
pub use gesture_arena_scope::GestureArenaScope;
pub use gesture_detector::{GestureDetector, GestureDetectorState};
pub use ignore_pointer::IgnorePointer;
pub use listener::Listener;
pub use mouse_region::MouseRegion;
pub use offstage::Offstage;
pub use visibility::Visibility;
