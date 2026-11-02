//! Touch handles: what they look like ([`SelectionHandles`], [`DotHandles`],
//! [`DefaultSelectionHandles`]) and how a [`SelectionArea`](super::SelectionArea)
//! shows them, in its nearest [`Overlay`](crate::Overlay), each pinned under one end of the
//! selection by a layer link the paragraph holding that end publishes.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::Offset;
use flui_foundation::notifier::Listenable;
use flui_foundation::panic::FirstPanic;
use flui_interaction::GestureEndReason;
use flui_objects::RenderFollowerLayer;
use flui_painting::BoxShape;
use flui_painting::paint::Alignment;
use flui_painting::styling::{BoxDecoration, Color};
use flui_painting::typography::TextDirection;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::layer::{LayerLink, LeaderClip};
use flui_rendering::pipeline::LeaderWatch;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{
    BoxedView, Child, InheritedView, RenderView, View, ViewExt, impl_inherited_view,
    impl_render_view,
};

use super::SelectionModel;
use crate::interaction::{DragSlot, GestureDetector};
use crate::overlay::{InsertPosition, OverlayEntry, OverlayHandle};
use crate::text::editable_text::CaretHandle;
use crate::{Container, Positioned, Stack};

/// Which end of a selection a handle moves, in reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HandleSide {
    /// The end a reader meets first.
    Start,
    /// The end a reader meets last.
    End,
    /// The caret of an editable text with nothing selected, which the
    /// handle moves.
    Collapsed,
}

/// What a [`SelectionHandles`] builds one handle for.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct HandleInfo {
    /// The end the handle moves.
    pub side: HandleSide,
    /// The direction of the text the area reads in.
    pub text_direction: TextDirection,
    /// The height of the line the end sits on, in logical pixels.
    pub line_height: f64,
}

/// Builds the touch handles of a [`SelectionArea`](super::SelectionArea).
///
/// The handle is laid out under the end's line, its top centre at the
/// bottom of that line where the end sits; the laid-out box is what takes
/// the touch, so a handle that wants a larger target pads itself.
///
/// # Examples
///
/// ```rust
/// # use flui_widgets::prelude::*;
/// # use flui_painting::styling::Color;
/// let _ = SelectionArea::new(Text::new("Terms")).handles(DotHandles::new(Color::rgb(0, 120, 215)));
/// ```
pub trait SelectionHandles: 'static {
    /// The handle for `info`'s end.
    fn build(&self, info: &HandleInfo, ctx: &dyn BuildContext) -> BoxedView;
}

/// Round handles in one colour, the default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DotHandles {
    color: Color,
}

impl DotHandles {
    /// The diameter of the dot, a comfortable touch target.
    const DIAMETER: f64 = 22.0;

    /// Dots in `color`.
    #[must_use]
    pub const fn new(color: Color) -> Self {
        Self { color }
    }
}

impl SelectionHandles for DotHandles {
    fn build(&self, _info: &HandleInfo, _ctx: &dyn BuildContext) -> BoxedView {
        Container::new()
            .width(Self::DIAMETER)
            .height(Self::DIAMETER)
            .decoration(BoxDecoration::with_color(self.color).set_shape(BoxShape::Circle))
            .boxed()
    }
}

/// Sets the [`SelectionHandles`] of the selection areas and editable texts
/// beneath it that set none of their own, and when a tap in those texts
/// shows a caret handle.
#[derive(Clone)]
pub struct DefaultSelectionHandles {
    handles: Rc<dyn SelectionHandles>,
    caret_handle: Option<CaretHandle>,
    child: BoxedView,
}

impl std::fmt::Debug for DefaultSelectionHandles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultSelectionHandles")
            .field("caret_handle", &self.caret_handle)
            .field("child", &self.child)
            .finish_non_exhaustive()
    }
}

impl DefaultSelectionHandles {
    /// `handles` for the areas and fields in `child`.
    pub fn new(handles: impl SelectionHandles, child: impl IntoView) -> Self {
        Self {
            handles: Rc::new(handles),
            caret_handle: None,
            child: child.into_view().boxed(),
        }
    }

    /// Also set when a tap in an editable text beneath shows a caret
    /// handle, in place of the platform's ([`CaretHandle::default`]).
    #[must_use]
    pub fn caret_handle(mut self, caret_handle: CaretHandle) -> Self {
        self.caret_handle = Some(caret_handle);
        self
    }

    /// The handles the nearest `DefaultSelectionHandles` above `ctx` sets,
    /// registering a dependency on it.
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Rc<dyn SelectionHandles>> {
        ctx.depend_on::<Self, _>(|scope| Rc::clone(&scope.handles))
    }

    /// The caret handle the nearest `DefaultSelectionHandles` above `ctx`
    /// sets, if it sets one, registering a dependency on it.
    pub fn caret_handle_of(ctx: &dyn BuildContext) -> Option<CaretHandle> {
        ctx.depend_on::<Self, _>(|scope| scope.caret_handle)
            .flatten()
    }
}

