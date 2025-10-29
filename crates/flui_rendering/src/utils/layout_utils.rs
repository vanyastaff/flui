//! Common layout utilities to reduce duplication
//!
//! This module provides helper functions for common layout patterns
//! used across RenderStack, RenderFlex, RenderWrap, and other layout objects.

use flui_engine::{BoxedLayer, Transform, TransformLayer};
use flui_types::Offset;

/// Apply offset transform to a child layer
///
/// This is a common pattern in paint() methods: if offset is non-zero,
/// wrap the child in a TransformLayer; otherwise return child directly.
///
/// # Example
///
/// ```rust,ignore
/// // Before (duplicated in stack.rs, flex.rs, indexed_stack.rs):
/// if child_offset != Offset::ZERO {
///     let transform = Transform::Translate(child_offset);
///     let transform_layer = TransformLayer::new(child_layer, transform);
///     container.add_child(Box::new(transform_layer));
/// } else {
///     container.add_child(child_layer);
/// }
///
/// // After:
/// container.add_child(apply_offset_transform(child_layer, child_offset));
/// ```
///
/// # Parameters
///
/// - `child_layer`: The child layer to potentially transform
/// - `offset`: The offset to apply (ZERO means no transform)
///
/// # Returns
///
/// - `BoxedLayer`: Either the original layer or a TransformLayer wrapping it
#[inline]
pub fn apply_offset_transform(child_layer: BoxedLayer, offset: Offset) -> BoxedLayer {
    if offset == Offset::ZERO {
        child_layer
    } else {
        let transform = Transform::Translate(offset);
        Box::new(TransformLayer::new(child_layer, transform))
    }
}

/// Alternative version using TransformLayer::translate for newer API
///
/// Some code uses `TransformLayer::translate()` instead of `TransformLayer::new()`
/// with `Transform::Translate`. This provides a unified interface.
#[inline]
pub fn apply_offset_transform_v2(child_layer: BoxedLayer, offset: Offset) -> BoxedLayer {
    if offset == Offset::ZERO {
        child_layer
    } else {
        Box::new(TransformLayer::translate(child_layer, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_engine::ContainerLayer;

    #[test]
    fn test_apply_offset_transform_zero() {
        let child = Box::new(ContainerLayer::new()) as BoxedLayer;
        let result = apply_offset_transform(child, Offset::ZERO);
        // Should return the same layer (no transform)
        // We can't directly test this without downcasting, but we verify it compiles
        drop(result);
    }

    #[test]
    fn test_apply_offset_transform_nonzero() {
        let child = Box::new(ContainerLayer::new()) as BoxedLayer;
        let offset = Offset::new(10.0, 20.0);
        let result = apply_offset_transform(child, offset);
        // Should wrap in TransformLayer
        drop(result);
    }

    #[test]
    fn test_apply_offset_transform_v2_zero() {
        let child = Box::new(ContainerLayer::new()) as BoxedLayer;
        let result = apply_offset_transform_v2(child, Offset::ZERO);
        drop(result);
    }

    #[test]
    fn test_apply_offset_transform_v2_nonzero() {
        let child = Box::new(ContainerLayer::new()) as BoxedLayer;
        let offset = Offset::new(10.0, 20.0);
        let result = apply_offset_transform_v2(child, offset);
        drop(result);
    }
}
