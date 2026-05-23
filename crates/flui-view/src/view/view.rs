//! Base View trait - immutable UI configuration.
//!
//! Views are the declarative description of UI. They are:
//! - **Immutable**: Created fresh each build cycle
//! - **Short-lived**: Exist only for diffing, then dropped
//! - **Composable**: Build trees of nested Views
//!
//! This is equivalent to Flutter's `Widget` class.

use std::any::TypeId;

use downcast_rs::{Downcast, impl_downcast};
use dyn_clone::{DynClone, clone_trait_object};

/// Base trait for all Views.
///
/// A View is an immutable configuration for a piece of UI. Views are created
/// during the build phase and compared against previous Views to determine
/// what needs to change. Unlike Elements, Views are short-lived and recreated
/// each build cycle.
///
/// # Type Parameter
///
/// Each View type has an associated `Element` type that manages its lifecycle.
/// This association is determined at compile time, avoiding runtime type
/// checks.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{View, StatelessView, BuildContext, IntoView};
///
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///         Text::new(format!("Hello, {}!", self.name))
///     }
/// }
/// ```
///
/// # Flutter Equivalent
///
/// This trait corresponds to Flutter's `Widget` abstract class:
/// - `create_element()` → `Widget.createElement()`
/// - `can_update()` → `Widget.canUpdate()` static method
pub trait View: Downcast + DynClone + Send + Sync + 'static {
    /// Create a new Element for this View.
    ///
    /// Called once when this View first appears in the tree.
    /// The Element manages the View's lifecycle and holds any mutable state.
    ///
    /// # Returns
    ///
    /// A boxed Element that will manage this View's lifecycle.
    fn create_element(&self) -> Box<dyn ElementBase>;

    /// Get the type ID of this View for runtime type checking.
    ///
    /// Used by the framework to determine if two Views are of the same type.
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }

    /// Check if this View can update an existing Element.
    ///
    /// Returns `true` if the Element created by `old` can be updated with
    /// `self`. Two Views can update each other only when they share the
    /// same concrete type **and** the same `Key` — where "same `Key`"
    /// means both Views are keyless, or both carry keys that compare
    /// equal via [`ViewKey::key_eq`](flui_foundation::ViewKey::key_eq).
    ///
    /// The key check is what makes keyed child reconciliation work: a
    /// keyed widget moved to a new slot must NOT be absorbed by whatever
    /// same-type sibling happens to land in its old position. It also
    /// means a `UniqueKey` never matches (each instance is distinct), so
    /// it always forces a fresh element — Flutter parity.
    ///
    /// # Arguments
    ///
    /// * `old` - The previous View that created the Element
    ///
    /// # Returns
    ///
    /// `true` if the Element can be updated, `false` if it must be replaced.
    ///
    /// # Flutter Equivalent
    ///
    /// `Widget.canUpdate` (`framework.dart:4123`):
    /// `oldWidget.runtimeType == newWidget.runtimeType
    ///  && oldWidget.key == newWidget.key`.
    fn can_update(&self, old: &dyn View) -> bool {
        if self.view_type_id() != old.view_type_id() {
            return false;
        }
        match (self.key(), old.key()) {
            // Both keyless — same-type is enough.
            (None, None) => true,
            // Both keyed — must compare equal.
            (Some(new_key), Some(old_key)) => new_key.key_eq(old_key),
            // One keyed, one keyless — never updatable.
            _ => false,
        }
    }

    /// Get the Key associated with this View, if any.
    ///
    /// Keys are used for:
    /// - Preserving state across reorderings
    /// - GlobalKey lookups
    /// - Efficient reconciliation
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        None
    }
}

impl_downcast!(View);
clone_trait_object!(View);

