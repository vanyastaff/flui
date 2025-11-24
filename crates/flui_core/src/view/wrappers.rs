//! Type-erased wrappers for view protocols.

use std::any::Any;
use std::sync::Arc;

use crate::element::{Element, ElementId, IntoElement};
use crate::foundation::Listenable;
use crate::render::arity::Arity;
use crate::render::protocol::Protocol;
use crate::render::{LayoutProtocol, RenderObject, RenderState, RuntimeArity};
use crate::view::UpdateResult;
use crate::view::{
    AnimatedView, BuildContext, ProviderView, ProxyView, RenderView, StatefulView, StatelessView,
    ViewMode, ViewObject, ViewState,
};

// ============================================================================
// STATELESS VIEW WRAPPER
// ============================================================================

/// Wrapper for stateless views that implements ViewObject.
#[derive(Debug)]
pub struct StatelessViewWrapper<V: StatelessView> {
    view: Option<V>,
}

impl<V: StatelessView> StatelessViewWrapper<V> {
    /// Creates a new wrapper with the given view.
    pub fn new(view: V) -> Self {
        Self { view: Some(view) }
    }
}

impl<V: StatelessView> ViewObject for StatelessViewWrapper<V> {
    fn build(&mut self, ctx: &BuildContext) -> Element {
        // StatelessView is consumed on build
        let view = self.view.take().expect("StatelessView already consumed");
        view.build(ctx).into_element()
    }

    fn init(&mut self, _ctx: &BuildContext) {}

    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}

    fn did_update(&mut self, _new_view: &dyn Any, _ctx: &BuildContext) {
        // StatelessView is consumed on build and cannot be updated.
        // A new wrapper will be created with the new view.
    }

    fn deactivate(&mut self, _ctx: &BuildContext) {}

    fn dispose(&mut self, _ctx: &BuildContext) {}

    fn mode(&self) -> ViewMode {
        ViewMode::Stateless
    }

    fn as_any(&self) -> &dyn Any {
        self.view.as_ref().expect("StatelessView not available")
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.view.as_mut().expect("StatelessView not available")
    }
}

// ============================================================================
// STATEFUL VIEW WRAPPER
// ============================================================================

/// Wrapper for stateful views that implements ViewObject.
#[derive(Debug)]
pub struct StatefulViewWrapper<V, S>
where
    V: StatefulView<S>,
    S: ViewState,
{
    view: V,
    state: Option<S>,
}

impl<V, S> StatefulViewWrapper<V, S>
where
    V: StatefulView<S>,
    S: ViewState,
{
    /// Creates a new wrapper with the given view.
    pub fn new(view: V) -> Self {
        Self { view, state: None }
    }
}

impl<V, S> ViewObject for StatefulViewWrapper<V, S>
where
    V: StatefulView<S>,
    S: ViewState + Default,
{
    fn build(&mut self, ctx: &BuildContext) -> Element {
        let state = self.state.as_mut().expect("State not initialized");
        self.view.build(state, ctx).into_element()
    }

    fn init(&mut self, ctx: &BuildContext) {
        // For now, require Default. Later we can add create_state to trait
        self.state = Some(Default::default());
        if let Some(state) = &mut self.state {
            self.view.init_state(state, ctx);
        }
    }

    fn did_change_dependencies(&mut self, ctx: &BuildContext) {
        if let Some(state) = &mut self.state {
            self.view.did_change_dependencies(state, ctx);
        }
    }

    fn did_update(&mut self, new_view: &dyn Any, ctx: &BuildContext) {
        if let Some(new) = new_view.downcast_ref::<V>() {
            self.view = new.clone();
            if let Some(state) = &mut self.state {
                self.view.did_update(state, ctx);
            }
        }
    }

    fn deactivate(&mut self, ctx: &BuildContext) {
        if let Some(state) = &mut self.state {
            self.view.deactivate(state, ctx);
        }
    }

    fn dispose(&mut self, ctx: &BuildContext) {
        if let Some(state) = &mut self.state {
            self.view.dispose(state, ctx);
        }
    }

    fn mode(&self) -> ViewMode {
        ViewMode::Stateful
    }

    fn as_any(&self) -> &dyn Any {
        &self.view
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.view
    }
}

// ============================================================================
// ANIMATED VIEW WRAPPER
// ============================================================================

/// Wrapper for animated views that implements ViewObject.
#[derive(Debug)]
pub struct AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    view: V,
    listenable: L,
    _subscription: Option<()>, // TODO: proper subscription
}

impl<V, L> AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable + Clone,
{
    /// Creates a new wrapper with the given view.
    pub fn new(view: V) -> Self {
        let listenable = view.listenable().clone();
        Self {
            view,
            listenable,
            _subscription: None,
        }
    }
}

