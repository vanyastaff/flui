//! Element types for the view layer.
//!
//! This module provides ViewElement and related types for managing
//! component views in the element tree.
//!
//! # Key Types
//!
//! - [`ViewElement`] - Element for component views (Stateless, Stateful, Provider)
//! - [`ViewLifecycle`] - Lifecycle states (Initial, Active, Inactive, Defunct)
//! - [`ViewFlags`] / [`AtomicViewFlags`] - Lock-free dirty tracking
//!
//! # Architecture
//!
//! ViewElement is independent of flui-element's Element type, allowing
//! flui-view to be a lower-level dependency. The element tree in flui-element
//! can wrap ViewElement in its Element enum.
//!
//! ```text
//! flui-view (ViewElement, ViewLifecycle, ViewFlags)
//!     ↓
//! flui-element (Element enum wraps ViewElement)
//! ```

mod flags;
mod lifecycle;
mod view_element;

pub use flags::{AtomicViewFlags, ViewFlags};
pub use lifecycle::ViewLifecycle;
pub use view_element::{PendingChildren, ViewElement};
