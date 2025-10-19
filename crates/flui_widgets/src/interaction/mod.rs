//! Interaction widgets - widgets that control pointer event handling
//!
//! This module provides widgets for controlling how pointer events are handled:
//!
//! - **IgnorePointer**: Makes widget transparent to pointer events (events pass through)
//! - **AbsorbPointer**: Blocks pointer events from reaching widgets behind

pub mod absorb_pointer;
pub mod ignore_pointer;

pub use absorb_pointer::AbsorbPointer;
pub use ignore_pointer::IgnorePointer;
