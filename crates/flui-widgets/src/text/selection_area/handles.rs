//! Touch handles: what they look like ([`SelectionHandles`], [`DotHandles`])
//! and how a [`SelectionArea`](super::SelectionArea)
//! shows them, in its nearest [`Overlay`](crate::Overlay), each pinned under one end of the
//! selection by a layer link the paragraph holding that end publishes.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::geometry::Offset;
use flui_foundation::notifier::Listenable;
use flui_foundation::panic::FirstPanic;
use flui_interaction::{GestureEndReason, HapticsHandle};
use flui_objects::RenderFollowerLayer;
use flui_painting::BoxShape;
use flui_painting::paint::Alignment;
use flui_painting::styling::{BoxDecoration, Color};
use flui_painting::typography::TextDirection;
use flui_platform::HapticFeedback;
use flui_platform::haptics::{HapticRequest, HapticTiming};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::layer::{LayerLink, LeaderClip};
use flui_rendering::pipeline::LeaderWatch;
use flui_rendering::protocol::BoxProtocol;
use flui_view::prelude::*;
use flui_view::{BoxedView, Child, RenderView, View, ViewExt, impl_render_view};

use super::SelectionModel;
use crate::interaction::{DragSlot, GestureDetector};
use crate::overlay::{InsertPosition, OverlayEntry, OverlayHandle};
use crate::text::{MagnifierInfo, MenuHost, SelectionMagnifier};
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

/// What an area's handles look like: handles it was given, its own or a
/// default's, else dots in its active colour.
#[derive(Clone)]
pub(crate) enum Look {
    /// Handles given to the area or to a `DefaultSelectionControls` above it.
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
    watches: RefCell<Vec<(LeaderWatch, flui_foundation::Subscription)>>,
    /// Where a handle drag holds the end, relative to the contact.
    pub(super) grip: Cell<Option<(HandleSide, Offset<f64>)>>,
    /// What a handle drag shows over the dragged end, resolved at the
    /// area's last build.
    magnifier: RefCell<Option<Rc<dyn SelectionMagnifier>>>,
    /// How far along the line from the dragged end the drag's magnifier
    /// sits: where the finger holds the end, less where the end is.
    pub(super) magnifier_along: Cell<f64>,
    /// What a handle drag plays a step over a character with, acquired
    /// with the area's dependencies.
    pub(super) haptics: RefCell<Option<HapticsHandle>>,
    /// The area's selection menu, whose toolbar shows over shown handles.
    pub(super) menu: Rc<MenuHost>,
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
            magnifier: RefCell::new(None),
            magnifier_along: Cell::new(0.0),
            haptics: RefCell::new(None),
            menu: Rc::new(MenuHost::default()),
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
        self.menu.stop_toolbar();
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

    /// Rebuilds the handles when a drag shows a magnifier, which comes or
    /// goes with the drag; a wake that fails is held in `failure`.
    pub(super) fn refresh_magnifier(&self, failure: &mut FirstPanic) {
        if self.magnifier.borrow().is_some() {
            self.refresh(failure);
        }
    }

