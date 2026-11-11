//! StatefulView - Views with persistent mutable state.
//!
//! StatefulViews maintain state that persists across rebuilds.
//! The state is held by the Element, not the View itself.

use super::into_view::IntoView;
use crate::context::{BuildContext, LifecycleContext};

/// A View that has persistent mutable state.
///
/// StatefulViews separate configuration (the View) from mutable state
/// (the `ViewState`). The View is immutable and recreated each build,
/// while the State persists in the Element.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{BuildContextExt, LifecycleContext, Signal, StatefulView, ViewState, BuildContext, IntoView};
///
/// #[derive(Clone)]
/// struct Counter {
///     initial: i32,
/// }
///
/// struct CounterState {
///     count: Signal<i32>,
/// }
///
/// impl StatefulView for Counter {
///     type State = CounterState;
///
///     fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
///         CounterState { count: ctx.signal(self.initial) }
///     }
/// }
///
/// impl ViewState<Counter> for CounterState {
///     fn build(&self, _view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
///         Text::new(format!("Count: {}", self.count.get(ctx)))
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not implement `StatefulView`",
    label = "missing `impl StatefulView for {Self}`",
    note = "a stateful view names its state and creates it: `impl StatefulView for {Self} {{ type State = MyState; fn create_state(&self, ctx: &dyn LifecycleContext) -> MyState {{ .. }} }}`, where `MyState: ViewState<{Self}>`"
)]
pub trait StatefulView: Clone + 'static + Sized {
    /// The State type for this View.
    type State: ViewState<Self>;

    /// Create the state, with its signals and capabilities already bound.
    ///
    /// Called exactly once per element, at its first build and before
    /// [`ViewState::init_state`], from the element's current view. `ctx` is
    /// the same [`LifecycleContext`] `init_state` receives: create the
    /// state's signals here (`ctx.signal(..)`), so a state never holds a
    /// signal that is not yet bound, and acquire the presentation
    /// capabilities the state keeps. The returned state persists across
    /// View rebuilds.
    ///
    /// Flutter's `createState` takes no context and its `initState` creates
    /// what needs one. Here the context arrives one step earlier so a field
    /// can be the handle itself: a `Signal<T>` has no unbound value, and a
    /// state never holds an `Option` that `init_state` fills in. `init_state`
    /// stays, for work that needs the created state.
    ///
    /// A panic here or in `init_state` is recorded as an `init_state` panic
    /// and drops the state: the next build calls `create_state` again, from
    /// the view the element holds then.
    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State;
}

