//! Shifted container for custom child positioning

use flui_tree::arity::{Arity, ChildrenStorage, Exact};
use flui_types::Offset;

use crate::containers::Single;
use crate::protocol::{BoxProtocol, Protocol};

/// Container for single child with custom offset positioning
///
/// `Shifted<P, A>` is used for render objects that position their child at a custom
/// offset. It stores the child, geometry, and the child's offset.
///
/// By default, Shifted uses `Exact<1>` arity to ensure exactly one child is present.
///
/// Uses ambassador to delegate ChildrenStorage trait to internal Single container.
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
/// - `A`: The arity constraint (Exact<1> by default for exactly one child)
///
/// # Examples
///
/// ```ignore
/// use flui_rendering::containers::Shifted;
/// use flui_rendering::protocol::BoxProtocol;
///
/// struct RenderPadding {
///     shifted: Shifted<BoxProtocol>,
///     padding: EdgeInsets,
/// }
///
/// impl RenderBox for RenderPadding {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///         // Layout child with deflated constraints
///         let inner = constraints.deflate(self.padding);
///         let child_size = if let Some(child) = self.shifted.child_mut() {
///             child.perform_layout(inner)
///         } else {
///             Size::ZERO
///         };
///
///         // Compute parent size
///         let size = Size {
///             width: child_size.width + self.padding.horizontal(),
///             height: child_size.height + self.padding.vertical(),
///         };
///
///         // Set child offset
///         self.shifted.set_offset(Offset {
///             dx: self.padding.left,
///             dy: self.padding.top,
///         });
///
///         self.shifted.set_geometry(size);
///         size
///     }
/// }
/// ```

pub struct Shifted<P: Protocol, A: Arity = Exact<1>> {
    child: Single<P, A>,
    geometry: P::Geometry,
    offset: Offset,
}

// Manual Debug impl since P::Object doesn't require Debug
impl<P: Protocol, A: Arity> std::fmt::Debug for Shifted<P, A>
where
    P::Geometry: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shifted")
            .field("geometry", &self.geometry)
            .field("offset", &self.offset)
            .field("has_child", &self.child.has_child())
            .finish()
    }
}

impl<P: Protocol, A: Arity> Shifted<P, A> {
    /// Creates a new shifted container with default geometry and zero offset
    pub fn new() -> Self {
        Self {
            child: Single::new(),
            geometry: P::default_geometry(),
            offset: Offset::ZERO,
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
}

impl<P: Protocol, A: Arity> Default for Shifted<P, A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for Box shifted container
pub type ShiftedBox = Shifted<BoxProtocol>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let shifted: ShiftedBox = ShiftedBox::new();
        assert!(!shifted.has_child());
        assert_eq!(*shifted.offset(), Offset::ZERO);
    }

    #[test]
    fn test_set_offset() {
        let mut shifted: ShiftedBox = ShiftedBox::new();
        shifted.set_offset(Offset::new(10.0, 20.0));
        assert_eq!(shifted.offset().dx, 10.0);
        assert_eq!(shifted.offset().dy, 20.0);
    }
}
