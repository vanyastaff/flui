// PORT-TARGET: flui-widgets::Flexible, flui-widgets::Positioned
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

use super::view::View;

/// Marker trait for types that can be used as parent-data configuration.
///
/// Implementing types describe the per-child configuration that a
/// `ParentDataView` widget supplies to its parent RenderObject (e.g.
/// `Flex`'s `flex` factor, `Stack`'s `top` / `left`). The parent reads
/// the configuration during layout to position / size the child.
///
/// # Why the name
///
/// Cycle 4 R-11 renamed this trait from `ParentData` to
/// `ParentDataConfig` so it no longer collides with
/// `flui_rendering::ParentData` (the actual render-object storage
/// trait carrying `Any` + downcasting). The two traits serve
/// different concerns:
///
/// - `flui_view::ParentDataConfig` (this trait): marker for the
///   widget-side **configuration value**, what a `ParentDataView`
///   supplies (Flutter's `ParentDataWidget.applyParentData` payload).
/// - `flui_rendering::ParentData`: the render-side **storage trait**
///   that a `RenderObject` carries.
///
/// Same-name trait collision pre-cycle forced every workspace
/// consumer importing both crates to fully-qualify or alias one of
/// them. The rename matches Flutter's `ParentDataWidget` naming:
/// the widget **configures** the parent-data; it is not itself the
/// parent-data.
pub trait ParentDataConfig: flui_rendering::parent_data::ParentData + Clone + Default {}

/// Blanket: any concrete `flui-rendering` parent-data type
/// (`FlexParentData`, `StackParentData`, …) that is `Clone + Default` is usable
/// as a [`ParentDataView::ParentData`], so `create_parent_data()` returns the
/// exact type written onto the render node — there is no widget-side
/// parent-data type to convert from. (`ParentData` already requires
/// `Send + Sync + 'static` for arena storage.)
impl<T: flui_rendering::parent_data::ParentData + Clone + Default> ParentDataConfig for T {}

/// A View that provides parent data to its child RenderObject.
///
/// ParentDataViews sit between a parent RenderObject and its children,
/// configuring how the parent should lay out each child.
///
/// # Type Parameter
///
/// - `ParentData`: The type of data this View provides to the parent. Must
///   implement `Clone + Default + Send + Sync + 'static`.
///
/// # How It Works
///
/// 1. ParentDataView wraps a child View
/// 2. When the child creates a RenderObject, the ParentDataElement attaches the
///    parent data to it
/// 3. The parent RenderObject reads this data during layout
///
/// # Example Widgets Using ParentData
///
/// | Widget | Parent | ParentData |
/// |--------|--------|------------|
/// | Positioned | Stack | left, top, right, bottom, width, height |
/// | Flexible | Flex | flex, fit |
/// | TableCell | Table | row, column span |
pub trait ParentDataView: Clone + Send + Sync + 'static + Sized {
    /// The type of parent data this View provides.
    ///
    /// Cycle 4 R-11: bound is `ParentDataConfig` (was `ParentData`,
    /// renamed to disambiguate from `flui_rendering::ParentData`).
    /// The associated-type name `ParentData` is kept because no
    /// cross-crate collision can occur on associated-type names.
    type ParentData: ParentDataConfig;

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
            fn create_element(&self) -> $crate::element::ElementKind {
                $crate::element::ElementKind::parent_data(self)
            }
        }
    };
}

// The element for `ParentDataView`s is the unified
// `Element<V, Single, ParentDataBehavior>` — see the `ParentDataElement<V>`
// type alias in `element/mod.rs`. The behavior is a transparent proxy
// (`ParentDataBehavior`), and the parent-data it contributes is written onto
// the child render node at the `ElementTree` insert/update seams
// (`apply_ancestor_parent_data`). The former bespoke, owner-blind element with
// its stubbed `apply_parent_data_to_child` was deleted in the 2026-06 cutover.

#[cfg(test)]
mod tests {
    use flui_objects::RenderSizedBox;
    use flui_rendering::protocol::BoxProtocol;

    use super::*;

    // A real render-side parent-data type — satisfies the `ParentDataConfig`
    // blanket via `flui_rendering::parent_data::ParentData`, so it is the exact
    // type written onto the child render node. (No widget-side conversion.)
    #[derive(Debug, Clone, Default)]
    struct TestParentData {
        flex: f64,
        fit: bool,
    }

    impl flui_rendering::parent_data::ParentData for TestParentData {}

    // A dummy child view
    #[derive(Clone)]
    struct DummyChild;

    impl crate::RenderView for DummyChild {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(&self) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}
    }

    impl View for DummyChild {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    /// A test parent data view (like `Flexible`). Drives the unified
    /// `ParentDataElement<V>` (`Element<V, Single, ParentDataBehavior>`) via the
    /// `impl_parent_data_view!` macro — no bespoke element type.
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

    impl_parent_data_view!(TestFlexible);

    /// The macro-built View resolves to the unified `ParentDataBehavior`
    /// element, whose `parent_data_config()` surfaces the view's configured
    /// parent data (the seam `ElementTree` writes onto the child render node).
    #[test]
    fn behavior_surfaces_configured_parent_data() {
        let view = TestFlexible {
            flex: 2.0,
            fit: true,
            child: DummyChild,
        };

        let element = view.create_element();
        let config = element
            .element()
            .parent_data_config()
            .expect("ParentDataBehavior must surface a parent-data config");
        let data = config
            .as_any()
            .downcast_ref::<TestParentData>() // PORT-CHECK-OK-DOWNCAST: test asserts the concrete config type round-trips
            .expect("the surfaced config is the view's concrete ParentData type");
        assert!((data.flex - 2.0).abs() < f64::EPSILON);
        assert!(data.fit);
    }

    /// E3 regression: a ParentData element scheduled in the tree actually
    /// reconciles its wrapped child through `build_scope`.
    ///
    /// `ParentDataBehavior` is a proxy-style behavior whose `build_into_views`
    /// returns the wrapped child for the id-reconciler. `build_scope`'s dirty
    /// guard reads `is_dirty()`; a freshly-mounted element reports dirty, so
    /// the guard must let it build and hand its child off.
    #[test]
    fn regression_parent_data_reconciles_child_through_build_scope() {
        let view = TestFlexible {
            flex: 2.0,
            fit: true,
            child: DummyChild,
        };

        let mut tree = crate::ElementTree::new();
        let mut owner = crate::BuildOwner::new();
        let root = tree.mount_root(&view, &mut owner.element_owner_mut());

        // Mount leaves the element dirty; the guard must observe that.
        assert!(
            tree.get(root).unwrap().element().is_dirty(),
            "a freshly-mounted ParentData element reports is_dirty() == true",
        );

        owner.schedule_build_for(root, 0);
        owner.build_scope(&mut tree);

        let child_ids = tree.get(root).unwrap().child_ids().to_vec();
        assert_eq!(
            child_ids.len(),
            1,
            "build_scope must let the dirty ParentData element reconcile its wrapped child",
        );
        assert!(
            tree.get(child_ids[0]).is_some(),
            "the reconciled child resolves in the slab",
        );
        // The build cleared the flag, so a no-op rebuild won't re-fire.
        assert!(
            !tree.get(root).unwrap().element().is_dirty(),
            "is_dirty() is false after the build hands the child off",
        );
    }
}