/// Base trait for Elements that can be boxed.
///
/// This is the object-safe version of Element for dynamic dispatch.
/// Specific Element types (StatelessElement, StatefulElement, etc.)
/// implement the full Element trait.
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `Element` abstract class. Key methods:
/// - `mount()` / `unmount()` - lifecycle
/// - `update()` - update with new widget
/// - `rebuild()` / `performRebuild()` - rebuild children
/// - `activate()` / `deactivate()` - temporary removal
/// - `didChangeDependencies()` - inherited widget changed
pub trait ElementBase: Downcast + Send + Sync + 'static {
    // ========================================================================
    // Identity
    // ========================================================================

    /// Get the TypeId of the View that created this Element.
    fn view_type_id(&self) -> TypeId;

    /// Hash of the `Key` carried by the View this element currently
    /// holds, or `None` if that View is keyless.
    ///
    /// Keyed child reconciliation
    /// ([`reconcile_children`](crate::reconcile_children)) operates on
    /// the live `Vec<Box<dyn ElementBase>>` and must answer "what key
    /// does this old child element match on?" without naming the
    /// concrete `View` type — `ElementBase` is object-safe and erases
    /// `V`. The unified `Element<V, A, B>` overrides this to forward to
    /// `View::key().map(ViewKey::key_hash)`; every other implementor
    /// keeps the keyless default.
    ///
    /// Flutter parity: `framework.dart:4125` `Element.updateChildren`
    /// reads `oldChild.widget.key` directly because Dart elements carry
    /// a typed `widget` field. FLUI's object-safe element surface
    /// exposes the same fact through this type-erased accessor instead.
    fn current_key_hash(&self) -> Option<u64> {
        None
    }

    /// The `ViewKey` carried by the View this element currently holds,
    /// or `None` if that View is keyless.
    ///
    /// Hash-based lookup ([`Self::current_key_hash`]) is the entry point
    /// for keyed reconciliation: it indexes old children by `u64` for
    /// O(1) HashMap claims. The hash alone is not enough to decide that
    /// two keys are equal, though — distinct keys can hash to the same
    /// `u64`. This accessor surfaces the underlying
    /// [`flui_foundation::ViewKey`] so the reconciler can call
    /// [`flui_foundation::ViewKey::key_eq`] on a hash hit and reject
    /// silent collisions. Plan §U12 / FR-024 work item (c).
    ///
    /// The default impl returns `None`; the unified `Element<V, A, B>`
    /// overrides it to forward to `core.view().key()`. The borrow is
    /// alive for as long as the immutable borrow on the element holds —
    /// callers use it synchronously inside the reconciler dispatch and
    /// must not extend it across mutating calls on the element.
    ///
    /// Flutter parity: `framework.dart:4123` `Widget.canUpdate` uses
    /// `oldWidget.key == newWidget.key` directly. FLUI exposes the
    /// same fact through this typed accessor at the dispatch
    /// boundary; `View::can_update` calls the parallel typed surface
    /// on `&dyn View`.
    fn current_key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        None
    }

    /// Get the depth in the element tree (root = 0).
    fn depth(&self) -> usize;

    /// Inform this element of its own `ElementId` in the surrounding
    /// `ElementTree`.
    ///
    /// Default impl is a no-op. The unified `Element<V, A, B>`
    /// overrides this to forward to `ElementCore::set_self_id`, so
    /// `ElementCore<V, Variable>::update_or_create_children` can
    /// stamp the real parent id onto every emitted
    /// [`ReconcileEvent`](crate::tree::ReconcileEvent) instead of
    /// the §U13 placeholder.
    ///
    /// Called by [`crate::tree::ElementTree::insert`] +
    /// [`crate::tree::ElementTree::mount_root_with_pipeline_owner`]
    /// immediately after slab insertion, BEFORE the element's
    /// `mount` call. Plan §U15.
    fn set_self_id(&mut self, _id: flui_foundation::ElementId) {
        // Default: ignore. Only the unified `Element<V, A, B>`
        // overrides this; hand-rolled element impls (test fixtures,
        // future custom elements) opt in by overriding.
    }

    /// Get the slot position in parent's child list.
    fn slot(&self) -> usize {
        0
    }

    // ========================================================================
    // Lifecycle State
    // ========================================================================

    /// Get the current lifecycle state.
    fn lifecycle(&self) -> crate::element::Lifecycle;

    /// Check if this Element is currently mounted.
    fn mounted(&self) -> bool {
        matches!(
            self.lifecycle(),
            crate::element::Lifecycle::Active | crate::element::Lifecycle::Inactive
        )
    }

    // ========================================================================
    // Lifecycle Methods
    // ========================================================================

    /// Mount this Element into the tree.
    ///
    /// Called when the Element is first inserted. Sets up parent relationship
    /// and initializes state.
    ///
    /// # Arguments
    ///
    /// * `parent` - Parent `ElementId`, or `None` for the root element.
    /// * `slot` - Slot/depth position assigned by the parent.
    /// * `owner` - Split-borrow handle into [`BuildOwner`](crate::BuildOwner)
    ///   (see [`ElementOwner`](crate::ElementOwner)). Implementations
    ///   may use it to register `GlobalKey`s, schedule rebuilds, or
    ///   thread it into recursive child `mount` calls. Plan §U8.
    fn mount(
        &mut self,
        parent: Option<flui_foundation::ElementId>,
        slot: usize,
        owner: &mut crate::ElementOwner<'_>,
    );

    /// Unmount this Element (permanently removed).
    ///
    /// Called when the Element is removed from the tree permanently.
    /// Resources should be released. The split-borrow `owner` handle
    /// is provided so implementations may unregister `GlobalKey`s and
    /// recurse into child unmounts. Plan §U8.
    fn unmount(&mut self, owner: &mut crate::ElementOwner<'_>);

    /// Activate this Element (re-inserted into tree).
    ///
    /// Called when a previously deactivated Element is reinserted.
    fn activate(&mut self);

    /// Deactivate this Element (temporarily removed from tree).
    ///
    /// Called when the Element is removed but may be reinserted.
    /// State is preserved.
    fn deactivate(&mut self);

    // ========================================================================
    // Update & Rebuild
    // ========================================================================

    /// Update this Element with a new View of the same type.
    ///
    /// Called when the parent rebuilds and provides a new View
    /// configuration. The Element should update its internal state to
    /// match the new View.
    ///
    /// The split-borrow `owner` handle is provided so implementations
    /// may schedule rebuilds for descendants whose `InheritedView`
    /// dependencies changed (R16, U9 territory). Plan §U8.
    fn update(&mut self, new_view: &dyn View, owner: &mut crate::ElementOwner<'_>);

    /// Mark this Element as needing a rebuild.
    ///
    /// The Element will be rebuilt in the next build phase.
    fn mark_needs_build(&mut self);

    /// Rebuild this Element.
    ///
    /// Called by the framework when this Element is dirty.
    /// Calls `perform_build()` if needed.
    fn rebuild(&mut self, force: bool, owner: &mut crate::ElementOwner<'_>) {
        if force || self.lifecycle() == crate::element::Lifecycle::Active {
            self.perform_build(owner);
        }
    }

    /// Perform the actual build phase.
    ///
    /// Subclasses override this to rebuild their children. The
    /// split-borrow `owner` handle is threaded through so newly-mounted
    /// child elements created during this build can register
    /// `GlobalKey`s or schedule downstream rebuilds without re-borrowing
    /// the [`BuildOwner`](crate::BuildOwner). Plan §U8.
    fn perform_build(&mut self, owner: &mut crate::ElementOwner<'_>);

    // ========================================================================
    // Dependency Notifications
    // ========================================================================

    /// Notify this element that an inherited dependency it observes has
    /// changed.
    ///
    /// Called by `BuildOwner::build_scope` immediately before
    /// `perform_build` when the element's id is present in the
    /// owner's `pending_dependency_changes` set (populated by
    /// `InheritedBehavior::on_view_updated` when `update_should_notify`
    /// returns true). The unified `Element<V, A, B>` impl routes this
    /// through the behavior so `StatefulBehavior` can fire the typed
    /// `ViewState::did_change_dependencies` hook on the dependent's
    /// state BEFORE its build runs — Flutter parity for
    /// `framework.dart:6117` `StatefulElement.didChangeDependencies`
    /// (which sets the `_didChangeDependencies` flag) plus
    /// `framework.dart:5977-5982` `StatefulElement.performRebuild`
    /// (which fires `state.didChangeDependencies()` when the flag is
    /// set). Plan §U14.
    ///
    /// Default implementation is a no-op — non-stateful behaviors
    /// (Stateless, Proxy, Inherited, Render) have no typed `ViewState`
    /// to notify; the scheduled rebuild handles their reaction.
    /// `StatefulBehavior` and `AnimatedBehavior` override the
    /// behavior-side hook to forward to the state.
    fn notify_dependency_change(&mut self) {}

    // ========================================================================
    // Slot Management
    // ========================================================================

    /// Update the slot position of this Element.
    ///
    /// Called when the Element's position in the parent's child list changes.
    fn update_slot(&mut self, _new_slot: usize) {
        // Default: no-op. Subclasses can override.
    }

    // ========================================================================
    // Child Management
    // ========================================================================

    /// Visit all child Elements.
    fn visit_children(&self, visitor: &mut dyn FnMut(flui_foundation::ElementId));

    /// Get the first child Element, if any.
    fn first_child(&self) -> Option<flui_foundation::ElementId> {
        let mut first = None;
        self.visit_children(&mut |id| {
            if first.is_none() {
                first = Some(id);
            }
        });
        first
    }

    /// Deactivate a child Element.
    ///
    /// Removes the child from the tree but preserves its state.
    fn deactivate_child(&mut self, _child: flui_foundation::ElementId) {
        // Default: no-op. Subclasses should implement.
    }

    // ========================================================================
    // Debug
    // ========================================================================

    /// Get a debug description of this Element.
    fn debug_description(&self) -> String {
        format!(
            "Element(type={:?}, lifecycle={:?}, depth={})",
            self.view_type_id(),
            self.lifecycle(),
            self.depth()
        )
    }

    // ========================================================================
    // RenderObject Access
    // ========================================================================

    /// Get the RenderObject managed by this Element, if any.
    ///
    /// Only RenderObjectElement implementations return Some.
    /// ComponentElements (Stateless, Stateful) return None.
    ///
    /// This is used by parent RenderObjectElements to attach child
    /// RenderObjects to the render tree.
    fn render_object_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Get the RenderObject managed by this Element mutably, if any.
    fn render_object_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }

    /// Get the first child element, if any.
    ///
    /// Used for traversing the element tree to find descendant RenderObjects.
    fn child_element(&self) -> Option<&dyn ElementBase> {
        None
    }

    /// Get the first child element mutably, if any.
    fn child_element_mut(&mut self) -> Option<&mut dyn ElementBase> {
        None
    }

    /// Called by parent to attach this element's RenderObject to the render
    /// tree.
    ///
    /// For RenderObjectElements, this returns the RenderObject that should be
    /// inserted into the parent's render object.
    ///
    /// For ComponentElements (Stateless, Stateful), this delegates to the
    /// child.
    ///
    /// # Flutter Equivalent
    ///
    /// This corresponds to the pattern where `attachRenderObject` calls
    /// `ancestorRenderObjectElement.insertRenderObjectChild(renderObject,
    /// slot)`.
    fn attach_to_render_tree(&mut self) -> Option<&mut dyn std::any::Any> {
        // Default: no RenderObject to attach
        // ComponentElements override to delegate to child
        // RenderElements override to return their RenderObject
        None
    }

    /// Get the RenderObject as a shared Arc for render tree attachment.
    ///
    /// This enables the Flutter-like pattern where RenderObjects are owned
    /// by Elements but referenced by parent RenderObjects in the render tree.
    ///
    /// # Returns
    ///
    /// An Arc containing the RenderObject, or None if this element doesn't
    /// have a RenderObject or doesn't support shared ownership.
    fn render_object_shared(
        &self,
    ) -> Option<std::sync::Arc<parking_lot::RwLock<dyn std::any::Any + Send + Sync>>> {
        None
    }

    // ========================================================================
    // Pipeline Owner Propagation (for RenderTree integration)
    // ========================================================================

    /// Set the PipelineOwner for this element.
    ///
    /// Called by parent elements to propagate the PipelineOwner down the tree.
    /// RenderObjectElements use this to insert their RenderObjects into the
    /// RenderTree.
    ///
    /// Default implementation does nothing - only RenderObjectElements need
    /// this.
    ///
    /// # Arguments
    /// * `owner` - `Arc<dyn Any>` that should be downcast to the concrete
    ///   `PipelineOwner` type
    fn set_pipeline_owner_any(&mut self, _owner: std::sync::Arc<dyn std::any::Any + Send + Sync>) {
        // Default: no-op
    }

    /// Set the parent's RenderId for tree structure.
    ///
    /// Called by parent elements to establish parent-child relationships in
    /// RenderTree. Child RenderObjects will be attached as children of this
    /// RenderId.
    fn set_parent_render_id(&mut self, _parent_id: Option<flui_foundation::RenderId>) {
        // Default: no-op
    }

    // ========================================================================
    // Inherited-element protocol (U9 / R4)
    // ========================================================================

    /// Object-safe accessor onto this element if it is an
    /// `InheritedElement<V>` (a `Element<V, Single, InheritedBehavior<V>>`).
    ///
    /// Returns `None` for every other behavior. Used by
    /// [`BuildContext::depend_on_inherited`](crate::BuildContext::depend_on_inherited) (plan §U9) to read the
    /// view as `&dyn Any` and to record this caller as a dependent.
    ///
    /// The default impl returns `None`. Only the unified `Element`
    /// specialization for `InheritedBehavior<V>` overrides this in
    /// `crates/flui-view/src/element/unified.rs`.
    fn as_inherited(&self) -> Option<&dyn crate::element::InheritedElementAccess> {
        None
    }

    /// Mutable variant of [`Self::as_inherited`].
    fn as_inherited_mut(&mut self) -> Option<&mut dyn crate::element::InheritedElementAccess> {
        None
    }

    // ========================================================================
    // Ancestor-finder protocol (U11 / R6, R7, R8)
    // ========================================================================

    /// Borrow the View configuration this element holds as `&dyn Any`.
    ///
    /// Returns `None` by the default impl; the unified `Element<V, A, B>`
    /// overrides it to hand out `&ElementCore::view` as `&dyn Any` so
    /// [`BuildContext::find_ancestor_view`](crate::BuildContext::find_ancestor_view) can downcast at the dispatch
    /// boundary without naming `V` at the trait surface.
    ///
    /// The reference is borrowed for the lifetime of the immutable
    /// borrow on this element — the caller's typed-callback wrapper
    /// runs synchronously while the tree-read-lock is held, never
    /// extending the borrow into the rest of `build()`. Plan §U11.
    ///
    /// Flutter parity: `framework.dart:5122`
    /// `findAncestorWidgetOfExactType<T>` — reads `element.widget` once
    /// the ancestor is identified.
    fn view_as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    /// Borrow this element's persistent `ViewState` as `&dyn Any` if
    /// this is a `StatefulElement<V>`.
    ///
    /// Returns `None` for every behavior other than `StatefulBehavior<V>`
    /// (which yields `Some(&self.behavior.state)`). Used by
    /// [`BuildContext::find_ancestor_state`](crate::BuildContext::find_ancestor_state) and
    /// [`BuildContext::find_root_ancestor_state`](crate::BuildContext::find_root_ancestor_state) (plan §U11) to surface
    /// the typed `ViewState` without leaking `V` into the object-safe
    /// trait surface.
    ///
    /// Flutter parity: `framework.dart:5132`
    /// `findAncestorStateOfType<T>` and `framework.dart:5146`
    /// `findRootAncestorStateOfType<T>` both read `element.state` on a
    /// `StatefulElement` after the runtime-type check succeeds. We do
    /// the equivalent runtime-type check via `TypeId::of::<S>()` keyed
    /// off `V::State`.
    fn state_as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }

    // ========================================================================
    // RenderObject-finder protocol (U12 / R9)
    // ========================================================================

    /// Borrow this element's `RenderId` if it is a `RenderElement<V>`
    /// (a `Element<V, Variable, RenderBehavior<V>>`) whose `on_mount`
    /// already created its `RenderObject`.
    ///
    /// Returns `None` for every behavior other than `RenderBehavior<V>`
    /// AND for `RenderElement`s that have not yet been mounted with a
    /// `PipelineOwner` (in which case `RenderBehavior::render_id` is
    /// still `None`). Used by [`BuildContext::find_render_object`](crate::BuildContext::find_render_object)
    /// (plan §U12) to surface the nearest ancestor's `RenderId` without
    /// extending a `&self` borrow — `RenderId` is `Copy`, so the
    /// non-callback signature is sound (plan §D2).
    ///
    /// Flutter parity: `framework.dart:5160`
    /// `findAncestorRenderObjectOfType<T>` walks `_parent` and reads
    /// `(ancestor as RenderObjectElement).renderObject` once the
    /// runtime-type check succeeds. We do the equivalent strict-ancestor
    /// walk and read `RenderBehavior::render_id` at the dispatch
    /// boundary.
    fn render_id(&self) -> Option<flui_foundation::RenderId> {
        None
    }

    // ========================================================================
    // Notification handler protocol (U13 / R10)
    // ========================================================================

    /// Object-safe notification handler invoked by
    /// [`BuildContext::dispatch_notification`](crate::BuildContext::dispatch_notification) during ancestor bubble walks.
    ///
    /// `type_id` is `TypeId::of::<N>()` for the static notification type
    /// `N` captured at the dispatch call-site. `notification` is the
    /// notification value coerced to `&dyn Any` so this method can stay
    /// object-safe (Constitution Principle 4: single `dyn` boundary at
    /// dispatch — not `dyn`-everywhere). Implementations must:
    ///
    /// 1. Check `type_id` against the static `TypeId::of::<N>()` of the
    ///    notification type they care about — skip if mismatch.
    /// 2. Downcast `notification` via `<dyn Any>::downcast_ref::<N>()`.
    /// 3. Invoke their typed handler (e.g. the typed
    ///    [`NotifiableElement<N>`](crate::element::NotifiableElement)
    ///    wrapper) and return its `bool`.
    ///
    /// Default returns `false` so non-listener elements are skipped
    /// cleanly during the bubble walk. The unified `Element<V, A, B>`
    /// overrides this to delegate through the behavior, which in turn
    /// keeps the default unless the user opts in.
    ///
    /// Returning `true` cancels the bubble; `false` lets it continue to
    /// the next ancestor. Plan U13 / R10. Flutter parity:
    /// `notification_listener.dart:127`
    /// (`_NotificationElement.onNotification`) performs the same
    /// runtime-type check + downcast + typed-callback chain.
    fn on_notification(&self, type_id: std::any::TypeId, notification: &dyn std::any::Any) -> bool {
        let _ = (type_id, notification);
        false
    }
}

