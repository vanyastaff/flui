//! BuildContext - the interface Elements provide during build.
//!
//! BuildContext is passed to Views during the build phase, providing
//! access to tree information and dependency injection.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `BuildContext` abstract class.
//! In Flutter, `Element` implements `BuildContext` - same pattern here.

use std::any::TypeId;

use flui_foundation::ElementId;

/// Context provided to Views during the build phase.
///
/// `BuildContext` provides Views with:
/// - Element identity and tree position
/// - Dependency injection (InheritedView lookups)
/// - Ancestor lookups (find ancestors by type)
/// - Dirty marking for rebuilds
///
/// # Important Notes
///
/// - Most methods should only be called during build
/// - `depend_on_inherited` registers a dependency (causes rebuild on change)
/// - `get_inherited` does NOT register a dependency (one-time lookup)
/// - Ancestor lookups walk the tree - use sparingly
///
/// # Example
///
/// ```rust,ignore
/// impl StatelessView for MyView {
///     fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///         // Access inherited data (registers dependency)
///         let theme = ctx.depend_on::<ThemeData>();
///
///         // One-time lookup (no dependency)
///         let config = ctx.get::<AppConfig>();
///
///         // Get element info
///         let depth = ctx.depth();
///
///         // Build child
///         Text::new("Hello")
///     }
/// }
/// ```
pub trait BuildContext: Send + Sync {
    // ========================================================================
    // Identity & State
    // ========================================================================

    /// Get the ElementId of the Element providing this context.
    fn element_id(&self) -> ElementId;

    /// Get the depth of this Element in the tree.
    ///
    /// Root Element has depth 0.
    fn depth(&self) -> usize;

    /// Check if this Element is currently mounted in the tree.
    ///
    /// Returns false after `unmount()` has been called.
    fn mounted(&self) -> bool;

    /// Check if we're currently in a build phase.
    ///
    /// Only valid in debug builds.
    fn is_building(&self) -> bool;

    // ========================================================================
    // Rebuild capability
    // ========================================================================

    /// An owned, `'static` handle that schedules **this** element for rebuild on
    /// the next frame (ADR-0018 U1).
    ///
    /// Capture it in `ViewState::init_state` (or `did_change_dependencies`) and
    /// call [`RebuildHandle::schedule`](crate::RebuildHandle::schedule) from a
    /// completion callback on any thread. `schedule()` only writes to
    /// `BuildOwner`'s shared inbox and requests a frame; the rebuild itself runs
    /// on the frame thread inside `build_scope`.
    ///
    /// # Never acquire this during `build`
    ///
    /// Scheduling from `build` is an unbounded rebuild loop, and scheduling from
    /// layout or paint would rebuild the tree mid-frame. `scripts/port-check.sh`
    /// trigger **#22** rejects `rebuild_handle()` in `build` / `perform_layout` /
    /// `paint` / composite bodies, as [`FOUNDATIONS.md`] requires of any
    /// out-of-catalog `mark_needs_build` driver.
    ///
    /// [`FOUNDATIONS.md`]: ../../../docs/FOUNDATIONS.md
    fn rebuild_handle(&self) -> crate::RebuildHandle;

    /// The binding's frame-driven async task driver (ADR-0018 U2), if a binding
    /// installed one.
    ///
    /// Spawn subscriptions from `ViewState::init_state` / `did_change_dependencies`
    /// and hold the returned `TaskToken` in the state — dropping it cancels.
    ///
    /// `None` when the tree is not bound to a binding (a bare `ElementTree` in a
    /// unit test), reported honestly rather than by silently spawning into a
    /// driver nobody polls. Never reach for `Scheduler::instance()` from a
    /// widget: `HeadlessBinding` drives a binding-local `Scheduler`, so the
    /// singleton's tasks would never run headlessly.
    fn async_driver(&self) -> Option<flui_scheduler::AsyncDriver>;

    /// The binding's post-frame capability — schedule work that must observe this
    /// frame's committed layout (ADR-0021 U2).
    ///
    /// `None` when no binding installed one. Acquire it in a lifecycle hook
    /// (`init_state` / `did_change_dependencies`), never in `build`/layout/paint —
    /// the same rule `rebuild_handle` follows (port-check trigger #22).
    fn post_frame_handle(&self) -> Option<flui_scheduler::PostFrameHandle>;

    // ========================================================================
    // Inherited Data (Dependency Injection)
    // ========================================================================

