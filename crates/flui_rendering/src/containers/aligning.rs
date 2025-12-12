//! Aligning container for alignment-based positioning

use flui_tree::arity::{Arity, Exact};
use flui_types::{Alignment, Offset};

use crate::containers::Single;
use crate::protocol::{BoxProtocol, Protocol};

/// Container for single child with alignment and size factors
///
/// `Aligning<P, A>` is used for render objects that position their child using alignment
/// and optional width/height factors (like Align, Center widgets).
///
/// By default, Aligning uses `Exact<1>` arity to ensure exactly one child is present.
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
/// - `A`: The arity constraint (Exact<1> by default for exactly one child)
///
/// # Examples
///
/// ```ignore
/// use flui_rendering::containers::Aligning;
/// use flui_rendering::protocol::BoxProtocol;
///
/// struct RenderAlign {
///     aligning: Aligning<BoxProtocol>,
/// }
///
/// impl RenderBox for RenderAlign {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///         if let Some(child) = self.aligning.child_mut() {
///             let child_size = child.perform_layout(constraints.loosen());
///
///             // Compute parent size based on factors
///             let width = self.aligning.width_factor()
///                 .map(|f| child_size.width * f)
///                 .unwrap_or(constraints.max_width);
///
///             let height = self.aligning.height_factor()
///                 .map(|f| child_size.height * f)
///                 .unwrap_or(constraints.max_height);
///
///             let size = constraints.constrain(Size::new(width, height));
///
///             // Compute child offset from alignment
///             let offset = self.aligning.alignment().resolve(child_size, size);
///             self.aligning.set_offset(offset);
///             self.aligning.set_geometry(size);
///
///             size
///         } else {
///             constraints.smallest()
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Aligning<P: Protocol, A: Arity = Exact<1>> {
    child: Single<P, A>,
    geometry: P::Geometry,
    offset: Offset,
    alignment: Alignment,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
}

impl<P: Protocol, A: Arity> Aligning<P, A> {
    /// Creates a new aligning container with center alignment
    pub fn new() -> Self {
        Self {
            child: Single::new(),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
            alignment: Alignment::CENTER,
            width_factor: None,
            height_factor: None,
        }
    }

    /// Creates a new aligning container with specified alignment
    pub fn with_alignment(alignment: Alignment) -> Self {
        Self {
            child: Single::new(),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
            alignment,
            width_factor: None,
            height_factor: None,
        }
    }

    /// Returns a reference to the child, if any
    pub fn child(&self) -> Option<&P::Object> {
        self.child.child()
    }

    /// Returns a mutable reference to the child, if any
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.child_mut()
    }

    /// Sets the child
    pub fn set_child(&mut self, child: Box<P::Object>) {
        self.child.set_child(child);
    }

    /// Takes the child
    pub fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.child.take_child()
    }

    /// Returns whether this container has a child
    pub fn has_child(&self) -> bool {
        self.child.has_child()
    }

    /// Returns a reference to the geometry
    pub fn geometry(&self) -> &P::Geometry {
        &self.geometry
    }

    /// Sets the geometry
    pub fn set_geometry(&mut self, geometry: P::Geometry) {
        self.geometry = geometry;
    }

    /// Returns a mutable reference to the geometry
    pub fn geometry_mut(&mut self) -> &mut P::Geometry {
        &mut self.geometry
    }

    /// Returns the child offset
    pub fn offset(&self) -> &Offset {
        &self.offset
    }

    /// Sets the child offset
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Returns a mutable reference to the offset
    pub fn offset_mut(&mut self) -> &mut Offset {
        &mut self.offset
    }

    /// Returns the alignment
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// Sets the alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }

    /// Returns the width factor
    pub fn width_factor(&self) -> Option<f32> {
        self.width_factor
    }

    /// Sets the width factor
    ///
    /// If Some, the parent's width will be the child's width multiplied by this factor.
    /// If None, the parent's width will be as large as possible.
    pub fn set_width_factor(&mut self, factor: Option<f32>) {
        self.width_factor = factor;
    }

    /// Returns the height factor
    pub fn height_factor(&self) -> Option<f32> {
        self.height_factor
    }

    /// Sets the height factor
    ///
    /// If Some, the parent's height will be the child's height multiplied by this factor.
    /// If None, the parent's height will be as large as possible.
    pub fn set_height_factor(&mut self, factor: Option<f32>) {
        self.height_factor = factor;
    }
}

impl<P: Protocol, A: Arity> Default for Aligning<P, A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for Box aligning container
pub type AligningBox = Aligning<BoxProtocol>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let aligning: AligningBox = AligningBox::new();
        assert!(!aligning.has_child());
        assert_eq!(aligning.alignment(), Alignment::CENTER);
        assert_eq!(aligning.width_factor(), None);
        assert_eq!(aligning.height_factor(), None);
    }

    #[test]
    fn test_with_alignment() {
        let aligning = AligningBox::with_alignment(Alignment::TOP_LEFT);
        assert_eq!(aligning.alignment(), Alignment::TOP_LEFT);
    }

    #[test]
    fn test_set_factors() {
        let mut aligning = AligningBox::new();
        aligning.set_width_factor(Some(1.5));
        aligning.set_height_factor(Some(2.0));
        assert_eq!(aligning.width_factor(), Some(1.5));
        assert_eq!(aligning.height_factor(), Some(2.0));
    }
}
