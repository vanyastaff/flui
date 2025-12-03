//! Framework bindings
//!
//! Bindings connect flui-platform to other FLUI subsystems.
//! Following Flutter's `*Binding` pattern for clean integration:
//!
//! - `SchedulerBinding` - Frame scheduling and task management
//! - `GestureBinding` - Safe hit testing and event routing

mod gesture_binding;
mod scheduler_binding;

pub use gesture_binding::{EventRouterExt, GestureBinding};
pub use scheduler_binding::{SchedulerBinding, SchedulerStats};