impl InheritedView for DefaultSelectionHandles {
    type Data = Rc<dyn SelectionHandles>;

    fn data(&self) -> &Self::Data {
        &self.handles
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.handles, &old.handles) || self.caret_handle != old.caret_handle
    }
}

impl_inherited_view!(DefaultSelectionHandles);

/// What an area's handles look like: handles it was given, its own or a
/// default's, else dots in its active colour.
#[derive(Clone)]
pub(crate) enum Look {
    /// Handles given to the area or to a `DefaultSelectionHandles` above it.
    Given(Rc<dyn SelectionHandles>),
    /// The dots an area without given handles draws.
    Dots(DotHandles),
}

impl Look {
    pub(crate) fn handles(&self) -> &dyn SelectionHandles {
        match self {
            Self::Given(handles) => handles.as_ref(),
            Self::Dots(dots) => dots,
        }
    }

    /// Whether both build the same handles: the same given handles, or
    /// dots in one colour.
    pub(crate) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Given(a), Self::Given(b)) => Rc::ptr_eq(a, b),
            (Self::Dots(a), Self::Dots(b)) => a == b,
            _ => false,
        }
    }
}

/// The area's handles: whether they are shown, the links each end is
/// published under, and the overlay entry that holds them while shown.
pub(super) struct HandleState {
    /// Whether a touch selection shows the handles.
    pub(super) shown: Cell<bool>,
    /// The links the start and the end are published under.
    pub(super) links: [LayerLink; 2],
    /// The overlay the handles go in, acquired with the area's
    /// dependencies.
    pub(super) overlay: RefCell<Option<OverlayHandle>>,
    /// The entry holding the handles while they are shown.
    entry: RefCell<Option<OverlayEntry>>,
    /// What the handles look like, resolved at the area's last build.
    look: RefCell<Option<Look>>,
    /// Watches on the two ends' leaders: a leader's size is its end's line,
    /// so a change rebuilds shown handles, a relayout without a build too.
    watches: RefCell<Vec<LeaderWatch>>,
    /// Where a handle drag holds the end, relative to the contact.
    pub(super) grip: Cell<Option<(HandleSide, Offset<f64>)>>,
    /// Whether the missing overlay was already reported.
    warned: Cell<bool>,
}

impl Default for HandleState {
    fn default() -> Self {
        Self {
            shown: Cell::new(false),
            links: [LayerLink::new(), LayerLink::new()],
            overlay: RefCell::new(None),
            entry: RefCell::new(None),
            look: RefCell::new(None),
            watches: RefCell::new(Vec::new()),
            grip: Cell::new(None),
            warned: Cell::new(false),
        }
    }
}

impl HandleState {
    /// The link `side`'s end is published under.
    pub(super) fn link(&self, side: HandleSide) -> LayerLink {
        match side {
            HandleSide::Start | HandleSide::Collapsed => self.links[0],
            HandleSide::End => self.links[1],
        }
    }

    /// Puts the handles in the overlay, once; without an overlay there are
    /// none, which a debug build reports once. Returns whether they are
    /// shown. The entry is held before the overlay is told, so a wake that
    /// fails there (held in `failure`) leaves an entry `hide` removes.
    pub(super) fn show(&self, model: &Rc<SelectionModel>, failure: &mut FirstPanic) -> bool {
        let overlay = self.overlay.borrow().clone();
        let Some(overlay) = overlay else {
            if cfg!(debug_assertions) && !self.warned.replace(true) {
                tracing::warn!("a SelectionArea shows no touch handles: no Overlay is above it");
            }
            return false;
        };
        self.shown.set(true);
        if self.entry.borrow().is_none() {
            let model = Rc::downgrade(model);
            let entry = OverlayEntry::new(move |_ctx| {
                HandlesView {
                    model: model.clone(),
                }
                .boxed()
            });
            *self.entry.borrow_mut() = Some(entry.clone());
            failure.contain(|| overlay.insert(&entry, &InsertPosition::Top));
        }
        true
    }

    /// Takes the handles out of the overlay. Returns whether they were
    /// shown. They are hidden before the overlay is told, so a wake that
    /// fails there (held in `failure`) leaves nothing shown.
    pub(super) fn hide(&self, failure: &mut FirstPanic) -> bool {
        self.grip.set(None);
        let was_shown = self.shown.replace(false);
        let entry = self.entry.borrow_mut().take();
        if let Some(entry) = entry {
            failure.contain(|| entry.remove());
        }
        was_shown
    }

    /// Sets what the handles look like, rebuilding shown handles when it
    /// changed.
    pub(super) fn set_look(&self, look: Look) {
        let old = self.look.borrow_mut().replace(look.clone());
        if !old.is_some_and(|old| old.same(&look)) {
            let mut failure = FirstPanic::new();
            self.refresh(&mut failure);
            failure.resume();
        }
    }

    /// Rebuilds the handles, whose line height may have changed; a wake
    /// that fails is held in `failure`.
    pub(super) fn refresh(&self, failure: &mut FirstPanic) {
        let entry = self.entry.borrow().clone();
        if let Some(entry) = entry {
            failure.contain(|| entry.mark_needs_build());
        }
    }
}

