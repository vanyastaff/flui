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
//!
//!     fn apply_parent_data(
//!         &self,
//!         data: &mut Self::ParentData,
//!     ) -> flui_rendering::RenderUpdateImpact {
//!         // Compare and mutate only `left`/`top`/`right`/`bottom`.
//!         # flui_rendering::RenderUpdateImpact::NONE
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
/// This trait is named `ParentDataConfig` rather than `ParentData` so it
/// does not collide with `flui_rendering::ParentData` (the actual
/// render-object storage trait carrying `Any` + downcasting). The two
/// traits serve different concerns:
///
/// - `flui_view::ParentDataConfig` (this trait): marker for the
///   widget-side **configuration value**, what a `ParentDataView`
///   supplies (Flutter's `ParentDataWidget.applyParentData` payload).
/// - `flui_rendering::ParentData`: the render-side **storage trait**
///   that a `RenderObject` carries.
///
/// A same-name trait would force every workspace consumer importing both
/// crates to fully-qualify or alias one of them. The distinct name
/// matches Flutter's `ParentDataWidget` naming: the widget **configures**
/// the parent-data; it is not itself the parent-data.
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
pub trait ParentDataView: Clone + 'static + Sized {
    /// The type of parent data this View provides.
    ///
    /// The trait bound is `ParentDataConfig` (rather than `ParentData`,
    /// which was renamed to disambiguate from `flui_rendering::ParentData`).
    /// The associated-type name `ParentData` is kept as-is because no
    /// cross-crate collision can occur on associated-type names.
    type ParentData: ParentDataConfig;

    /// Get the child View.
    fn child(&self) -> &dyn View;

    /// Create the parent data to attach to the child's RenderObject.
    fn create_parent_data(&self) -> Self::ParentData;

    /// Apply parent data changes to an existing parent data instance.
    ///
    /// This is called when the View updates. Implementations mutate only the
    /// configuration-owned fields, preserving layout-owned offsets and
    /// container metadata, and report the required invalidation.
    fn apply_parent_data(
        &self,
        parent_data: &mut Self::ParentData,
    ) -> flui_rendering::RenderUpdateImpact;

    /// Short name for diagnostics (`"Expanded"`, `"Positioned"`).
    ///
    /// Default: the fully-qualified Rust type name. Catalog widgets override
    /// with a stable short label so attach-seam failures read like Flutter's
    /// `Incorrect use of ParentDataWidget` messages.
    fn debug_type_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }

    /// Human description of legal render ancestors for this view.
    ///
    /// Shown when the attach seam rejects a `ParentDataView` whose storage
    /// type does not match the nearest render parent's
    /// [`child_parent_data_type_id`](flui_rendering::RenderObject::child_parent_data_type_id).
    fn typical_ancestor_description(&self) -> &'static str {
        "a render parent whose children use this parent-data type"
    }

    /// Fully-qualified name of [`Self::ParentData`] for diagnostics.
    fn parent_data_type_name(&self) -> &'static str {
        core::any::type_name::<Self::ParentData>()
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
///     fn apply_parent_data(&self, data: &mut Self::ParentData) -> RenderUpdateImpact { ... }
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
