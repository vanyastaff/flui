//! Object-safe accessor protocol for InheritedElement.
//!
//! [`BuildContext::depend_on_inherited`](crate::BuildContext::depend_on_inherited) walks the element tree, finds
//! the nearest matching `InheritedElement<V>`, and needs to (a) read the
//! view as `&dyn Any` so the caller's downcast can succeed, and
//! (b) record the caller in the inherited element's dependent map.
//!
//! Because `BuildContext` is parameter-free at the trait surface
//! (object-safe `&dyn BuildContext`), it can't name `V`. The retained
//! element side, however, is parametric: each `InheritedElement<V>` is
//! a distinct concrete type at `V`-instantiation time. The bridge is a
//! small object-safe trait that exposes the two operations
//! `BuildContext` needs without leaking `V` into the trait surface.
//!
//! Flutter parity: `framework.dart:5081`
//! `dependOnInheritedWidgetOfExactType<T>` resolves the ancestor via
//! `_inheritedElements` lookup then invokes
//! `inheritedElement.updateDependencies(self, null)` — same shape.

use flui_foundation::ElementId;

/// Object-safe view of an `InheritedElement<V>` exposed to
/// `BuildContext` so the dependency-injection machinery can record
/// dependents and read the inherited view as `&dyn Any` without
/// naming the concrete `V`.
///
/// Implemented by `Element<V, Single, InheritedBehavior<V>>` (see
/// [`Element`](crate::element::Element)) in
/// `crates/flui-view/src/element/unified.rs`. The default `ElementBase`
/// hooks ([`ElementBase::as_inherited`] / [`as_inherited_mut`]) return
/// `None` for every other element type.
///
/// [`ElementBase::as_inherited`]: crate::view::ElementBase::as_inherited
/// [`as_inherited_mut`]: crate::view::ElementBase::as_inherited_mut
pub trait InheritedElementAccess {
    /// Borrow the inherited view as `&dyn Any` so the caller can
    /// downcast to the concrete `V` (the `InheritedView` type).
    ///
    /// This is the typed payload Flutter's `InheritedElement.widget`
    /// returns to the dependent's `BuildContext`.
    fn view_as_any(&self) -> &dyn std::any::Any;

    /// Register a dependent element with this `InheritedElement`.
    ///
    /// `depth` is the dependent's depth in the element tree, threaded
    /// through so a later
    /// [`InheritedBehavior::on_view_updated`](crate::element::InheritedBehavior)
    /// can call `ElementOwner::schedule_build_for` with
    /// [`RebuildReason::DependencyChange`](crate::RebuildReason::DependencyChange)
    /// without an extra tree traversal.
    ///
    /// Idempotent: re-registering the same id overwrites its depth
    /// (HashMap keyed by id) so reconciliation-driven depth changes are
    /// captured without leaving stale entries.
    fn record_dependent(&mut self, dependent: ElementId, depth: usize);

    /// Release a dependent during deactivate or unmount.
    ///
    /// The reverse ownership index supplies the exact provider ids, so
    /// lifecycle cleanup never scans the tree or waits for a later
    /// notification to prune stale entries.
    fn remove_dependent(&mut self, dependent: ElementId);
}
