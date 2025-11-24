//! Type-erased wrappers for view protocols.

use std::any::Any;
use std::sync::Arc;

use crate::element::{Element, IntoElement};
use crate::foundation::Listenable;
use crate::render::arity::Arity;
use crate::render::protocol::Protocol;
use crate::render::RenderObject;
use crate::view::UpdateResult;
use crate::view::{
    AnimatedView, BuildContext, ProviderView, ProxyView, RenderView, StatefulView, StatelessView,
    ViewMode, ViewObject, ViewState,
};

// ============================================================================
// STATELESS VIEW WRAPPER
// ============================================================================

pub struct StatelessViewWrapper<V: StatelessView> {
    view: Option<V>,
}

impl<V: StatelessView> StatelessViewWrapper<V> {
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

    fn did_update(&mut self, new_view: &dyn Any, _ctx: &BuildContext) {
        if let Some(new) = new_view.downcast_ref::<V>() {
            self.view = Some(new.clone());
        }
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
    pub fn new(view: V) -> Self {
        Self { view, state: None }
    }
}

impl<V, S> ViewObject for StatefulViewWrapper<V, S>
where
    V: StatefulView<S>,
    S: ViewState,
{
    fn build(&mut self, ctx: &BuildContext) -> Element {
        let state = self.state.as_mut().expect("State not initialized");
        self.view.build(state, ctx).into_element()
    }

    fn init(&mut self, ctx: &BuildContext) {
        self.state = Some(self.view.create_state());
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
            if let Some(state) = &mut self.state {
                self.view.did_update_view(&new.clone(), state, ctx);
            }
            self.view = new.clone();
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
    L: Listenable,
{
    pub fn new(view: V) -> Self {
        let listenable = view.listenable();
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
    L: Listenable + Send + 'static,
{
    fn build(&mut self, ctx: &BuildContext) -> Element {
        self.view.build(&self.listenable, ctx).into_element()
    }

    fn init(&mut self, _ctx: &BuildContext) {
        // TODO: subscribe to listenable
    }

    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}

    fn did_update(&mut self, new_view: &dyn Any, _ctx: &BuildContext) {
        if let Some(new) = new_view.downcast_ref::<V>() {
            self.view = new.clone();
            self.listenable = new.listenable();
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

pub struct ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Send + 'static,
{
    view: V,
    value: Arc<T>,
}

impl<V, T> ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Send + 'static,
{
    pub fn new(view: V) -> Self {
        let value = Arc::new(view.provide());
        Self { view, value }
    }
}

impl<V, T> ViewObject for ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    fn build(&mut self, ctx: &BuildContext) -> Element {
        self.view.build(&self.value, ctx).into_element()
    }

    fn init(&mut self, ctx: &BuildContext) {
        ctx.register_provider(&*self.value);
    }

    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}

    fn did_update(&mut self, new_view: &dyn Any, ctx: &BuildContext) {
        if let Some(new) = new_view.downcast_ref::<V>() {
            let new_value = Arc::new(new.provide());
            if !Arc::ptr_eq(&self.value, &new_value) {
                ctx.unregister_provider::<T>();
                ctx.register_provider(&*new_value);
                self.value = new_value;
            }
            self.view = new.clone();
        }
    }

    fn deactivate(&mut self, _ctx: &BuildContext) {}

    fn dispose(&mut self, ctx: &BuildContext) {
        ctx.unregister_provider::<T>();
    }

    fn mode(&self) -> ViewMode {
        ViewMode::Provider
    }

    fn as_any(&self) -> &dyn Any {
        &self.view
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.view
    }
}

// ============================================================================
// PROXY VIEW WRAPPER
// ============================================================================

pub struct ProxyViewWrapper<V: ProxyView> {
    view: V,
}

impl<V: ProxyView> ProxyViewWrapper<V> {
    pub fn new(view: V) -> Self {
        Self { view }
    }
}

impl<V: ProxyView> ViewObject for ProxyViewWrapper<V> {
    fn build(&mut self, ctx: &BuildContext) -> Element {
        self.view.build(ctx).into_element()
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
pub struct RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    view: V,
    render_object: Option<V::RenderObject>,
}

impl<V, P, A> RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    pub fn new(view: V) -> Self {
        Self {
            view,
            render_object: None,
        }
    }
}

impl<V, P, A> ViewObject for RenderViewWrapper<V, P, A>
where
    V: RenderView<P, A>,
    P: Protocol,
    A: Arity,
{
    fn build(&mut self, _ctx: &BuildContext) -> Element {
        // RenderView doesn't build children - they're managed by Element tree
        // Just return a placeholder (will be replaced by framework)
        panic!("RenderView::build should not be called - render objects don't build")
    }

    fn init(&mut self, _ctx: &BuildContext) {
        // Create render object on mount
        self.render_object = Some(self.view.create_render_object());
    }

    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}

    fn did_update(&mut self, new_view: &dyn Any, _ctx: &BuildContext) {
        if let Some(new_config) = new_view.downcast_ref::<V>() {
            self.view = new_config.clone();

            if let Some(render) = &mut self.render_object {
                let result = self.view.update_render_object(render);

                match result {
                    UpdateResult::Unchanged => {
                        #[cfg(debug_assertions)]
                        tracing::trace!("RenderView update: unchanged");
                    }
                    UpdateResult::NeedsLayout => {
                        #[cfg(debug_assertions)]
                        tracing::trace!("RenderView update: needs layout");
                        // TODO: mark element for layout
                        // This should be handled by RenderElement
                    }
                    UpdateResult::NeedsPaint => {
                        #[cfg(debug_assertions)]
                        tracing::trace!("RenderView update: needs paint");
                        // TODO: mark element for paint
                        // This should be handled by RenderElement
                    }
                }
            }
        }
    }

    fn deactivate(&mut self, _ctx: &BuildContext) {}

    fn dispose(&mut self, _ctx: &BuildContext) {
        if let Some(render) = &mut self.render_object {
            self.view.did_unmount(render);
        }
        self.render_object = None;
    }

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

    fn as_any(&self) -> &dyn Any {
        &self.view
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.view
    }

    fn render_object(&self) -> Option<&dyn RenderObject> {
        self.render_object.as_ref().map(|r| r as &dyn RenderObject)
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.render_object
            .as_mut()
            .map(|r| r as &mut dyn RenderObject)
    }
}
