//! Axis-configuring constructors for one drag recognizer type.
//!
//! The descriptive aliases below are the same Rust type and impose no axis
//! restriction. Use the corresponding constructor to select the gesture axis.
use super::drag::{DragGestureRecognizer, DragGestureRecognizerBuilder};
use crate::gesture::{arena::GestureArena, constraints::DragAxis};
/// Alias for [`DragGestureRecognizer`]; [`vertical_drag`] selects its axis.
pub type VerticalDragGestureRecognizer = DragGestureRecognizer;
/// Alias for [`DragGestureRecognizer`]; [`horizontal_drag`] selects its axis.
pub type HorizontalDragGestureRecognizer = DragGestureRecognizer;
/// Alias for [`DragGestureRecognizer`]; [`pan`] selects unrestricted motion.
pub type PanGestureRecognizer = DragGestureRecognizer;
/// Configure a vertical-axis drag before allocating shared ownership.
#[must_use]
pub fn vertical_drag(arena: GestureArena) -> DragGestureRecognizerBuilder {
    DragGestureRecognizer::builder(arena, DragAxis::Vertical)
}
/// Configure a horizontal-axis drag before allocating shared ownership.
#[must_use]
pub fn horizontal_drag(arena: GestureArena) -> DragGestureRecognizerBuilder {
    DragGestureRecognizer::builder(arena, DragAxis::Horizontal)
}
/// Configure a free-axis drag before allocating shared ownership.
#[must_use]
pub fn pan(arena: GestureArena) -> DragGestureRecognizerBuilder {
    DragGestureRecognizer::builder(arena, DragAxis::Free)
}
