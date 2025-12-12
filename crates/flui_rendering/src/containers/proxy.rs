//! Proxy container for pass-through objects

use flui_tree::arity::{Arity, Exact};

use crate::containers::Single;
use crate::protocol::{BoxProtocol, Protocol};

/// Container for single child where parent geometry equals child geometry
///
/// `Proxy<P, A>` is used for render objects that pass their size through to their child.
/// It stores both the child and the geometry (Size or SliverGeometry).
///
/// By default, Proxy uses `Exact<1>` arity to ensure exactly one child is present.
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
/// use flui_rendering::containers::Proxy;
/// use flui_rendering::protocol::BoxProtocol;
///
/// struct RenderOpacity {
///     proxy: Proxy<BoxProtocol>,
///     opacity: f32,
/// }
///
/// impl RenderBox for RenderOpacity {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///         let size = if let Some(child) = self.proxy.child_mut() {
///             child.perform_layout(constraints)
///         } else {
///             constraints.smallest()
///         };
///         self.proxy.set_geometry(size);
///         size
///     }
///
///     fn size(&self) -> Size {
///         *self.proxy.geometry()
///     }
/// }
/// ```

pub struct Proxy<P: Protocol, A: Arity = Exact<1>> {
    child: Single<P, A>,
    geometry: P::Geometry,
    // RenderObject state (used when P = BoxProtocol)
    depth: usize,
    attached: bool,
    needs_layout: bool,
    needs_paint: bool,
}

// Manual Debug impl since P::Object doesn't require Debug
impl<P: Protocol, A: Arity> std::fmt::Debug for Proxy<P, A>
where
    P::Geometry: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Proxy")
            .field("geometry", &self.geometry)
            .field("has_child", &self.child.has_child())
            .finish()
    }
}

impl<P: Protocol, A: Arity> Proxy<P, A> {
    /// Creates a new proxy container with default geometry
    pub fn new() -> Self {
        Self {
            child: Single::new(),
            geometry: P::default_geometry(),
            depth: 0,
            attached: false,
            needs_layout: true,
            needs_paint: true,
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
}

impl<P: Protocol, A: Arity> Default for Proxy<P, A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for Box proxy container
pub type ProxyBox = Proxy<BoxProtocol>;

// ============================================================================
// Trait Implementations for Proxy<BoxProtocol>
// ============================================================================

// Implement RenderObject
impl<A: Arity> crate::traits::RenderObject for Proxy<BoxProtocol, A> {
    fn depth(&self) -> usize {
        self.depth
    }

    fn attached(&self) -> bool {
        self.attached
    }

    fn attach(&mut self) {
        self.attached = true;
        if let Some(child) = self.child.child_mut() {
            child.attach();
        }
    }

    fn detach(&mut self) {
        if let Some(child) = self.child.child_mut() {
            child.detach();
        }
        self.attached = false;
    }

    fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
    }

    fn mark_needs_paint(&mut self) {
        self.needs_paint = true;
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// Implement RenderBox
impl<A: Arity> crate::traits::RenderBox for Proxy<BoxProtocol, A> {
    fn perform_layout(
        &mut self,
        constraints: crate::constraints::BoxConstraints,
    ) -> crate::geometry::Size {
        if let Some(child) = self.child.child_mut() {
            let size = child.perform_layout(constraints);
            self.geometry = size;
            size
        } else {
            constraints.smallest()
        }
    }

    fn size(&self) -> crate::geometry::Size {
        self.geometry
    }

    fn paint(&self, context: &mut dyn crate::traits::PaintingContext, offset: flui_types::Offset) {
        if let Some(child) = self.child.child() {
            context.paint_child(child, offset);
        }
    }

    // Other RenderBox methods use defaults
}

// Implement SingleChildRenderBox
impl<A: Arity> crate::traits::SingleChildRenderBox for Proxy<BoxProtocol, A> {
    fn child(&self) -> Option<&dyn crate::traits::RenderBox> {
        self.child.child()
    }

    fn child_mut(&mut self) -> Option<&mut dyn crate::traits::RenderBox> {
        self.child.child_mut()
    }

    // Other SingleChildRenderBox methods use defaults from the trait
}

// Implement RenderProxyBox (marker trait, uses all defaults)
impl<A: Arity> crate::traits::RenderProxyBox for Proxy<BoxProtocol, A> {
    fn size(&self) -> crate::geometry::Size {
        self.geometry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let proxy: ProxyBox = ProxyBox::new();
        assert!(!proxy.has_child());
    }
}
