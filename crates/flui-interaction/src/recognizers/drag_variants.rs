//! Axis-specific entry points for the same drag recognizer.
use super::drag::{DragGestureRecognizer, DragGestureRecognizerBuilder};
use crate::{arena::GestureArena, traits::DragAxis};
/// Vertical-axis drag, configured through [`vertical_drag`].
pub type VerticalDragGestureRecognizer = DragGestureRecognizer;
/// Horizontal-axis drag, configured through [`horizontal_drag`].
pub type HorizontalDragGestureRecognizer = DragGestureRecognizer;
/// Free-axis drag, configured through [`pan`].
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