/// Persistent state for a StatefulView.
///
/// ViewState holds mutable data that persists across rebuilds.
/// It receives lifecycle callbacks and builds the child View tree.
///
/// # Lifecycle
///
/// 0. `StatefulView::create_state()` - Creates the state, with its signals, at the first build
/// 1. `init_state()` - Called once, right after creation
/// 2. `did_change_dependencies()` - Called when inherited data changes
/// 3. `apply_view()` - Called before a build that follows a new View config or a dependency change
/// 4. `build()` - Called to create child Views (may be called many times)
/// 5. `did_update_view()` - Called when parent provides new View config
/// 6. `deactivate()` - Called when temporarily removed
/// 7. `activate()` - Called when re-inserted
/// 8. `dispose()` - Called before permanent removal
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not the state type of the view `{V}`",
    label = "missing `impl ViewState<{V}> for {Self}`",
    note = "`ViewState<V>` is implemented for the type named as `type State` in `impl StatefulView for {V}`; its `build(&self, view: &{V}, ctx: &dyn BuildContext)` returns the child view"
)]
pub trait ViewState<V: StatefulView>: 'static {
    /// Called once, right after [`StatefulView::create_state`].
    ///
    /// Use this for one-time work that needs the created state: registering
    /// it somewhere, or a post-frame callback that names it. Signals and the
    /// presentation capabilities the state holds (rebuild, focus, …) are
    /// created in `create_state`, so the state never holds an unbound handle.
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {}

    /// Called when an already-registered `InheritedView` dependency changes.
    ///
    /// This does not fire once
    /// unconditionally after `init_state`: it fires only when an `InheritedView`
    /// this state has *already* registered as a dependent of (via
    /// `ctx.depend_on()`) later notifies. A state that needs its first
    /// `depend_on`-derived value at mount time must resolve it directly in
    /// `init_state` too (see e.g. `interaction::draggable::DraggableState`,
    /// which resolves `Overlay::maybe_of` in both hooks for exactly this
    /// reason) — relying on this hook alone for the initial value silently
    /// never fires.
    fn did_change_dependencies(&mut self, _ctx: &dyn LifecycleContext) {}

    /// Publish values to the subtree under this state's element (ADR-0322 §5).
    ///
    /// Runs right before each `build`, after `init_state` and `apply_view`. A
    /// descendant reads a value with `ctx.depend_on::<T, _>(..)` and rebuilds
    /// when a later value's
    /// [`ProvidedValue::update_should_notify`](crate::ProvidedValue::update_should_notify)
    /// says so. The default publishes nothing.
    fn provide(&self, view: &V, ctx: &dyn BuildContext, values: &mut crate::Provided<'_>) {
        let _ = (view, ctx, values);
    }

    /// Applies the current view configuration and the ambient dependencies to
    /// the state, right before a build.
    ///
    /// `build` takes `&self`, so a state that has to derive something from
    /// its view (a controller serving it, a slot the child reads) cannot do
    /// it there. `apply_view` is the place: it receives the view and a
    /// [`BuildContext`] (not a [`LifecycleContext`]: the presentation
    /// capabilities stay in `init_state` and `did_change_dependencies`, ADR-0078), and runs before the first build, and before
    /// every build that follows a [`did_update_view`](Self::did_update_view)
    /// or a [`did_change_dependencies`](Self::did_change_dependencies). A
    /// rebuild that changed neither (a `set_state`) does not call it.
    ///
    /// Keep it idempotent: the same view and dependencies may reach it again
    /// after a rebuild another path requested.
    fn apply_view(&mut self, _view: &V, _ctx: &dyn BuildContext) {}

    /// Build the child View tree.
    ///
    /// Called whenever the UI needs to be rendered. Can be called many times.
    ///
    /// # Arguments
    ///
    /// * `view` - The current View configuration
    /// * `ctx` - The build context
    ///
    /// # Object safety
    ///
    /// `ViewState::build` returns `impl IntoView` (return-position
    /// `impl Trait` in trait, stabilized in Rust 1.75). This makes
    /// `ViewState` **non-object-safe** — no `dyn ViewState` use exists
    /// or is needed (FR-008).
    ///
    /// The framework normalizes the opaque return via
    /// [`IntoView::into_view`] inside the build call site (see
    /// `element/behavior.rs`), boxing the concrete `'static` value
    /// into `Box<dyn View>` before the closure / catch-unwind
    /// boundary. See `StatelessView::build` for the rationale.
    fn build(&self, view: &V, ctx: &dyn BuildContext) -> impl IntoView;

    /// Called when the View configuration changes.
    ///
    /// The Element has just swapped in `new_view` (the current configuration);
    /// `old_view` is the previous one. Compare the two to react to a changed
    /// field. The new configuration is passed explicitly as `new_view`, since
    /// FLUI state does not hold the view.
    ///
    /// An implicitly-animated widget retargets its controller here: if the
    /// animated property differs between `old_view` and `new_view`, it sets the
    /// tween's `begin` to the current displayed value, its `end` to the new
    /// target, and restarts the controller from `0`.
    fn did_update_view(&mut self, _old_view: &V, _new_view: &V) {}

    /// Called when the Element is temporarily removed from the tree.
    ///
    /// The state may be reactivated later.
    fn deactivate(&mut self) {}

    /// Called when the Element is re-inserted after deactivation.
    fn activate(&mut self) {}

    /// Called before the Element is permanently removed.
    ///
    /// Release any resources here. After this, the state will be dropped.
    fn dispose(&mut self) {}
}

// The legacy `impl_stateful_view!` declarative macro was deleted
// (FR-010 "MUST NOT be two parallel authoring paths").
// Widget authors now write `#[derive(StatefulView)]` from
// `flui-macros` instead; the derive is re-exported from
// `flui_view::prelude` for ergonomic single-import access. See
// `crates/flui-macros/src/derive_stateful.rs` for the generated
// `impl View` block this used to emit by hand-rolled `macro_rules!`.
//
// NOTE: StatefulElement implementation has been moved to unified Element
// architecture. See crates/flui-view/src/element/unified.rs and
// element/behavior.rs The type alias is exported from element/mod.rs:
//   pub type StatefulElement<V> = Element<V, Single, StatefulBehavior<V>>;
