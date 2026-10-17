//! [`BackdropFilter`] — filters what is already painted behind its child —
//! and [`BackdropGroup`], whose grouped filters share one backdrop.

use flui_objects::RenderBackdropFilter;
use flui_painting::paint::{BlendMode, ImageFilter};
use flui_rendering::layer::BackdropGroupKey;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::{BuildContext, BuildContextExt, StatefulView, StatelessView, ViewState};
use flui_view::{
    BoxedView, Child, InheritedView, IntoView, RenderView, View, ViewExt, impl_inherited_view,
    impl_render_view,
};

/// Filters whatever is already painted behind its child — a frosted-glass
/// pane — then paints the child on top.
///
/// The filter covers the child's laid-out box. Layout is a pass-through. With
/// [`grouped`](Self::grouped), the filter samples the backdrop shared by its
/// nearest [`BackdropGroup`] instead of the live one.
#[derive(Clone, Debug, StatelessView)]
pub struct BackdropFilter {
    filter: ImageFilter,
    blend_mode: BlendMode,
    enabled: bool,
    grouped: bool,
    child: Child,
}

impl BackdropFilter {
    /// Filters the backdrop with `filter`, composited with `SrcOver`.
    pub fn new(filter: ImageFilter) -> Self {
        Self {
            filter,
            blend_mode: BlendMode::SrcOver,
            enabled: true,
            grouped: false,
            child: Child::empty(),
        }
    }

    /// How the filtered backdrop composites. Defaults to `SrcOver`.
    #[must_use]
    pub fn blend_mode(mut self, blend_mode: BlendMode) -> Self {
        self.blend_mode = blend_mode;
        self
    }

    /// Whether the filter applies; when `false` the child paints unfiltered.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Samples the backdrop shared by the nearest [`BackdropGroup`]; without
    /// one, the filter reads the live backdrop as usual.
    #[must_use]
    pub fn grouped(mut self) -> Self {
        self.grouped = true;
        self
    }

    /// Set the child painted over the filtered backdrop.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl StatelessView for BackdropFilter {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        RawBackdropFilter {
            filter: self.filter.clone(),
            blend_mode: self.blend_mode,
            enabled: self.enabled,
            group: self.grouped.then(|| BackdropGroup::maybe_of(ctx)).flatten(),
            child: self.child.clone(),
        }
    }
}

/// The render view a [`BackdropFilter`] builds once its group is resolved.
#[derive(Clone, Debug)]
struct RawBackdropFilter {
    filter: ImageFilter,
    blend_mode: BlendMode,
    enabled: bool,
    group: Option<BackdropGroupKey>,
    child: Child,
}

impl RenderView for RawBackdropFilter {
    type Protocol = BoxProtocol;
    type RenderObject = RenderBackdropFilter;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let render = RenderBackdropFilter::new(self.filter.clone())
            .with_blend_mode(self.blend_mode)
            .with_enabled(self.enabled);
        match &self.group {
            Some(group) => render.with_group(group.clone()),
            None => render,
        }
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_filter(self.filter.clone())
            | render_object.set_blend_mode(self.blend_mode)
            | render_object.set_enabled(self.enabled)
            | render_object.set_group(self.group.clone())
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(RawBackdropFilter);

/// Lets the [`grouped`](BackdropFilter::grouped) backdrop filters below it
/// sample one shared backdrop.
///
/// On one compositing surface, the group captures the backdrop when its first
/// filter is reached; every grouped filter reads that capture. Overlapping
/// glass panes therefore never filter each other's content, and the backdrop
/// is read once. Content painted between grouped filters is not part of what
/// later ones sample. The group's identity is stable for the widget's
/// lifetime, so rebuilding it does not repaint its filters.
#[derive(Clone, Debug, StatefulView)]
pub struct BackdropGroup {
    child: BoxedView,
}

impl BackdropGroup {
    /// Groups the backdrop filters in `child`.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
        }
    }

    /// The nearest group's identity, registering a dependency on it.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<BackdropGroupKey> {
        ctx.depend_on::<BackdropGroupScope, _>(|scope| scope.key.clone())
    }
}

/// Holds the group's identity for the widget's lifetime.
#[derive(Debug)]
pub struct BackdropGroupState {
    key: BackdropGroupKey,
}

impl StatefulView for BackdropGroup {
    type State = BackdropGroupState;

    fn create_state(&self) -> Self::State {
        BackdropGroupState {
            key: BackdropGroupKey::new(),
        }
    }
}

impl ViewState<BackdropGroup> for BackdropGroupState {
    fn build(&self, view: &BackdropGroup, _ctx: &dyn BuildContext) -> impl IntoView {
        BackdropGroupScope {
            key: self.key.clone(),
            child: view.child.clone(),
        }
    }
}

/// Publishes a group's identity to its subtree.
#[derive(Clone, Debug)]
struct BackdropGroupScope {
    key: BackdropGroupKey,
    child: BoxedView,
}

impl InheritedView for BackdropGroupScope {
    type Data = BackdropGroupKey;

    fn data(&self) -> &Self::Data {
        &self.key
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.key != old.key
    }
}

impl_inherited_view!(BackdropGroupScope);