impl<V, L> ViewObject for AnimatedViewWrapper<V, L>
where
    V: AnimatedView<L>,
    L: Listenable + Clone + Send + 'static,
{
    fn build(&mut self, ctx: &BuildContext) -> Element {
        self.view.build(ctx).into_element()
    }

    fn init(&mut self, _ctx: &BuildContext) {
        // TODO: subscribe to listenable
    }

    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}

    fn did_update(&mut self, new_view: &dyn Any, _ctx: &BuildContext) {
        if let Some(new) = new_view.downcast_ref::<V>() {
            self.view = new.clone();
            self.listenable = new.listenable().clone();
            // TODO: resubscribe
        }
    }

    fn deactivate(&mut self, _ctx: &BuildContext) {}

    fn dispose(&mut self, _ctx: &BuildContext) {
        // TODO: unsubscribe
    }

    fn mode(&self) -> ViewMode {
        ViewMode::Animated
    }

    fn as_any(&self) -> &dyn Any {
        &self.view
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.view
    }
}

// ============================================================================
// PROVIDER VIEW WRAPPER
// ============================================================================

/// Wrapper for ProviderView implementations.
///
/// Stores the view configuration, provided value, and dependent elements.
#[derive(Debug)]
pub struct ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Send + 'static,
{
    /// View configuration
    view: V,

    /// Provided value (shared with dependents)
    value: Arc<T>,

    /// Elements that depend on this provider
    dependents: Vec<ElementId>,
}

impl<V, T> ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Clone + Send + 'static,
{
    /// Creates a new wrapper with the given view.
    pub fn new(view: V) -> Self {
        let value = Arc::new(view.value().clone());
        Self {
            view,
            value,
            dependents: Vec::new(),
        }
    }

    /// Returns reference to the provided value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Adds a dependent element.
    pub fn add_dependent(&mut self, element_id: ElementId) {
        if !self.dependents.contains(&element_id) {
            self.dependents.push(element_id);
        }
    }

    /// Removes a dependent element.
    pub fn remove_dependent(&mut self, element_id: ElementId) {
        self.dependents.retain(|&id| id != element_id);
    }

    /// Returns the number of dependents.
    pub fn dependent_count(&self) -> usize {
        self.dependents.len()
    }
}

impl<V, T> ViewObject for ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Clone + Send + Sync + 'static,
{
    fn mode(&self) -> ViewMode {
        ViewMode::Provider
    }

    fn build(&mut self, ctx: &BuildContext) -> Element {
        self.view.build(ctx).into_element()
    }

    fn init(&mut self, _ctx: &BuildContext) {
        #[cfg(debug_assertions)]
        tracing::trace!(
            "ProviderViewWrapper::init - type: {}",
            std::any::type_name::<T>()
        );
    }

    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}

    fn did_update(&mut self, new_view: &dyn Any, _ctx: &BuildContext) {
        if let Some(new) = new_view.downcast_ref::<V>() {
            let new_value = Arc::new(new.value().clone());
            let value_changed = !Arc::ptr_eq(&self.value, &new_value);

            if value_changed {
                self.value = new_value;

                #[cfg(debug_assertions)]
                tracing::trace!(
                    "ProviderViewWrapper: value changed, {} dependents to notify",
                    self.dependents.len()
                );
            }
            self.view = new.clone();
        }
    }

    fn deactivate(&mut self, _ctx: &BuildContext) {}

    fn dispose(&mut self, _ctx: &BuildContext) {
        self.dependents.clear();
    }

    fn as_any(&self) -> &dyn Any {
        &self.view
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.view
    }

    // ========== PROVIDER-SPECIFIC IMPLEMENTATIONS ==========

    fn provided_value(&self) -> Option<&(dyn Any + Send + Sync)> {
        Some(&*self.value as &(dyn Any + Send + Sync))
    }

    fn dependents(&self) -> Option<&[ElementId]> {
        Some(&self.dependents)
    }

    fn dependents_mut(&mut self) -> Option<&mut Vec<ElementId>> {
        Some(&mut self.dependents)
    }

    fn should_notify_dependents(&self, old_value: &dyn Any) -> bool {
        if let Some(old_view) = old_value.downcast_ref::<V>() {
            self.view.should_notify(old_view)
        } else {
            true
        }
    }
}

// ============================================================================
// PROXY VIEW WRAPPER
// ============================================================================

/// Wrapper for proxy views that implements ViewObject.
#[derive(Debug)]
pub struct ProxyViewWrapper<V: ProxyView> {
    view: V,
}

impl<V: ProxyView> ProxyViewWrapper<V> {
    /// Creates a new wrapper with the given view.
    pub fn new(view: V) -> Self {
        Self { view }
    }
}

impl<V: ProxyView> ViewObject for ProxyViewWrapper<V> {
    fn build(&mut self, ctx: &BuildContext) -> Element {
        self.view.before_child_build(ctx);
        let child = self.view.build_child(ctx).into_element();
        self.view.after_child_build(ctx);
        child
    }

