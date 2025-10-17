//! Interaction and state types.
//!
//! This module contains types for user interaction and widget states:
//! - [`WidgetState`]: Widget states (default, hovered, pressed, focused, disabled)
//! - [`WidgetStates`]: Collection of boolean state flags
//! - [`InputType`]: Input field types (text, number, email, etc.)
//! - [`InputMode`]: Virtual keyboard modes
//! - [`ValidationState`], [`ValidationError`]: Form validation
//! - [`Curve`]: Animation curves (ease-in, ease-out, etc.)
//! - Common UI enums (Cursor, Visibility, Overflow, etc.)

pub mod curves;
pub mod enums;
pub mod input;
pub mod state;
pub mod validation;

// Re-export types for convenience
pub use curves::{Curve, Curves};
pub use enums::*;
pub use input::{Autocorrect, InputMode, InputType, TextCapitalization};
pub use state::{WidgetState, WidgetStates};
pub use validation::{ValidationError, ValidationResult, ValidationState};








