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

    /// Typed memoization short-circuit.
    ///
    /// Returns `true` if a same-type, same-position rebuild can be **skipped**
    /// because `self` is interchangeable with `prev` — i.e. both views would
    /// produce identical output if built.
    ///
    /// # Default
    ///
    /// `false` — always rebuild (Flutter parity). This is the safe default:
    /// skipping a rebuild whose output *would* have differed silently loses
    /// user-visible state, so the opt-in direction is the only safe one.
    ///
    /// # Opting in
    ///
    /// Override this method, or wrap your view in [`crate::Memo<V>`] which
    /// provides a `PartialEq`-based implementation. The `PartialEq` bound
    /// lives **only** on `Memo<V>` — there is no blanket bound on `View`
    /// (the Druid trap). See `docs/FOUNDATIONS.md` C1.
    ///
    /// # Opposite polarity to `can_update`
    ///
    /// [`View::can_update`] is the Flutter *type + key matchability* gate:
    /// can this element be *reused at all*? `should_skip_rebuild` is a
    /// *content equality* short-circuit: given that the element **is**
    /// being reused, can the rebuild be skipped? Do **not** merge them —
    /// they answer different questions and operate at different points in
    /// the dispatch pipeline.
    ///
    /// # Object-safety
    ///
    /// The `where Self: Sized` bound excludes this method from the `dyn View`
    /// vtable, keeping `View` object-safe (Constitution C4).
    fn should_skip_rebuild(&self, prev: &Self) -> bool
    where
        Self: Sized,
    {
        let _ = prev;
        false
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
    /// Keyed child reconciliation (`reconcile_children_by_id`) walks the
    /// parent's slab-resident [`ElementNode::child_ids`](crate::tree::ElementNode)
    /// and must answer "what key does this old child element match on?"
    /// without naming the concrete `View` type — `ElementBase` is
    /// object-safe and erases `V`. The unified `Element<V, A, B>` overrides
    /// this to forward to `View::key().map(ViewKey::key_hash)`; every other
    /// implementor keeps the keyless default.
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

    /// Returns `true` if this element has been marked dirty and needs a
    /// rebuild before the next frame.
    ///
    /// The default implementation returns `false`; the unified
    /// `Element<V, A, B>` overrides it to forward to
    /// `ElementCore::is_dirty`. Hand-rolled element impls (test
    /// fixtures, future custom elements) may override as needed.
    fn is_dirty(&self) -> bool {
        false
    }

    /// Run this element's build half and return its OWNED child view(s).
    ///
    /// E3 (atomic box→arena swap): this replaces the old
    /// `perform_build(&mut self, owner)` that reconciled children against
    /// box storage in place. The build seam is now cut so that
    /// `build_into_views` runs the behavior's `build()` (today's
    /// `build_or_recover` / `build_proxy_style` half) and returns the
    /// owned child views WITHOUT touching any child storage — there is no
    /// child storage on the element any more. The reconcile half is
    /// hoisted out to [`BuildOwner::build_scope`](crate::BuildOwner),
    /// which feeds the returned views to `reconcile_children_by_id` against
    /// the slab-resident [`ElementTree`](crate::tree::ElementTree) with a
    /// fresh `&mut tree` borrow. No `&mut element` is ever live across a
    /// `&mut tree` child mutation — see the E3 design.
    ///
    /// Return contract by behavior:
    /// - Stateless / Stateful → `vec![child_view]` (single child).
    /// - Proxy / Inherited → `vec![child_view]` (the proxied child).
    /// - Leaf / a render element with no view-children → `vec![]`.
    /// - RenderObject → the child view(s) the render element owns; the
    ///   RenderObject-attach side-effects stay inside `build_into_views`
    ///   (they touch the pipeline owner / render tree, not the element
    ///   slab, so they cannot double-borrow the element arena).
    ///
    /// The split-borrow `owner` handle is threaded through so the build
    /// half can still register `GlobalKey`s or schedule downstream
    /// rebuilds without re-borrowing the [`BuildOwner`](crate::BuildOwner).
    fn build_into_views(&mut self, owner: &mut crate::ElementOwner<'_>) -> Vec<Box<dyn View>>;

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
    ///
    /// `owner` carries the split-borrow build handle so the typed hook can
    /// resolve the same live tree-backed `BuildContext` the rebuild uses
    /// (PR-K).
    #[allow(unused_variables)]
    fn notify_dependency_change(&mut self, owner: &mut crate::ElementOwner<'_>) {}

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

    // E3 (atomic box→arena swap): `visit_children` is deleted. Elements
    // no longer own a child graph — the slab-resident
    // [`ElementTree`](crate::tree::ElementTree) is the single element
    // graph, and a node's children are its
    // [`ElementNode::child_ids`](crate::tree::ElementNode) list. All
    // child traversal goes through `tree.get(id).child_ids()`, where both
    // the id and a `&ElementTree` are in scope. The old per-element
    // `visit_children` was a no-op on every unified `Element<V, A, B>`
    // anyway (children lived inside `ElementCore`, invisible to the slab
    // walk), which is exactly why production `setState` was inert.

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

    /// Hand out this element's `PipelineOwner` as `Arc<dyn Any>` so the
    /// slab `insert` path can propagate it to a freshly-inserted child
    /// BEFORE the child mounts.
    ///
    /// E3 (atomic box→arena swap): in the old box graph the parent
    /// propagated the owner to its children inside
    /// `update_or_create_child(ren)`. Children are now slab-resident, so
    /// [`ElementTree::insert`](crate::tree::ElementTree) reads this from
    /// the parent and threads it (plus [`Self::child_render_id`]) into the
    /// child's `set_pipeline_owner_any` / `set_parent_render_id` before
    /// `mount`, preserving the propagate-before-mount ordering
    /// `RenderBehavior::on_mount` depends on (it creates its
    /// `RenderObject` only when a `PipelineOwner` is already in scope).
    ///
    /// Default returns `None`; the unified `Element<V, A, B>` overrides it
    /// to hand out its `ElementCore`'s owner.
    fn pipeline_owner_any(&self) -> Option<std::sync::Arc<dyn std::any::Any + Send + Sync>> {
        None
    }

    /// The `RenderId` that this element's *children* should attach their
    /// `RenderObject`s under.
    ///
    /// E3 propagation contract (companion to [`Self::pipeline_owner_any`]):
    /// for a `RenderObjectElement` this is its own `render_id` (children
    /// attach under it); for a component element it is the
    /// `parent_render_id` the element itself received (the nearest
    /// ancestor render object passes straight through). Default `None`.
    fn child_render_id(&self) -> Option<flui_foundation::RenderId> {
        None
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

    /// The parent-data configuration this element contributes to its child's
    /// render node, if any.
    ///
    /// Returns `Some` only for a `ParentDataElement` (a `ParentDataView` such
    /// as `Flexible` / `Expanded` / `Positioned`); every other behavior returns
    /// the default `None`. The [`ElementTree`](crate::tree::ElementTree)
    /// insert/update seam walks ancestors of a freshly-attached render child,
    /// collects each `Some` between the child and the nearest ancestor render
    /// object, and writes them onto that child's render node (nearest wins).
    ///
    /// Flutter parity: `ParentDataElement.applyParentData` —
    /// `framework.dart`'s `ParentDataElement<T>` attaches its
    /// `ParentDataWidget.applyParentData` payload to the descendant
    /// `RenderObject.parentData` at the same point we write it here.
    fn parent_data_config(&self) -> Option<Box<dyn flui_rendering::parent_data::ParentData>> {
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
