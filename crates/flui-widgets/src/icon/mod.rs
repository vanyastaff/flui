//! Icon-font glyph widgets.
//!
//! Flutter parity: `widgets/icon.dart`, `widgets/icon_data.dart`,
//! `widgets/icon_theme.dart`, `widgets/icon_theme_data.dart`.
//!
//! This is a widget-layer slice only — see [`Icon`]'s docs for what glyph
//! rendering does and does not guarantee today.

mod icon;
mod icon_data;
mod icon_theme_data;

pub use icon::Icon;
pub use icon_data::IconData;
pub use icon_theme_data::{IconTheme, IconThemeData};
