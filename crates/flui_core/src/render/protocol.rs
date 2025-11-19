//! Protocol system for unified rendering architecture
//!
//! The protocol system abstracts over different layout systems:
//! - **Box Protocol**: Standard 2D layout (constraints → size)
//! - **Sliver Protocol**: Scrollable content (sliver constraints → sliver geometry)
//!
//! This enables a single Render<A> trait to work with both protocols transparently.

use super::arity::Arity;
use std::fmt;

// Re-export from flui_types
pub use flui_types::constraints::BoxConstraints;
pub use flui_types::SliverGeometry;

/// Identifies a layout protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutProtocol {
    /// Standard 2D box layout (BoxConstraints → Size)
    Box,
    /// Scrollable/sliver layout (SliverConstraints → SliverGeometry)
    Sliver,
}

impl fmt::Display for LayoutProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Box => write!(f, "Box"),
            Self::Sliver => write!(f, "Sliver"),
        }
    }
}

/// Protocol trait - defines constraints, geometry, and context types
///
/// This trait is sealed and users cannot implement it directly.
/// Use `BoxProtocol` or `SliverProtocol` instead.
pub trait Protocol: sealed::Sealed + Send + Sync + 'static {
    /// Constraint type for this protocol
    type Constraints: Clone + fmt::Debug + Default + Send + Sync + 'static;

    /// Geometry type resulting from layout
    type Geometry: Clone + fmt::Debug + Default + Send + Sync + 'static;

    /// Layout context type
    type LayoutContext<'a, A: Arity>;

    /// Paint context type
    type PaintContext<'a, A: Arity>;

    /// Hit test context type
    type HitTestContext<'a, A: Arity>;

    /// Hit test result type
    type HitTestResult: fmt::Debug + Default + 'static;

    /// Protocol identifier
    const ID: LayoutProtocol;

    /// Human-readable protocol name
    const NAME: &'static str;
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for super::BoxProtocol {}
    impl Sealed for super::SliverProtocol {}
}

/// Trait for contexts that provide typed children access
///
/// This trait allows generic access to children with type safety.
/// All context types (BoxLayoutContext, SliverPaintContext, etc.) implement this.
pub trait HasTypedChildren<'a, A: Arity> {
    /// Get typed children accessor for this arity
    fn children(&self) -> A::Children<'a>;
}

// ============================================================================
// BOX PROTOCOL
// ============================================================================

/// Box protocol - standard 2D layout
#[derive(Debug, Clone, Copy)]
pub struct BoxProtocol;

impl Protocol for BoxProtocol {
    type Constraints = BoxConstraints;
    type Geometry = BoxGeometry;
    type LayoutContext<'a, A: Arity> = BoxLayoutContext<'a, A>;
    type PaintContext<'a, A: Arity> = BoxPaintContext<'a, A>;
    type HitTestContext<'a, A: Arity> = BoxHitTestContext<'a, A>;
    type HitTestResult = bool;

    const ID: LayoutProtocol = LayoutProtocol::Box;
    const NAME: &'static str = "Box";
}

/// Box layout geometry (computed size)
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxGeometry {
    pub size: crate::prelude::Size,
}

