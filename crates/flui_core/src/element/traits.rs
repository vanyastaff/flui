//! Element trait - Core trait for all element types
//!
//! The Element trait defines the lifecycle and behavior of elements in the widget tree.
//! Elements are mutable state holders that persist across rebuilds.
//!
//! # Design Pattern: Two-Trait Approach
//!
//! Flui uses a two-trait pattern for elements (similar to Widget/DynWidget):
//! - **DynElement** - Object-safe base trait for `Box<dyn DynElement>` collections
//! - **Element** - Extended trait with associated types for zero-cost concrete usage
//!
//! This allows:
//! - Zero-cost widget updates for concrete element types
//! - Type-safe widget-element relationships via associated types
//! - Heterogeneous element storage in the element tree



use super::dyn_element::DynElement;

/// Extended Element trait with associated types
///
/// This trait extends DynElement with associated types for zero-cost concrete operations.
/// All types implementing Element automatically implement DynElement via a blanket impl.
///
/// # Design Pattern
///
/// Similar to Widget/DynWidget split:
/// - **DynElement** (object-safe) → `Box<dyn DynElement>` for heterogeneous storage
/// - **Element** (with associated types) → Zero-cost for concrete types
///
/// # Associated Types
///
/// - `Widget` - The concrete widget type this element holds
///
/// # Enhanced Lifecycle (Phase 3)
///
/// 1. **Initial**: Element created
/// 2. **Mount**: Element inserted into tree → Active
/// 3. **Update**: Widget configuration changes
/// 4. **Rebuild**: Element rebuilds its subtree
/// 5. **Deactivate**: Element removed from tree → Inactive (might be reactivated)
/// 6. **Activate**: Element reinserted into tree → Active (from Inactive)
/// 7. **Unmount**: Element permanently removed → Defunct
///
/// # Architecture
///
/// Elements form the middle layer of the three-tree architecture:
/// - **Widget** → Immutable configuration (recreated each rebuild)
/// - **Element** → Mutable state holder (persists across rebuilds)
/// - **RenderObject** → Layout and painting (optional, for render widgets)
///
/// # Example
///
/// ```rust,ignore
/// impl<W: StatelessWidget> Element for ComponentElement<W> {
///     type Widget = W;
///
///     fn update(&mut self, new_widget: W) {
///         self.widget = new_widget;  // Zero-cost! No downcast needed!
///         self.mark_dirty();
///     }
///
///     fn widget(&self) -> &W {
///         &self.widget
///     }
/// }
/// ```
pub trait Element: DynElement + Sized {
    /// The concrete widget type this element holds
    ///
    /// This associated type enables zero-cost widget updates and type-safe
    /// widget-element relationships.
    type Widget: crate::Widget;

    /// Update this element with a new widget configuration (zero-cost)
    ///
    /// This is the type-safe version of `DynElement::update_any()`.
    /// Use this for concrete element types to avoid runtime downcasts.
    ///
    /// # Parameters
    /// - `new_widget`: The new widget configuration (concrete type)
    fn update(&mut self, new_widget: Self::Widget);

    /// Get reference to the widget this element holds (zero-cost)
    ///
    /// # Returns
    /// Reference to the concrete widget type
    fn widget(&self) -> &Self::Widget;
}

// OldElement trait has been removed. All element types now use the new Element trait
// with associated types (Element + DynElement pattern).

// Note: Element trait is not dyn-compatible (requires Sized).
// Downcasting is handled by DynElement instead (see dyn_element.rs).