/// The two handles, each following its end.
#[derive(Clone)]
struct HandlesView {
    model: Weak<SelectionModel>,
}

impl std::fmt::Debug for HandlesView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlesView").finish_non_exhaustive()
    }
}

impl View for HandlesView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for HandlesView {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let handles: Vec<BoxedView> = self
            .model
            .upgrade()
            .map(|model| {
                let heights = model.end_line_heights();
                [HandleSide::Start, HandleSide::End]
                    .into_iter()
                    .zip(heights)
                    .filter_map(|(side, height)| handle(&model, side, height?, ctx))
                    .collect()
            })
            .unwrap_or_default();
        Stack::new(handles)
    }
}

impl SelectionModel {
    /// The heights of the lines the start and the end sit on, each `None`
    /// while its end is nowhere laid out.
    fn end_line_heights(&self) -> [Option<f64>; 2] {
        [
            self.end_line_height(HandleSide::Start),
            self.end_line_height(HandleSide::End),
        ]
    }

    /// Watches the two ends' leaders, once: a leader's size is its end's
    /// line, so shown handles rebuild when a line changes, a relayout no
    /// build caused too. A change is heard after the frame that painted it,
    /// and the handles rebuild in the next. Without a pipeline or a
    /// post-frame lane the handles keep the lines they were built for.
    pub(super) fn watch_handle_lines(self: &Rc<Self>) {
        let pipeline = self.pipeline.borrow().clone();
        let post_frame = self.post_frame.borrow().clone();
        let (Some(pipeline), Some(post_frame)) = (pipeline, post_frame) else {
            return;
        };
        let mut watches = self.handles.watches.borrow_mut();
        if !watches.is_empty() {
            return;
        }
        for link in self.handles.links {
            let watch = pipeline.watch_leader(link, post_frame.clone());
            let model = Rc::downgrade(self);
            watch.add_listener(Rc::new(move || {
                let Some(model) = Weak::upgrade(&model) else {
                    return;
                };
                if model.handles.shown.get() {
                    let mut failure = FirstPanic::new();
                    model.handles.refresh(&mut failure);
                    failure.resume();
                }
            }));
            watches.push(watch);
        }
    }
}

/// `side`'s handle, under its end on a line `line_height` tall, which a
/// drag moves.
fn handle(
    model: &Rc<SelectionModel>,
    side: HandleSide,
    line_height: f64,
    ctx: &dyn BuildContext,
) -> Option<BoxedView> {
    let look = model.handles.look.borrow().clone()?;
    let info = HandleInfo {
        side,
        text_direction: model.text_direction(),
        line_height,
    };
    let (start, update, end) = (
        Rc::downgrade(model),
        Rc::downgrade(model),
        Rc::downgrade(model),
    );
    let dragged = GestureDetector::new()
        .behavior(HitTestBehavior::Opaque)
        .drag(
            DragSlot::pan()
                .on_start(move |_cx, details| {
                    if let Some(model) = start.upgrade() {
                        model.handle_drag_started(side, details.global_position);
                    }
                })
                .on_update(move |_cx, details| {
                    if let Some(model) = update.upgrade() {
                        model.handle_dragged(side, details.global_position);
                    }
                })
                .on_end(move |_cx, details| {
                    if let Some(model) = end.upgrade() {
                        // A release ends where the finger lifted, which no
                        // update may have reported yet.
                        if details.reason == GestureEndReason::Completed {
                            model.handle_dragged(side, details.global_position);
                        }
                        // A drag of the other handle that began since holds
                        // the grip now.
                        if model
                            .handles
                            .grip
                            .get()
                            .is_some_and(|(gripped, _)| gripped == side)
                        {
                            model.handles.grip.set(None);
                        }
                    }
                }),
        )
        .child(look.handles().build(&info, ctx));
    Some(
        Positioned::new(LinkFollower::new(model.handles.link(side), dragged))
            .left(0.0)
            .top(0.0)
            .boxed(),
    )
}

/// Lays its child out where the leader under `link` is painted: the
/// child's top centre at the leader's bottom-left corner, which a
/// paragraph puts at the bottom of an end's line. Nothing paints or takes
/// a touch while no leader is published, or while its end is clipped out
/// (scrolled out of a viewport).
#[derive(Debug, Clone)]
pub(crate) struct LinkFollower {
    link: LayerLink,
    child: Child,
}

impl LinkFollower {
    pub(crate) fn new(link: LayerLink, child: impl IntoView) -> Self {
        Self {
            link,
            child: Child::some(child.into_view()),
        }
    }
}

impl RenderView for LinkFollower {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFollowerLayer;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderFollowerLayer::new(self.link)
            .with_show_when_unlinked(false)
            .with_leader_anchor(Alignment::BOTTOM_LEFT)
            .with_follower_anchor(Alignment::TOP_CENTER)
            .with_leader_clip(LeaderClip::Hide)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_link(self.link)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(LinkFollower);
