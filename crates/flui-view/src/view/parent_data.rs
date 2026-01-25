//! ParentDataView - Views that configure parent data on RenderObjects.
//!
//! ParentDataViews are special ProxyViews that apply configuration
//! data to child RenderObjects. The data is stored on the child's
//! `parentData` field and used by the parent RenderObject during layout.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `ParentDataWidget<T>` which is used for:
//! - `Positioned` - sets position in Stack
//! - `Flexible`/`Expanded` - sets flex properties in Flex
//! - `TableCell` - sets table cell properties
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_view::{ParentDataView, View};
//!
//! /// Data for positioning a child in a Stack
//! #[derive(Clone, Default)]
//! struct StackParentData {
//!     left: Option<f64>,
//!     top: Option<f64>,
//!     right: Option<f64>,
//!     bottom: Option<f64>,
//! }
//!
//! /// Positioned widget for Stack
//! #[derive(Clone)]
//! struct Positioned {
//!     left: Option<f64>,
//!     top: Option<f64>,
//!     right: Option<f64>,
//!     bottom: Option<f64>,
//!     child: Box<dyn View>,
//! }
//!
//! impl ParentDataView for Positioned {
//!     type ParentData = StackParentData;
//!
//!     fn child(&self) -> &dyn View {
//!         &*self.child
//!     }
//!
//!     fn create_parent_data(&self) -> Self::ParentData {
//!         StackParentData {
//!             left: self.left,
//!             top: self.top,
//!             right: self.right,
//!             bottom: self.bottom,
//!         }
//!     }
//! }
//! impl_parent_data_view!(Positioned);
//! ```

use super::view::{ElementBase, View};
use crate::element::Lifecycle;
use flui_foundation::ElementId;
use std::any::TypeId;

/// Marker trait for types that can be used as ParentData.
///
/// ParentData is attached to a child RenderObject and used by
/// the parent during layout to position/configure the child.
pub trait ParentData: Clone + Default + Send + Sync + 'static {}

// Implement for common types
impl ParentData for () {}

/// A View that provides parent data to its child RenderObject.
///
/// ParentDataViews sit between a parent RenderObject and its children,
/// configuring how the parent should lay out each child.
///
/// # Type Parameter
///
/// - `ParentData`: The type of data this View provides to the parent.
///   Must implement `Clone + Default + Send + Sync + 'static`.
///
/// # How It Works
///
/// 1. ParentDataView wraps a child View
/// 2. When the child creates a RenderObject, the ParentDataElement
///    attaches the parent data to it
/// 3. The parent RenderObject reads this data during layout
///
/// # Example Widgets Using ParentData
///
/// | Widget | Parent | ParentData |
/// |--------|--------|------------|
/// | Positioned | Stack | left, top, right, bottom, width, height |
/// | Flexible | Flex | flex, fit |
/// | TableCell | Table | row, column span |
pub trait ParentDataView: Send + Sync + 'static + Sized {
    /// The type of parent data this View provides.
    type ParentData: ParentData;

    /// Get the child View.
    fn child(&self) -> &dyn View;

    /// Create the parent data to attach to the child's RenderObject.
    fn create_parent_data(&self) -> Self::ParentData;

    /// Apply parent data changes to an existing parent data instance.
    ///
    /// This is called when the View updates. The default implementation
    /// replaces the entire parent data.
    fn apply_parent_data(&self, parent_data: &mut Self::ParentData) {
        *parent_data = self.create_parent_data();
    }
}

/// Implement View for a ParentDataView type.
///
/// This macro creates the View implementation for a ParentDataView type.
///
/// ```rust,ignore
/// impl ParentDataView for Positioned {
///     type ParentData = StackParentData;
///     fn child(&self) -> &dyn View { &*self.child }
///     fn create_parent_data(&self) -> Self::ParentData { ... }
/// }
/// impl_parent_data_view!(Positioned);
/// ```
#[macro_export]
macro_rules! impl_parent_data_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                Box::new($crate::ParentDataElement::new(self))
            }
        }
    };
}

// ============================================================================
// ParentDataElement
// ============================================================================

/// Element for ParentDataViews.
///
/// Manages the lifecycle of a ParentDataView and applies parent data
/// to the child's RenderObject.
pub struct ParentDataElement<V: ParentDataView> {
    /// The current View configuration.
    view: V,
    /// Current lifecycle state.
    lifecycle: Lifecycle,
    /// Depth in tree.
    depth: usize,
    /// Child element.
    child: Option<Box<dyn ElementBase>>,
    /// Whether we need to rebuild.
    dirty: bool,
    /// Cached parent data.
    parent_data: Option<V::ParentData>,
}

impl<V: ParentDataView> ParentDataElement<V>
where
    V: Clone,
{
    /// Create a new ParentDataElement for the given View.
    pub fn new(view: &V) -> Self {
        Self {
            view: view.clone(),
            lifecycle: Lifecycle::Initial,
            depth: 0,
            child: None,
            dirty: true,
            parent_data: None,
        }
    }

    /// Get a reference to the child element.
    pub fn child(&self) -> Option<&dyn ElementBase> {
        self.child.as_deref()
    }

    /// Get a mutable reference to the child element.
    pub fn child_mut(&mut self) -> Option<&mut dyn ElementBase> {
        self.child.as_deref_mut()
    }

    /// Get the current parent data.
    pub fn parent_data(&self) -> Option<&V::ParentData> {
        self.parent_data.as_ref()
    }

    /// Get the parent data type ID (for debug purposes).
    pub fn parent_data_type_id(&self) -> TypeId {
        TypeId::of::<V::ParentData>()
    }

    /// Apply parent data to the child's RenderObject.
    ///
    /// This walks down the element tree to find the first RenderElement
    /// and applies the parent data to its RenderObject.
    fn apply_parent_data_to_child(&mut self) {
        let data = self.view.create_parent_data();
        self.parent_data = Some(data);
        // In a full implementation, we would find the child's RenderObject
        // and set its parentData field
    }
}