/// Layout context for Box protocol
pub struct BoxLayoutContext<'a, A: Arity> {
    /// Reference to the element tree for child layout operations
    pub tree: &'a crate::element::ElementTree,
    pub constraints: BoxConstraints,
    pub children: A::Children<'a>,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a, A: Arity> BoxLayoutContext<'a, A> {
    pub fn new(
        tree: &'a crate::element::ElementTree,
        constraints: BoxConstraints,
        children: A::Children<'a>,
    ) -> Self {
        Self {
            tree,
            constraints,
            children,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Layout a child element with the given constraints
    #[inline]
    pub fn layout_child(
        &self,
        child_id: std::num::NonZeroUsize,
        constraints: BoxConstraints,
    ) -> crate::prelude::Size {
        self.tree
            .layout_child(crate::ElementId::new(child_id.get()), constraints)
    }
}

impl<'a, A: Arity> HasTypedChildren<'a, A> for BoxLayoutContext<'a, A> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

/// Paint context for Box protocol
pub struct BoxPaintContext<'a, A: Arity> {
    /// Reference to the element tree for child paint operations
    pub tree: &'a crate::element::ElementTree,
    pub offset: crate::prelude::Offset,
    pub children: A::Children<'a>,
    canvas: flui_painting::Canvas,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a, A: Arity> BoxPaintContext<'a, A> {
    pub fn new(
        tree: &'a crate::element::ElementTree,
        offset: crate::prelude::Offset,
        children: A::Children<'a>,
    ) -> Self {
        Self {
            tree,
            offset,
            children,
            canvas: flui_painting::Canvas::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get mutable access to the canvas for drawing
    pub fn canvas(&mut self) -> &mut flui_painting::Canvas {
        &mut self.canvas
    }

    /// Take ownership of the canvas (used by wrapper after paint())
    pub fn take_canvas(self) -> flui_painting::Canvas {
        self.canvas
    }

    /// Paint a child element at the given offset
    #[inline]
    pub fn paint_child(
        &mut self,
        child_id: std::num::NonZeroUsize,
        offset: crate::prelude::Offset,
    ) {
        let child_canvas = self
            .tree
            .paint_child(crate::ElementId::new(child_id.get()), offset);
        self.canvas.append_canvas(child_canvas);
    }
}

impl<'a, A: Arity> HasTypedChildren<'a, A> for BoxPaintContext<'a, A> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

/// Hit test context for Box protocol
pub struct BoxHitTestContext<'a, A: Arity> {
    /// Reference to the element tree for child hit testing
    pub tree: &'a crate::element::ElementTree,
    /// Hit test position in local coordinates
    pub position: crate::prelude::Offset,
    /// Element size from layout
    pub size: crate::prelude::Size,
    /// Element ID being tested
    pub element_id: crate::element::ElementId,
    /// Typed children accessor
    pub children: A::Children<'a>,
}

impl<'a, A: Arity> BoxHitTestContext<'a, A> {
    pub fn new(
        tree: &'a crate::element::ElementTree,
        position: crate::prelude::Offset,
        size: crate::prelude::Size,
        element_id: crate::element::ElementId,
        children: A::Children<'a>,
    ) -> Self {
        Self {
            tree,
            position,
            size,
            element_id,
            children,
        }
    }

    /// Create a new context with a different position (for coordinate transformations)
    pub fn with_position(&self, position: crate::prelude::Offset) -> Self {
        Self {
            tree: self.tree,
            position,
            size: self.size,
            element_id: self.element_id,
            children: self.children,
        }
    }
}

impl<'a, A: Arity> HasTypedChildren<'a, A> for BoxHitTestContext<'a, A> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

// ============================================================================
// SLIVER PROTOCOL
// ============================================================================

/// Sliver protocol - scrollable/viewport-aware layout
#[derive(Debug, Clone, Copy)]
pub struct SliverProtocol;

impl Protocol for SliverProtocol {
    type Constraints = SliverConstraints;
    type Geometry = SliverGeometry;
    type LayoutContext<'a, A: Arity> = SliverLayoutContext<'a, A>;
    type PaintContext<'a, A: Arity> = SliverPaintContext<'a, A>;
    type HitTestContext<'a, A: Arity> = SliverHitTestContext<'a, A>;
    type HitTestResult = bool;

    const ID: LayoutProtocol = LayoutProtocol::Sliver;
    const NAME: &'static str = "Sliver";
}

/// Sliver layout constraints (scrollable content)
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SliverConstraints {
    pub scroll_offset: f32,
    pub remaining_paint_extent: f32,
    pub remaining_cache_extent: f32,
}

/// Layout context for Sliver protocol
#[derive(Debug)]
pub struct SliverLayoutContext<'a, A: Arity> {
    pub constraints: SliverConstraints,
    pub children: A::Children<'a>,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a, A: Arity> SliverLayoutContext<'a, A> {
    pub fn new(constraints: SliverConstraints, children: A::Children<'a>) -> Self {
        Self {
            constraints,
            children,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<'a, A: Arity> HasTypedChildren<'a, A> for SliverLayoutContext<'a, A> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

/// Paint context for Sliver protocol
pub struct SliverPaintContext<'a, A: Arity> {
    pub offset: crate::prelude::Offset,
    pub children: A::Children<'a>,
    canvas: flui_painting::Canvas,
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a, A: Arity> SliverPaintContext<'a, A> {
    pub fn new(offset: crate::prelude::Offset, children: A::Children<'a>) -> Self {
        Self {
            offset,
            children,
            canvas: flui_painting::Canvas::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get mutable access to the canvas for drawing
    pub fn canvas(&mut self) -> &mut flui_painting::Canvas {
        &mut self.canvas
    }

    /// Take ownership of the canvas (used by wrapper after paint())
    pub fn take_canvas(self) -> flui_painting::Canvas {
        self.canvas
    }
}

impl<'a, A: Arity> HasTypedChildren<'a, A> for SliverPaintContext<'a, A> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

/// Hit test context for Sliver protocol
pub struct SliverHitTestContext<'a, A: Arity> {
    /// Reference to the element tree for child hit testing
    pub tree: &'a crate::element::ElementTree,
    /// Position along main axis (scroll direction)
    pub main_axis_position: f32,
    /// Position along cross axis (perpendicular to scroll)
    pub cross_axis_position: f32,
    /// Sliver geometry from layout
    pub geometry: SliverGeometry,
    /// Current scroll offset
    pub scroll_offset: f32,
    /// Element ID being tested
    pub element_id: crate::element::ElementId,
    /// Typed children accessor
    pub children: A::Children<'a>,
}

impl<'a, A: Arity> SliverHitTestContext<'a, A> {
    pub fn new(
        tree: &'a crate::element::ElementTree,
        main_axis_position: f32,
        cross_axis_position: f32,
        geometry: SliverGeometry,
        scroll_offset: f32,
        element_id: crate::element::ElementId,
        children: A::Children<'a>,
    ) -> Self {
        Self {
            tree,
            main_axis_position,
            cross_axis_position,
            geometry,
            scroll_offset,
            element_id,
            children,
        }
    }

    /// Check if hit position is within visible region
    pub fn is_visible(&self) -> bool {
        self.main_axis_position >= 0.0 && self.main_axis_position < self.geometry.paint_extent
    }

    /// Get local position as Offset
    pub fn local_position(&self) -> crate::prelude::Offset {
        crate::prelude::Offset::new(self.cross_axis_position, self.main_axis_position)
    }
}

impl<'a, A: Arity> HasTypedChildren<'a, A> for SliverHitTestContext<'a, A> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_protocol_display() {
        assert_eq!(format!("{}", LayoutProtocol::Box), "Box");
        assert_eq!(format!("{}", LayoutProtocol::Sliver), "Sliver");
    }

    #[test]
    fn test_box_protocol_constants() {
        assert_eq!(BoxProtocol::ID, LayoutProtocol::Box);
        assert_eq!(BoxProtocol::NAME, "Box");
    }

    #[test]
    fn test_sliver_protocol_constants() {
        assert_eq!(SliverProtocol::ID, LayoutProtocol::Sliver);
        assert_eq!(SliverProtocol::NAME, "Sliver");
    }

    #[test]
    fn test_box_constraints_tight() {
        let size = crate::prelude::Size::new(100.0, 50.0);
        let constraints = BoxConstraints::tight(size);

        assert_eq!(constraints.min_width, 100.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.min_height, 50.0);
        assert_eq!(constraints.max_height, 50.0);
    }

    #[test]
    fn test_box_constraints_loose() {
        let constraints = BoxConstraints::loose();

        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, f32::INFINITY);
        assert_eq!(constraints.min_height, 0.0);
        assert_eq!(constraints.max_height, f32::INFINITY);
    }

    #[test]
    fn test_box_constraints_constrain() {
        let constraints = BoxConstraints {
            min_width: 50.0,
            max_width: 200.0,
            min_height: 30.0,
            max_height: 150.0,
        };

        let size1 = crate::prelude::Size::new(100.0, 80.0);
        assert_eq!(constraints.constrain(size1), size1);

        let size2 = crate::prelude::Size::new(10.0, 80.0);
        assert_eq!(
            constraints.constrain(size2),
            crate::prelude::Size::new(50.0, 80.0)
        );

        let size3 = crate::prelude::Size::new(300.0, 200.0);
        assert_eq!(
            constraints.constrain(size3),
            crate::prelude::Size::new(200.0, 150.0)
        );
    }
}
