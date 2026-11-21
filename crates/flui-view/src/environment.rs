//! Inherited presentation data and subtree-scoped presentation services:
//! [`MediaQuery`]/[`MediaQueryData`], the gesture arena and its inherited
//! input policy ([`GestureArenaScope`], [`GesturePolicy`]), the shared
//! frame registry for implicitly-animated widgets ([`VsyncScope`]), and the
//! presentation's root focus scope ([`FocusTreeRoot`]); and the platform whose
//! conventions the subtree follows ([`PlatformConvention`]).
//!
//! These are the views the runtime installs above one root and the catalog,
//! the runtime, and the packages all read; the widget catalog owns nothing
//! about them.
//!
//! # [`MediaQuery`]
//!
//! ## Implemented subset
//!
//! `size`, `device_pixel_ratio`, `text_sizing`, `font_weight_adjustment`, `view_padding`,
//! `padding`, `view_insets`, `system_gesture_insets`, `platform_brightness`, `high_contrast`,
//! `preferred_locales`, `regional_formats`, `motion`, `accent_color`, `scroll_bars` —
//! presentation and preference fields for layout and theming.
//!
//! ## Deferred (not yet implemented)
//!
//! Accessible navigation, inverted colors, display features, and
//! navigation mode. Text-weight observation
//! is implemented by the UIKit backend; other backends leave it unavailable.
//! These require platform event plumbing (accessibility bridge, IME state)
//! that lives above this layer.

mod floating_keyboard;
mod focus_tree_root;
mod gesture_arena_scope;
mod media_query;
mod platform_convention;
mod vsync_scope;

pub use floating_keyboard::FloatingKeyboard;
pub use focus_tree_root::{FocusTreeRoot, FocusTreeRootState};
pub use gesture_arena_scope::{GestureArenaScope, GestureArenaScopeState, GesturePolicy};
pub use media_query::{MediaQuery, MediaQueryData};
pub use platform_convention::PlatformConvention;
pub use vsync_scope::VsyncScope;
