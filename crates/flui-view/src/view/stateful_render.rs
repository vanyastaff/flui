//! A render view with persistent state (ADR-0322 §4).

use flui_rendering::RenderUpdateImpact;

use super::into_view::IntoView;
use super::render::RenderObjectContext;
use crate::context::{BuildContext, LifecycleContext};

/// A view whose element owns one render object and a state that outlives
/// rebuilds.
///
/// It is the form for a widget that would otherwise be a [`StatefulView`]
/// building exactly one [`RenderView`]: the two elements become one. The
/// view creates the render object, and the state configures it, publishes
/// values to the subtree and builds the child.
///
/// The view creates the render object because that happens at mount, before
/// `init_state` has run: nothing the state acquires there exists yet. The
/// first build runs `init_state` and then `update_render_object`, in the same
/// frame and before layout, so the object never lays out without its state's
/// configuration.
///
/// [`impl_stateful_render_view!`](crate::impl_stateful_render_view) writes
/// the `View` implementation.
///
/// [`StatefulView`]: crate::StatefulView
/// [`RenderView`]: crate::RenderView
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not implement `StatefulRenderView`",
    label = "missing `impl StatefulRenderView for {Self}`"
)]
pub trait StatefulRenderView: Clone + 'static + Sized {
    /// The layout protocol of the render object.
    type Protocol: flui_rendering::protocol::Protocol;

    /// The render object the element owns.
    type RenderObject: flui_rendering::traits::RenderObject<Self::Protocol> + 'static;

    /// The state the element keeps across rebuilds.
    type State: RenderViewState<Self>;

    /// Creates the state, once, at the element's first build and right
    /// before `init_state`, from the view the element holds then. `cx` is
    /// the context `init_state` receives: the state's signals and the
    /// capabilities it keeps are created here, as in
    /// [`StatefulView::create_state`](crate::StatefulView::create_state). A
    /// panic here or in `init_state` drops the state and releases its
    /// signals; the next build creates it again.
    fn create_state(&self, cx: &dyn LifecycleContext) -> Self::State;

    /// Creates the render object, once, when the element mounts.
    fn create_render_object(&self, cx: &RenderObjectContext<'_>) -> Self::RenderObject;

    /// Releases what the render object holds in the owner's runtime, such as
    /// the targets registered for it, before it is removed from the render
    /// tree. It runs once, after the state's `dispose`, and also for an
    /// element whose state never initialized: what `create_render_object`
    /// registered is released either way.
    fn did_unmount_render_object(
        &self,
        _cx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
    }
}

/// The state of a [`StatefulRenderView`].
///
/// # Order of the hooks
///
/// 1. `create_state` and `init_state`, once, at the first build, from the
///    view the element holds then.
/// 2. At every build: `update_render_object`, `provide`, `child`.
/// 3. `did_update_view` when the parent gives a new view, and
///    `did_change_dependencies` when an inherited value read in a lifecycle
///    hook changed; each is followed by a build.
/// 4. `deactivate` and `activate` around a move by `GlobalKey`.
/// 5. `dispose`, once, before the render object is released and removed, and only if
///    `init_state` ran.
///
/// A change of the state that the render object must see schedules the
/// element through the rebuild handle taken in `init_state`. A change that
/// only repaints goes to the render object directly, through a handle the
/// state gave it in `update_render_object`, and schedules no element.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not the state type of the render view `{V}`",
    label = "missing `impl RenderViewState<{V}> for {Self}`"
)]
pub trait RenderViewState<V: StatefulRenderView>: 'static {
    /// Called once, at the element's first build. Presentation capabilities
    /// are acquired here or in `did_change_dependencies` (ADR-0078).
    fn init_state(&mut self, _cx: &dyn LifecycleContext) {}

    /// Called when an inherited value this state read in a lifecycle hook
    /// changed.
    fn did_change_dependencies(&mut self, _cx: &dyn LifecycleContext) {}

    /// Called when the parent replaced the view.
    fn did_update_view(&mut self, _old: &V, _new: &V) {}

    /// Pushes the view's and the state's configuration into the render
    /// object, and returns what the change invalidates.
    fn update_render_object(
        &self,
        view: &V,
        cx: &RenderObjectContext<'_>,
        render_object: &mut V::RenderObject,
    ) -> RenderUpdateImpact;

    /// Publishes values to the subtree, as [`ViewState::provide`] does.
    ///
    /// [`ViewState::provide`]: crate::ViewState::provide
    fn provide(&self, view: &V, cx: &dyn BuildContext, values: &mut crate::Provided<'_>) {
        let _ = (view, cx, values);
    }

    /// Builds the render object's child.
    fn child(&self, view: &V, cx: &dyn BuildContext) -> impl IntoView;

    /// Called when the element leaves the tree and may return in the same
    /// frame.
    fn deactivate(&mut self) {}

    /// Called when a deactivated element returns to the tree.
    fn activate(&mut self) {}

    /// Called once before the element is removed for good.
    fn dispose(&mut self) {}
}

/// Implements `View` for a [`StatefulRenderView`].
///
/// ```rust,ignore
/// impl_stateful_render_view!(MyControl);
///
/// // A view with a `CreationSite` field reports it through `View::creation_site`:
/// impl_stateful_render_view!(MyControl, site = site);
/// ```
#[macro_export]
macro_rules! impl_stateful_render_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> $crate::element::ElementKind {
                $crate::element::ElementKind::stateful_render(self)
            }
        }
    };
    ($ty:ty, site = $field:ident) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> $crate::element::ElementKind {
                $crate::element::ElementKind::stateful_render(self)
            }

            fn creation_site(&self) -> $crate::CreationSite {
                self.$field
            }
        }
    };
}