    fn init(&mut self, _ctx: &BuildContext) {}

    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}

    fn did_update(&mut self, new_view: &dyn Any, _ctx: &BuildContext) {
        if let Some(new) = new_view.downcast_ref::<V>() {
            self.view = new.clone();
        }
    }

    fn deactivate(&mut self, _ctx: &BuildContext) {}

    fn dispose(&mut self, _ctx: &BuildContext) {}

    fn mode(&self) -> ViewMode {
        ViewMode::Proxy
    }

    fn as_any(&self) -> &dyn Any {
        &self.view
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.view
    }
}

// ============================================================================
// RENDER VIEW WRAPPER
// ============================================================================

/// Wrapper for RenderView implementations.
///
/// Stores the view configuration AND the created render object + state.
/// This enables unified Element architecture where all type-specific
/// behavior is delegated to ViewObject.
#[derive(Debug)]
pub struct RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    /// View configuration (immutable description)
    view: V,

    /// Created render object (persists across rebuilds)
    render_object: Option<V::RenderObject>,

    /// Render state (size, offset, dirty flags)
    render_state: RenderState,

    /// Layout protocol (Box or Sliver)
    protocol: LayoutProtocol,

    /// Arity specification
    arity: RuntimeArity,
}

impl<V, P, A> RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    /// Creates a new wrapper with the given view.
    pub fn new(view: V) -> Self {
        Self {
            view,
            render_object: None,
            render_state: RenderState::new(),
            protocol: P::ID,
            arity: A::runtime_arity(),
        }
    }

    /// Returns reference to the view configuration.
    pub fn view(&self) -> &V {
        &self.view
    }

    /// Returns mutable reference to the view configuration.
    pub fn view_mut(&mut self) -> &mut V {
        &mut self.view
    }
}

impl<V, P, A> ViewObject for RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    V::RenderObject: RenderObject,
    P: Protocol,
    A: Arity,
{
    fn mode(&self) -> ViewMode {
        // Determine mode from protocol
        if std::any::TypeId::of::<P>()
            == std::any::TypeId::of::<crate::render::protocol::BoxProtocol>()
        {
            ViewMode::RenderBox
        } else {
            ViewMode::RenderSliver
        }
    }

    fn build(&mut self, _ctx: &BuildContext) -> Element {
        panic!("RenderViewWrapper::build() should not be called - RenderViews create RenderObjects, not child Elements")
    }

    fn init(&mut self, _ctx: &BuildContext) {
        // Create render object on mount (only once!)
        if self.render_object.is_none() {
            self.render_object = Some(self.view.create());

            #[cfg(debug_assertions)]
            tracing::trace!(
                "RenderViewWrapper::init - created render object: {:?}",
                std::any::type_name::<V::RenderObject>()
            );
        }
    }

    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}

    fn did_update(&mut self, new_view: &dyn Any, _ctx: &BuildContext) {
        if let Some(new_config) = new_view.downcast_ref::<V>() {
            // Update view configuration
            self.view = new_config.clone();

            // Update existing render object
            if let Some(render) = &mut self.render_object {
                let result = self.view.update(render);

                match result {
                    UpdateResult::Unchanged => {
                        #[cfg(debug_assertions)]
                        tracing::trace!("RenderView update: unchanged");
                    }
                    UpdateResult::NeedsLayout => {
                        #[cfg(debug_assertions)]
                        tracing::trace!("RenderView update: needs layout");
                        self.render_state.mark_needs_layout();
                    }
                    UpdateResult::NeedsPaint => {
                        #[cfg(debug_assertions)]
                        tracing::trace!("RenderView update: needs paint");
                        self.render_state.mark_needs_paint();
                    }
                }
            }
        }
    }

    fn deactivate(&mut self, _ctx: &BuildContext) {}

    fn dispose(&mut self, _ctx: &BuildContext) {
        if let Some(render) = &mut self.render_object {
            self.view.dispose(render);
        }
        self.render_object = None;
    }

    fn as_any(&self) -> &dyn Any {
        &self.view
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.view
    }

    // ========== RENDER-SPECIFIC IMPLEMENTATIONS ==========

    fn render_object(&self) -> Option<&dyn RenderObject> {
        self.render_object.as_ref().map(|r| r as &dyn RenderObject)
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.render_object
            .as_mut()
            .map(|r| r as &mut dyn RenderObject)
    }

    fn render_state(&self) -> Option<&RenderState> {
        Some(&self.render_state)
    }

    fn render_state_mut(&mut self) -> Option<&mut RenderState> {
        Some(&mut self.render_state)
    }

    fn protocol(&self) -> Option<LayoutProtocol> {
        Some(self.protocol)
    }

    fn arity(&self) -> Option<RuntimeArity> {
        Some(self.arity)
    }
}