impl_downcast!(ElementBase);

#[cfg(test)]
mod tests {
    use super::*;

    // Basic compile-time checks
    fn _assert_view_is_object_safe(_: &dyn View) {}
    fn _assert_element_base_is_object_safe(_: &dyn ElementBase) {}

    // AE7: `View::key()` accepts any flui_foundation::ViewKey impl
    // (GlobalKey, ValueKey, UniqueKey, ObjectKey) without an `as` cast.
    // Compile-time check that `&ValueKey<i32>` and `&UniqueKey`
    // (foundation ViewKey impls) coerce to
    // `Option<&dyn flui_foundation::ViewKey>` - the exact return type of
    // `View::key()`. This mirrors what a `View::key()` body does without
    // requiring a full View impl.
    fn _assert_view_key_accepts_concrete_impls() {
        use flui_foundation::{UniqueKey, ValueKey};

        // ValueKey<T> where T: Clone + Hash + Eq + Send + Sync + Debug
        static VALUE_KEY: ValueKey<i32> = ValueKey::new(42);
        let _: Option<&dyn flui_foundation::ViewKey> = Some(&VALUE_KEY);

        // UniqueKey
        let unique = UniqueKey::new();
        let _: Option<&dyn flui_foundation::ViewKey> = Some(&unique);

        // None branch (default View::key() impl)
        let _: Option<&dyn flui_foundation::ViewKey> = None;
    }
}