impl<V: ParentDataView + Clone> std::fmt::Debug for ParentDataElement<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParentDataElement")
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("dirty", &self.dirty)
            .field("has_child", &self.child.is_some())
            .field("parent_data_type", &std::any::type_name::<V::ParentData>())
            .finish_non_exhaustive()
    }
}

impl<V: ParentDataView + Clone> ElementBase for ParentDataElement<V> {
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<V>()
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn update(&mut self, new_view: &dyn View) {
        // Use View::as_any() for safe downcasting
        if let Some(v) = new_view.as_any().downcast_ref::<V>() {
            self.view = v.clone();
            self.dirty = true;
            // Apply updated parent data
            self.apply_parent_data_to_child();
        }
    }

    fn mark_needs_build(&mut self) {
        self.dirty = true;
    }

    fn perform_build(&mut self) {
        if !self.dirty || !self.lifecycle.can_build() {
            return;
        }

        // Apply parent data when building
        self.apply_parent_data_to_child();
        self.dirty = false;
    }

    fn mount(&mut self, _parent: Option<ElementId>, _slot: usize) {
        self.lifecycle = Lifecycle::Active;
        self.dirty = true;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
        if let Some(child) = &mut self.child {
            child.deactivate();
        }
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
        if let Some(child) = &mut self.child {
            child.activate();
        }
    }

    fn unmount(&mut self) {
        self.lifecycle = Lifecycle::Defunct;
        if let Some(child) = &mut self.child {
            child.unmount();
        }
        self.child = None;
        self.parent_data = None;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        // In a full implementation, we'd track child ElementIds
        let _ = visitor;
    }

    fn depth(&self) -> usize {
        self.depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test parent data type
    #[derive(Clone, Default)]
    struct TestParentData {
        flex: f64,
        fit: bool,
    }

    impl ParentData for TestParentData {}

    // A dummy child view
    #[derive(Clone)]
    struct DummyChild;

    impl View for DummyChild {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(DummyChildElement)
        }
    }

    struct DummyChildElement;

    impl ElementBase for DummyChildElement {
        fn view_type_id(&self) -> TypeId {
            TypeId::of::<DummyChild>()
        }
        fn lifecycle(&self) -> Lifecycle {
            Lifecycle::Active
        }
        fn update(&mut self, _: &dyn View) {}
        fn mark_needs_build(&mut self) {}
        fn perform_build(&mut self) {}
        fn mount(&mut self, _: Option<ElementId>, _: usize) {}
        fn deactivate(&mut self) {}
        fn activate(&mut self) {}
        fn unmount(&mut self) {}
        fn visit_children(&self, _: &mut dyn FnMut(ElementId)) {}
        fn depth(&self) -> usize {
            0
        }
    }

    /// A test parent data view (like Flexible)
    #[derive(Clone)]
    struct TestFlexible {
        flex: f64,
        fit: bool,
        child: DummyChild,
    }

    impl ParentDataView for TestFlexible {
        type ParentData = TestParentData;

        fn child(&self) -> &dyn View {
            &self.child
        }

        fn create_parent_data(&self) -> Self::ParentData {
            TestParentData {
                flex: self.flex,
                fit: self.fit,
            }
        }
    }

    impl View for TestFlexible {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(ParentDataElement::new(self))
        }
    }

    #[test]
    fn test_parent_data_element_creation() {
        let view = TestFlexible {
            flex: 2.0,
            fit: true,
            child: DummyChild,
        };

        let element = ParentDataElement::new(&view);
        assert_eq!(element.lifecycle(), Lifecycle::Initial);
        assert!(element.parent_data().is_none());
    }

    #[test]
    fn test_parent_data_element_mount_and_build() {
        let view = TestFlexible {
            flex: 2.0,
            fit: true,
            child: DummyChild,
        };

        let mut element = ParentDataElement::new(&view);
        element.mount(None, 0);
        element.perform_build();

        assert_eq!(element.lifecycle(), Lifecycle::Active);
        assert!(element.parent_data().is_some());

        let data = element.parent_data().unwrap();
        assert!((data.flex - 2.0).abs() < f64::EPSILON);
        assert!(data.fit);
    }

    #[test]
    fn test_parent_data_element_update() {
        let view = TestFlexible {
            flex: 1.0,
            fit: false,
            child: DummyChild,
        };

        let mut element = ParentDataElement::new(&view);
        element.mount(None, 0);
        element.perform_build();

        let new_view = TestFlexible {
            flex: 3.0,
            fit: true,
            child: DummyChild,
        };

        element.update(&new_view);

        let data = element.parent_data().unwrap();
        assert!((data.flex - 3.0).abs() < f64::EPSILON);
        assert!(data.fit);
    }

    #[test]
    fn test_parent_data_type_id() {
        let view = TestFlexible {
            flex: 1.0,
            fit: false,
            child: DummyChild,
        };

        let element = ParentDataElement::new(&view);
        assert_eq!(
            element.parent_data_type_id(),
            TypeId::of::<TestParentData>()
        );
    }
}