    /// Sets what a handle drag shows over its end, rebuilding the handles
    /// when a drag shows another one now.
    pub(super) fn set_magnifier(&self, magnifier: Option<Rc<dyn SelectionMagnifier>>) {
        let old = self.magnifier.replace(magnifier);
        let same = match (&old, &*self.magnifier.borrow()) {
            (Some(old), Some(new)) => Rc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        };
        if !same && self.grip.get().is_some() {
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
                    .chain(drag_magnifier(&model, ctx))
                    .chain(
                        model
                            .handles
                            .menu
                            .toolbar_shown()
                            .then(|| super::menu::toolbar(&model))
                            .flatten(),
                    )
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
        for (index, link) in self.handles.links.into_iter().enumerate() {
            let watch = pipeline.watch_leader(link, post_frame.clone());
            let model = Rc::downgrade(self);
            // The handles follow a move by themselves; only a new line
            // height changes how they look.
            let size = Cell::new(watch.geometry().map(|geometry| geometry.size));
            let listening = watch.listen(Rc::new(move || {
                let Some(model) = Weak::upgrade(&model) else {
                    return;
                };
                let now = model
                    .handles
                    .watches
                    .borrow()
                    .get(index)
                    .and_then(|(watch, _)| watch.geometry())
                    .map(|geometry| geometry.size);
                // The toolbar is placed against the overlay, so a move
                // places it again.
                let resized = size.replace(now) != now;
                if model.handles.shown.get() && (resized || model.handles.menu.toolbar_shown()) {
                    let mut failure = FirstPanic::new();
                    model.handles.refresh(&mut failure);
                    failure.resume();
                }
            }));
            watches.push((watch, listening));
        }
    }
}

/// The magnifier over the end a handle drag holds, if the area shows one.
fn drag_magnifier(model: &Rc<SelectionModel>, ctx: &dyn BuildContext) -> Option<BoxedView> {
    let (side, _) = model.handles.grip.get()?;
    let look = model.handles.magnifier.borrow().clone()?;
    let line_height = model.end_line_height(side)?;
    let info = MagnifierInfo::new(side, model.text_direction(), line_height);
    Some(crate::text::magnifier(
        look.as_ref(),
        &info,
        model.handles.link(side),
        model.handles.magnifier_along.get(),
        ctx,
    ))
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
                        // update may have reported yet. The drag ends, and
                        // the toolbar shows again, though that last move's
                        // listener panics; the first failure resumes after.
                        let mut failure = FirstPanic::new();
                        if details.reason == GestureEndReason::Completed {
                            failure.contain(|| {
                                model.handle_dragged(side, details.global_position);
                            });
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
                            // The magnifier goes with the drag.
                            model.handles.refresh_magnifier(&mut failure);
                            failure.contain(|| model.handle_released());
                        }
                        failure.resume();
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

/// Lays its child out where the leader under `link` is painted: by default
/// the child's top centre at the leader's bottom-left corner, which a
/// paragraph puts at the bottom of an end's line. Nothing paints or takes
/// a touch while no leader is published, or while its end is clipped out
/// (scrolled out of a viewport).
#[derive(Debug, Clone)]
pub(crate) struct LinkFollower {
    link: LayerLink,
    /// The follower's anchor at the leader's: under the end, or the
    /// child's top-left corner at the leader's moved by an offset.
    anchors: (Alignment, Alignment),
    offset: Offset<f64>,
    child: Child,
}

impl LinkFollower {
    pub(crate) fn new(link: LayerLink, child: impl IntoView) -> Self {
        Self {
            link,
            anchors: (Alignment::BOTTOM_LEFT, Alignment::TOP_CENTER),
            offset: Offset::ZERO,
            child: Child::some(child.into_view()),
        }
    }

    /// The child's top-left corner at `offset` from the leader's top-left
    /// corner: a child placed where the leader then was moves as it moves.
    pub(crate) fn pinned(link: LayerLink, offset: Offset<f64>, child: impl IntoView) -> Self {
        Self {
            anchors: (Alignment::TOP_LEFT, Alignment::TOP_LEFT),
            offset,
            ..Self::new(link, child)
        }
    }

    /// The child's bottom centre `gap` above the leader's top-left corner
    /// and `along` past it: over the line of the end the leader marks,
    /// `along` from the end.
    pub(crate) fn above(link: LayerLink, gap: f64, along: f64, child: impl IntoView) -> Self {
        Self {
            anchors: (Alignment::TOP_LEFT, Alignment::BOTTOM_CENTER),
            offset: Offset::new(along, -gap),
            ..Self::new(link, child)
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
            .with_leader_anchor(self.anchors.0)
            .with_follower_anchor(self.anchors.1)
            .with_offset(self.offset)
            .with_leader_clip(LeaderClip::Hide)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_link(self.link)
            | render_object.set_leader_anchor(self.anchors.0)
            | render_object.set_follower_anchor(self.anchors.1)
            | render_object.set_offset(self.offset)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(LinkFollower);

/// Plays the selection click of a handle drag's step over a character, with
/// the frame that shows the end moved.
pub(crate) fn play_step(haptics: Option<&HapticsHandle>) {
    if let Some(haptics) = haptics {
        let _ = haptics.perform(
            HapticFeedback::SelectionClick,
            HapticRequest::new(HapticTiming::FramePresented),
        );
    }
}
