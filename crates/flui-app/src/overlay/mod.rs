//! Overlay system for FLUI applications.
//!
//! Overlays are widgets that float above the main content, such as:
//! - Dialogs and modals
//! - Tooltips
//! - Dropdown menus
//! - Snackbars and toasts
//! - Debug overlays

mod entry;
mod manager;

pub use entry::{OverlayEntry, OverlayEntryBuilder, OverlayPosition};
pub use manager::OverlayManager;
