//! Icons: [`Icon`] draws a glyph from an icon font, [`AnimatedIcon`] vector paths that move
//! through key frames.
//!
//! See [`Icon`]'s docs for what glyph rendering does and does not guarantee today.

mod animated_icon;
mod animated_icon_data;
mod icon;
mod icon_data;
mod icon_theme_data;

pub use animated_icon::{AnimatedIcon, AnimatedIconState};
pub use animated_icon_data::{
    AnimatedIconData, IconCommand, IconCommandBuf, IconDataError, IconPath, IconPathBuf,
};
pub use icon::Icon;
pub use icon_data::IconData;
pub use icon_theme_data::{IconTheme, IconThemeData};
