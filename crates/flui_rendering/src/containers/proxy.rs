//! Proxy container for pass-through objects

use ambassador::Delegate;
use flui_tree::arity::{Arity, ChildrenStorage, Exact};
use crate::containers::delegation::{ambassador_impl_ChildrenStorage, DelegatableChildrenStorage as ChildrenStorage};

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

#[derive(Delegate)]
#[delegate(ChildrenStorage<Box<P::Object>>, target = "child")]
pub struct Proxy<P: Protocol, A: Arity = Exact<1>> {
    child: Single<P, A>,
    geometry: P::Geometry,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let proxy: ProxyBox = ProxyBox::new();
        assert!(!proxy.has_child());
    }
}