    /// Look up data from an ancestor InheritedView and register a dependency.
    ///
    /// This registers a dependency - when the InheritedView's data changes,
    /// this Element will be rebuilt and `did_change_dependencies()` called.
    ///
    /// # Callback form
    ///
    /// The closure receives a reference to the matched InheritedView as
    /// `&dyn Any` and runs synchronously while the implementation holds
    /// the necessary tree-lock — this preserves the declarative-build
    /// invariant (Constitution Principle 5) AND avoids extending a
    /// `&self` borrow into the rest of `build()`. The typed wrapper
    /// [`BuildContextExt::depend_on`] handles `TypeId` resolution and
    /// downcast.
    ///
    /// Returns `true` if an ancestor InheritedView of that type was
    /// found and the callback was invoked; `false` otherwise.
    ///
    /// Plan §U9 / R4. Flutter parity: `framework.dart:5081`
    /// `dependOnInheritedWidgetOfExactType`.
    fn depend_on_inherited(
        &self,
        type_id: TypeId,
        callback: &mut dyn FnMut(&dyn std::any::Any),
    ) -> bool;

    /// Look up data from an ancestor InheritedView WITHOUT registering a
    /// dependency.
    ///
    /// Unlike [`depend_on_inherited`], this does NOT cause rebuilds when
    /// the InheritedView changes. Use this for one-time lookups where
    /// you don't need to track changes.
    ///
    /// Same callback shape as `depend_on_inherited`.
    ///
    /// [`depend_on_inherited`]: BuildContext::depend_on_inherited
    fn get_inherited(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn std::any::Any)) -> bool;

    // ========================================================================
    // Ancestor Lookups
    // ========================================================================

    /// Get the nearest ancestor Element of a specific type.
    ///
    /// Walks up the tree until an Element with matching view type is found.
    /// This does NOT register a dependency.
    ///
    /// # Performance
    ///
    /// O(n) where n is distance to ancestor. Use sparingly.
    fn find_ancestor_element(&self, type_id: TypeId) -> Option<ElementId>;

    /// Get the nearest ancestor View of a specific type.
    ///
    /// Similar to `find_ancestor_element` but reads the View configuration
    /// itself rather than the `ElementId`.
    ///
    /// # Callback form
    ///
    /// The closure receives a reference to the matched ancestor View as
    /// `&dyn Any` and runs synchronously while the implementation holds
    /// the necessary tree-read-lock — this preserves the
    /// declarative-build invariant (Constitution Principle 5) AND avoids
    /// extending a `&self` borrow into the rest of `build()`. The typed
    /// wrapper [`BuildContextExt::find_ancestor`] handles `TypeId`
    /// resolution and downcast.
    ///
    /// Returns `true` if an ancestor View of that type was found and the
    /// callback was invoked; `false` otherwise. The callback is invoked
    /// at most once.
    ///
    /// Plan §U11 / R6. Flutter parity: `framework.dart:5122`
    /// `findAncestorWidgetOfExactType<T>`.
    fn find_ancestor_view(
        &self,
        type_id: TypeId,
        callback: &mut dyn FnMut(&dyn std::any::Any),
    ) -> bool;

    /// Get the nearest ancestor `ViewState` of a specific type.
    ///
    /// `type_id` keys off the **State** type (`TypeId::of::<V::State>()`),
    /// not the StatefulView type — Flutter's
    /// `findAncestorStateOfType<T extends State>` does the same: it
    /// matches against the State runtime type, since two different
    /// StatefulWidgets may share a State subtype.
    ///
    /// Same callback shape as [`find_ancestor_view`]: synchronous
    /// callback while the read-lock is held, no borrow extension.
    ///
    /// Plan §U11 / R7. Flutter parity: `framework.dart:5132`
    /// `findAncestorStateOfType<T>`.
    ///
    /// [`find_ancestor_view`]: BuildContext::find_ancestor_view
    fn find_ancestor_state(
        &self,
        type_id: TypeId,
        callback: &mut dyn FnMut(&dyn std::any::Any),
    ) -> bool;

    /// Get the root-most ancestor `ViewState` of a specific type.
    ///
    /// Unlike [`find_ancestor_state`], walks **all** the way to the
    /// root and yields the **furthest** matching ancestor, not the
    /// nearest. Useful for reaching a top-level navigator/scaffold
    /// state from a deeply nested view.
    ///
    /// Same callback shape and `type_id` semantics as
    /// [`find_ancestor_state`].
    ///
    /// Plan §U11 / R8. Flutter parity: `framework.dart:5146`
    /// `findRootAncestorStateOfType<T>`.
    ///
    /// [`find_ancestor_state`]: BuildContext::find_ancestor_state
    fn find_root_ancestor_state(
        &self,
        type_id: TypeId,
        callback: &mut dyn FnMut(&dyn std::any::Any),
    ) -> bool;

    // ========================================================================
    // RenderObject Access
    // ========================================================================

    /// Find the nearest RenderObject.
    ///
    /// If this Element is a RenderElement, returns its RenderObject.
    /// Otherwise, walks down to find the first descendant RenderObject.
    ///
    /// # Returns
    ///
    /// The RenderObject ID if found, None otherwise.
    fn find_render_object(&self) -> Option<flui_foundation::RenderId>;

    /// The render tree this element is mounted in.
    ///
    /// [`find_render_object`](Self::find_render_object) hands out a `RenderId`, and
    /// a `RenderId` alone answers nothing: geometry lives in the
    /// [`PipelineOwner`](flui_rendering::pipeline::PipelineOwner) that owns the
    /// node. Flutter has no equivalent because a Dart `RenderObject` *is* the
    /// handle — `renderObject.size`, `renderObject.getTransformTo(ancestor)`
    /// (`heroes.dart:952`, `:999`, `:1014-1018`). This is that reference,
    /// reified.
    ///
    /// # This is not a frame capability
    ///
    /// It schedules nothing, so port-check trigger #22 does not guard it and
    /// acquiring it inside `build` is harmless — a `PipelineOwner` read during build
    /// simply answers `None` for every un-laid-out node (ADR-0021 U1). What it is
    /// *for* is the opposite direction: code outside the tree (a routing observer, a
    /// `HeroController`) holding an owned handle so it can resolve a `RenderId` to
    /// geometry from a post-frame callback, after layout commits.
    ///
    /// `None` before the element is mounted under a pipeline owner.
    fn pipeline_owner(
        &self,
    ) -> Option<std::sync::Arc<parking_lot::RwLock<flui_rendering::pipeline::PipelineOwner>>>;

    // ========================================================================
    // Tree Traversal
    // ========================================================================

    /// Visit ancestor Elements from this Element up to root.
    ///
    /// The visitor returns `true` to continue, `false` to stop.
    fn visit_ancestor_elements(&self, visitor: &mut dyn FnMut(ElementId) -> bool);

    /// Visit child Elements of this Element.
    ///
    /// # Note
    ///
    /// Cannot be called during build - will panic in debug mode.
    fn visit_child_elements(&self, visitor: &mut dyn FnMut(ElementId));

    // ========================================================================
    // Rebuild Control
    // ========================================================================

    /// Mark this Element as needing a rebuild.
    ///
    /// The Element will be rebuilt in the next build phase.
    fn mark_needs_build(&self);

    // ========================================================================
    // Notification Dispatch
    // ========================================================================

    /// Dispatch a notification up the element tree.
    ///
    /// The notification bubbles up from this context until a
    /// NotifiableElement handles it (returns true) or the root is reached.
    ///
    /// # Arguments
    ///
    /// * `notification` - The notification to dispatch
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_view::{Notification, LayoutChangedNotification};
    ///
    /// // Dispatch from inside a View's build method
    /// ctx.dispatch_notification(&LayoutChangedNotification);
    /// ```
    ///
    /// # Flutter Equivalent
    ///
    /// Corresponds to Flutter's `BuildContext.dispatchNotification()`.
    fn dispatch_notification(&self, notification: &dyn crate::element::Notification);
}

