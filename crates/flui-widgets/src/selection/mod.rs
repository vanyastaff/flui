//! [`SelectionArea`]: static text selected with a mouse, touch and the keyboard, and copied.
//!
//! The area owns one selection across every paragraph beneath it, in reading order. Each
//! [`Text`](crate::Text) or [`RichText`](crate::RichText) under it registers itself and paints
//! its part; the area reads their laid-out text to map pointers and keys to positions.

mod area;
mod handles;
mod model;

pub use crate::text::SelectableId;
pub use area::{SelectionArea, SelectionAreaState, SelectionContainer};
pub use handles::{DotHandles, HandleSide, SelectionHandles};
pub use model::{
    AreaSelection, SelectedContent, SelectionController, SelectionError, SelectionPoint,
};