/// Extension trait for typed InheritedView lookups.
pub trait BuildContextExt: BuildContext {
    /// Look up data from an ancestor InheritedView (with dependency).
    ///
    /// Typed callback wrapper over [`BuildContext::depend_on_inherited`].
    /// The closure receives `&T` (the InheritedView) and returns any
    /// derived value `R` — typically a cloned `Data` field. Registers
    /// a dependency: when the InheritedView's data changes, this
    /// Element rebuilds.
    ///
    /// Callback form chosen over `Option<&T>` to preserve the
    /// declarative-build invariant (Constitution Principle 5) and avoid
    /// extending the inherited-data borrow across the rest of
    /// `build()`. See plan §D2.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Clone the entire data:
    /// let theme: Option<ThemeData> = ctx.depend_on::<Theme, _>(|t| t.data().clone());
    /// // Or extract a single field:
    /// let color: Option<u32> = ctx.depend_on::<Theme, _>(|t| t.primary_color);
    /// ```
    fn depend_on<T: 'static, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let mut result: Option<R> = None;
        let mut once = Some(f);
        self.depend_on_inherited(TypeId::of::<T>(), &mut |any| {
            if let (Some(typed), Some(call)) = (any.downcast_ref::<T>(), once.take()) {
                result = Some(call(typed));
            }
        });
        result
    }

    /// Look up data from an ancestor InheritedView (without dependency).
    ///
    /// Typed callback wrapper over [`BuildContext::get_inherited`]. Does
    /// NOT register a dependency — use for one-time lookups.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config_name: Option<String> = ctx.get::<AppConfig, _>(|c| c.name.clone());
    /// ```
    fn get<T: 'static, R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        let mut result: Option<R> = None;
        let mut once = Some(f);
        self.get_inherited(TypeId::of::<T>(), &mut |any| {
            if let (Some(typed), Some(call)) = (any.downcast_ref::<T>(), once.take()) {
                result = Some(call(typed));
            }
        });
        result
    }

    /// Find the nearest ancestor View of type `V` and apply `f` to it.
    ///
    /// Typed callback wrapper over [`BuildContext::find_ancestor_view`].
    /// The closure receives `&V` and returns any derived value `R` —
    /// typically a cloned field. Does NOT register a dependency
    /// (ancestor lookups are read-only walks; only `depend_on` records
    /// dependents).
    ///
    /// Callback form chosen over `Option<&V>` to preserve the
    /// declarative-build invariant (Constitution Principle 5) and avoid
    /// extending the ancestor-view borrow across the rest of `build()`.
    /// See plan §D2.
    ///
    /// Plan §U11 / R6. Flutter parity: `framework.dart:5122`
    /// `findAncestorWidgetOfExactType<T>`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let title: Option<String> =
    ///     ctx.find_ancestor::<Scaffold, _>(|s| s.title.clone());
    /// ```
    fn find_ancestor<V: 'static, R>(&self, f: impl FnOnce(&V) -> R) -> Option<R> {
        let mut result: Option<R> = None;
        let mut once = Some(f);
        self.find_ancestor_view(TypeId::of::<V>(), &mut |any| {
            if let (Some(typed), Some(call)) = (any.downcast_ref::<V>(), once.take()) {
                result = Some(call(typed));
            }
        });
        result
    }

    /// Find the nearest ancestor `ViewState` of type `S` and apply `f`.
    ///
    /// Typed callback wrapper over [`BuildContext::find_ancestor_state`].
    /// `S` is the State type itself (e.g. `MyCounterState`), not the
    /// owning StatefulView — Flutter's `findAncestorStateOfType<T>` does
    /// the same: it keys off the State runtime type.
    ///
    /// Same callback contract as [`find_ancestor`]: synchronous run, no
    /// borrow extension. Does NOT register a dependency.
    ///
    /// Plan §U11 / R7. Flutter parity: `framework.dart:5132`
    /// `findAncestorStateOfType<T>`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let count: Option<i32> =
    ///     ctx.find_state::<CounterState, _>(|s| s.count);
    /// ```
    ///
    /// [`find_ancestor`]: BuildContextExt::find_ancestor
    fn find_state<S: 'static, R>(&self, f: impl FnOnce(&S) -> R) -> Option<R> {
        let mut result: Option<R> = None;
        let mut once = Some(f);
        self.find_ancestor_state(TypeId::of::<S>(), &mut |any| {
            if let (Some(typed), Some(call)) = (any.downcast_ref::<S>(), once.take()) {
                result = Some(call(typed));
            }
        });
        result
    }

    /// Find the **root-most** ancestor `ViewState` of type `S` and apply `f`.
    ///
    /// Unlike [`find_state`], walks all the way to the root of the
    /// element tree and invokes the callback on the **furthest**
    /// matching State, not the nearest. Useful for reaching a top-level
    /// navigator/scaffold from a deeply nested view.
    ///
    /// Same callback contract as [`find_state`]: synchronous run, no
    /// borrow extension. Does NOT register a dependency.
    ///
    /// Plan §U11 / R8. Flutter parity: `framework.dart:5146`
    /// `findRootAncestorStateOfType<T>`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let navigator_route: Option<String> =
    ///     ctx.find_root_state::<NavigatorState, _>(|s| s.current_route.clone());
    /// ```
    ///
    /// [`find_state`]: BuildContextExt::find_state
    fn find_root_state<S: 'static, R>(&self, f: impl FnOnce(&S) -> R) -> Option<R> {
        let mut result: Option<R> = None;
        let mut once = Some(f);
        self.find_root_ancestor_state(TypeId::of::<S>(), &mut |any| {
            if let (Some(typed), Some(call)) = (any.downcast_ref::<S>(), once.take()) {
                result = Some(call(typed));
            }
        });
        result
    }
}

impl<C: BuildContext + ?Sized> BuildContextExt for C {}

#[cfg(test)]
mod tests {
    use super::*;

    // Check that BuildContext is object-safe
    fn _assert_object_safe(_: &dyn BuildContext) {}
}
