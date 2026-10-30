//! [`SelectionArea`], the selection model it owns, and [`SelectionContainer`].

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::fmt;
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{Offset, Rect};
use flui_interaction::focus::FocusNode;
use flui_interaction::gesture::SequencedBranch;
use flui_interaction::{
    ClipboardHandle, DragAxis, DragGestureRecognizer, GestureRecognizer, KeyEventResult,
    PointerEvent, PointerKindSet, TapAndDragGestureRecognizer, TapGestureRecognizer,
};
use flui_painting::paint::Alignment;
use flui_painting::styling::Color;
use flui_painting::text_boundaries::{next_grapheme_boundary, previous_grapheme_boundary};
use flui_painting::typography::{InlineSpan, TextPosition};
use flui_platform::TargetPlatform;
use flui_platform::keyboard::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::layer::LayerLink;
use flui_rendering::pipeline::PipelineCell;
use flui_rendering::storage::RenderTree;
use flui_view::prelude::*;
use flui_view::{BoxedView, PostFrameHandle, WriterSource};

use super::handles::{DotHandles, HandleSide, SelectionHandles, SharedHandles};
use super::model::{
    AreaSelection, SelectedContent, SelectionController, SelectionError, SelectionPoint,
};
use crate::composited_transform::LinkFollower;
use crate::context_menu::{ContextMenuArea, MenuOpenHook};
use crate::interaction::MenuHoldScope;
use crate::interaction::recognizer_attachment::RecognizerAttachment;
use crate::interaction::{
    Action, ActionOutcome, Actions, CopySelectionTextIntent, DismissIntent, DragSlot, Focus,
    GestureCommand, GestureDetector, GesturePolicy, Listener, MenuCause, MenuRequest,
    SelectAllTextIntent,
};
use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, WidgetsLocalizations,
};
use crate::overlay::{InsertPosition, Overlay, OverlayEntry, OverlayHandle};
use crate::support::command_modifiers;
use crate::text::controller::{next_word_boundary, prev_word_boundary};
use crate::text::editable_text::is_word_jump_modifier;
use crate::text::{Participant, ParticipantPaint, SelectableId, SelectionRegistry, SelectionScope};

/// The fill a [`SelectionArea`] paints its selection with when given none: a neutral grey at
/// half opacity that reads over light and dark text.
const DEFAULT_SELECTION_COLOR: Color = Color::rgba(128, 128, 128, 128);

type ChangedCallback = Rc<dyn Fn(&mut EventCx<'_>, Option<&SelectedContent>)>;

/// Told whether the touch handles show.
type HandlesHook = Rc<dyn Fn(bool)>;

/// Makes the static text beneath it selectable with a mouse, touch and the keyboard, and
/// copyable.
///
/// Every [`Text`](crate::Text) and [`RichText`](crate::RichText) under the area takes part,
/// in reading order: the order of the tree, not of the screen. Fields keep their own selection,
/// and [`SelectionContainer::disabled`] keeps a subtree out.
///
/// - A mouse or pen drag selects characters; a double click and drag selects words, a triple
///   click and drag whole paragraphs. A click clears the selection; Shift+click extends it.
/// - A touch long press selects the word under it and shows two handles that drag the ends; a
///   finger that keeps moving after the long press extends the selection word by word. A tap
///   clears it.
/// - While something is selected the area holds the keyboard focus: Shift with the arrows,
///   Home and End extends the selection (by word with the platform's word modifier), Ctrl+A
///   (Cmd+A) selects everything, Ctrl+C (Cmd+C) copies, and Escape clears.
/// - A secondary click, a long press, the menu key or assistive technology opens a context
///   menu with Copy and Select all, then the commands given to
///   [`menu_command`](Self::menu_command). The menu is a popup route, so the area needs a
///   [`Navigator`](crate::Navigator) above it, as `WidgetsApp` provides.
///
/// A [`SelectionController`] reads and sets the selection without any of these.
#[derive(Clone, StatefulView)]
pub struct SelectionArea {
    child: BoxedView,
    controller: Option<SelectionController>,
    on_selection_changed: Option<ChangedCallback>,
    selection_color: Color,
    handles: Option<SharedHandles>,
    menu_commands: Vec<GestureCommand>,
}

impl SelectionArea {
    /// Make the text in `child` selectable.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
            controller: None,
            on_selection_changed: None,
            selection_color: DEFAULT_SELECTION_COLOR,
            handles: None,
            menu_commands: Vec::new(),
        }
    }

    /// Share the selection with `controller`.
    #[must_use]
    pub fn controller(mut self, controller: SelectionController) -> Self {
        self.controller = Some(controller);
        self
    }

    /// Call `callback` after each change the user makes, with what is selected (`None` once
    /// cleared). Not called for a change made through a [`SelectionController`].
    #[must_use]
    pub fn on_selection_changed<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, Option<&SelectedContent>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_selection_changed = Some(Rc::new(move |cx, content| {
            callback(cx, content).report();
        }));
        self
    }

    /// Paint the selection in `color` (default: half-transparent grey).
    #[must_use]
    pub fn selection_color(mut self, color: Color) -> Self {
        self.selection_color = color;
        self
    }

    /// Draw the touch handles with `handles` (default: [`DotHandles`] in the selection colour).
    #[must_use]
    pub fn handles(mut self, handles: impl SelectionHandles + 'static) -> Self {
        self.handles = Some(SharedHandles(Rc::new(handles)));
        self
    }

    /// Add `command` to the context menu, after Copy and Select all.
    #[must_use]
    pub fn menu_command(mut self, command: GestureCommand) -> Self {
        self.menu_commands.push(command);
        self
    }
}

impl fmt::Debug for SelectionArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectionArea")
            .field("controller", &self.controller)
            .field("selection_color", &self.selection_color)
            .field("handles", &self.handles)
            .field(
                "menu_commands",
                &self
                    .menu_commands
                    .iter()
                    .map(GestureCommand::label)
                    .collect::<Vec<_>>(),
            )
            .field("on_selection_changed", &self.on_selection_changed.is_some())
            .finish_non_exhaustive()
    }
}

/// Keeps the text beneath it out of any enclosing [`SelectionArea`].
#[derive(Clone, Debug)]
pub struct SelectionContainer {
    child: BoxedView,
}

impl SelectionContainer {
    /// `child`, with none of its text selectable.
    pub fn disabled(child: impl IntoView) -> Self {
        Self {
            child: child.into_view().boxed(),
        }
    }
}

impl View for SelectionContainer {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for SelectionContainer {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        SelectionScope::new(None, self.child.clone())
    }
}

impl StatefulView for SelectionArea {
    type State = SelectionAreaState;

    fn create_state(&self) -> Self::State {
        let core = AreaCore::new(self.selection_color);
        let own_controller = SelectionController::new();
        SelectionAreaState {
            core,
            own_controller,
            handles: HandleOverlay::default(),
        }
    }
}

/// A mounted [`SelectionArea`]: the selection and what serves it.
pub struct SelectionAreaState {
    core: Rc<AreaCore>,
    /// Serves the area while the view names no controller of its own.
    own_controller: SelectionController,
    handles: HandleOverlay,
}

impl fmt::Debug for SelectionAreaState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectionAreaState")
            .field("selection", &self.core.selection.get())
            .finish_non_exhaustive()
    }
}

impl SelectionAreaState {
    fn attach_controller(&self, view: &SelectionArea) {
        let controller = view
            .controller
            .clone()
            .unwrap_or_else(|| self.own_controller.clone());
        controller.attach(&self.core);
        let previous = self.core.controller.replace(Some(controller));
        if let Some(previous) = previous
            && !self
                .core
                .controller
                .borrow()
                .as_ref()
                .is_some_and(|current| current.same(&previous))
        {
            previous.detach(&self.core);
        }
    }
}

impl ViewState<SelectionArea> for SelectionAreaState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let mut caps = self.core.caps.borrow_mut();
        caps.owner = ctx.pipeline_owner();
        caps.clipboard = ctx.clipboard_handle();
        caps.writer = Some(ctx.writer_source());
        caps.post_frame = ctx.post_frame_handle();
        drop(caps);
        self.handles.overlay = Overlay::maybe_of(ctx);
    }

    fn did_update_view(&mut self, _old: &SelectionArea, new: &SelectionArea) {
        self.core.registry.set_color(new.selection_color);
        let _previous = self
            .core
            .on_changed
            .replace(new.on_selection_changed.clone());
        self.attach_controller(new);
    }

    fn dispose(&mut self) {
        self.handles.hide();
        self.core.registry.set_on_removed(None);
        if let Some(controller) = self.core.controller.take() {
            controller.detach(&self.core);
        }
    }

    fn build(&self, view: &SelectionArea, ctx: &dyn BuildContext) -> impl IntoView {
        // The first build after mount: the view's callback, controller and handles.
        if self.core.controller.borrow().is_none() {
            let _previous = self
                .core
                .on_changed
                .replace(view.on_selection_changed.clone());
            self.attach_controller(view);
            let weak = Rc::downgrade(&self.core);
            self.core.registry.set_on_removed(Some(Rc::new(move |id| {
                if let Some(core) = weak.upgrade() {
                    core.paragraph_left(id);
                }
            })));
        }
        let handles = view.handles.clone().unwrap_or_else(|| {
            SharedHandles(Rc::new(DotHandles::new(opaque(view.selection_color))))
        });
        self.handles.configure(&self.core, handles);

        let localizations = BoxedWidgetsLocalizations::maybe_of(ctx);
        let (copy, select_all) = localizations.as_deref().map_or_else(
            || {
                (
                    DefaultWidgetsLocalizations.copy_button_label(),
                    DefaultWidgetsLocalizations.select_all_button_label(),
                )
            },
            |localizations| {
                (
                    localizations.copy_button_label(),
                    localizations.select_all_button_label(),
                )
            },
        );
        let content = SelectionGestures {
            core: Rc::clone(&self.core),
            child: SelectionScope::new(Some(Rc::clone(&self.core.registry)), view.child.clone())
                .boxed(),
        };
        let open: MenuOpenHook = {
            let core = Rc::downgrade(&self.core);
            Rc::new(move |cx, request| {
                if let Some(core) = core.upgrade() {
                    core.menu_opening(cx, request);
                }
            })
        };
        let menu = match ContextMenuArea::new(
            GestureCommand::new(copy, CopySelectionTextIntent::Copy),
            std::iter::once(GestureCommand::new(select_all, SelectAllTextIntent))
                .chain(view.menu_commands.iter().cloned()),
        ) {
            Ok(menu) => menu.on_open(open).child(content).boxed(),
            Err(error) => {
                tracing::warn!(%error, "a SelectionArea's menu commands were refused; it has no menu");
                content.boxed()
            }
        };
        let keys = {
            let core = Rc::downgrade(&self.core);
            move |cx: &mut EventCx<'_>, event: &KeyEvent| {
                core.upgrade()
                    .map_or(KeyEventResult::Ignored, |core| core.key(cx, event))
            }
        };
        let focused = Focus::new(menu)
            .focus_node(Rc::clone(&self.core.focus))
            .skip_traversal(true)
            .on_key_event(keys);
        Actions::new(focused)
            .action::<SelectAllTextIntent>(AreaAction(Rc::downgrade(&self.core)))
            .action::<CopySelectionTextIntent>(AreaAction(Rc::downgrade(&self.core)))
            .action::<DismissIntent>(AreaAction(Rc::downgrade(&self.core)))
    }
}

/// The selection colour with full opacity, for the default handles.
fn opaque(color: Color) -> Color {
    color.with_alpha(255)
}

// ============================================================================
// The model
// ============================================================================

/// Who changed the selection: only the user's changes reach `on_selection_changed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    /// A pointer, a key, a menu command or assistive technology.
    User,
    /// A [`SelectionController`].
    App,
}

#[derive(Default)]
struct Capabilities {
    owner: Option<PipelineCell>,
    clipboard: Option<ClipboardHandle>,
    writer: Option<WriterSource>,
    post_frame: Option<PostFrameHandle>,
}

/// How far a drag selects at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Character,
    Word,
    Paragraph,
}

/// A drag in progress: the unit it selects by and the unit it started on, which stays selected.
#[derive(Debug, Clone, Copy)]
struct DragOrigin {
    unit: Unit,
    start: SelectionPoint,
    end: SelectionPoint,
}

/// One participant as the area reads it: in reading order, with its plain text.
struct Paragraph {
    participant: Rc<Participant>,
    text: String,
}

/// A selection as positions in a reading-order list: `(paragraph index, byte offset)` for its
/// start and end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: (usize, usize),
    end: (usize, usize),
}

/// The selection model of one mounted area, shared with its gestures, keys, menu, handles and
/// controller.
pub(super) struct AreaCore {
    registry: Rc<SelectionRegistry>,
    selection: Cell<Option<AreaSelection>>,
    /// Reading order and text lengths as last read, so an end on a paragraph that left can move
    /// to its neighbour.
    order: RefCell<Vec<(SelectableId, usize)>>,
    caps: RefCell<Capabilities>,
    on_changed: RefCell<Option<ChangedCallback>>,
    controller: RefCell<Option<SelectionController>>,
    focus: Rc<FocusNode>,
    drag: Cell<Option<DragOrigin>>,
    /// Whether Shift was held when the current contact went down.
    shift: Cell<bool>,
    /// Whether the touch handles are showing, and the layers they follow.
    handles_shown: Cell<bool>,
    links: [LayerLink; 2],
    /// Told when the handles show or hide.
    handles_changed: RefCell<Option<HandlesHook>>,
}

impl AreaCore {
    fn new(color: Color) -> Rc<Self> {
        Rc::new(Self {
            registry: SelectionRegistry::new(color),
            selection: Cell::new(None),
            order: RefCell::new(Vec::new()),
            caps: RefCell::new(Capabilities::default()),
            on_changed: RefCell::new(None),
            controller: RefCell::new(None),
            focus: FocusNode::new(),
            drag: Cell::new(None),
            shift: Cell::new(false),
            handles_shown: Cell::new(false),
            links: [LayerLink::new(), LayerLink::new()],
            handles_changed: RefCell::new(None),
        })
    }

    fn owner(&self) -> Option<PipelineCell> {
        self.caps.borrow().owner.clone()
    }

    pub(super) fn selection(&self) -> Option<AreaSelection> {
        self.selection.get()
    }

    /// The laid-out participants in reading order: the order of their render nodes in the tree.
    fn document(&self) -> Vec<Paragraph> {
        let Some(owner) = self.owner() else {
            return Vec::new();
        };
        let mut paragraphs: Vec<(Vec<usize>, Paragraph)> = self
            .registry
            .participants()
            .into_iter()
            .filter_map(|participant| {
                let node = participant.render_id()?;
                let text = participant
                    .with_paragraph(&owner, |paragraph, _| Some(plain_text(paragraph)))?;
                let path = owner.try_with_layout(|owner| tree_path(owner.render_tree(), node))?;
                Some((path, Paragraph { participant, text }))
            })
            .collect();
        paragraphs.sort_by(|(a, _), (b, _)| a.cmp(b));
        let document: Vec<Paragraph> = paragraphs
            .into_iter()
            .map(|(_, paragraph)| paragraph)
            .collect();
        let _previous = self.order.replace(
            document
                .iter()
                .map(|paragraph| (paragraph.participant.id(), paragraph.text.len()))
                .collect(),
        );
        document
    }

    pub(super) fn paragraphs(&self) -> Vec<SelectableId> {
        self.document()
            .iter()
            .map(|paragraph| paragraph.participant.id())
            .collect()
    }

    /// `selection` as reading-order positions, or `None` when an end is not in `document`.
    fn span(document: &[Paragraph], selection: AreaSelection) -> Option<Span> {
        let place = |point: SelectionPoint| {
            let index = document
                .iter()
                .position(|paragraph| paragraph.participant.id() == point.paragraph)?;
            Some((index, point.offset.min(document[index].text.len())))
        };
        let (anchor, focus) = (place(selection.anchor)?, place(selection.focus)?);
        Some(if anchor <= focus {
            Span {
                start: anchor,
                end: focus,
            }
        } else {
            Span {
                start: focus,
                end: anchor,
            }
        })
    }

    pub(super) fn content(&self) -> Option<SelectedContent> {
        let document = self.document();
        let span = Self::span(&document, self.selection.get()?)?;
        let mut lines = Vec::new();
        for (index, paragraph) in document
            .iter()
            .enumerate()
            .take(span.end.0 + 1)
            .skip(span.start.0)
        {
            let from = if index == span.start.0 {
                span.start.1
            } else {
                0
            };
            let to = if index == span.end.0 {
                span.end.1
            } else {
                paragraph.text.len()
            };
            lines.push(paragraph.text.get(from..to).unwrap_or_default());
        }
        Some(SelectedContent {
            plain_text: lines.join("\n"),
        })
    }

    /// Replace the selection, repaint every participant and tell whoever listens.
    pub(super) fn set(&self, selection: Option<AreaSelection>, origin: Origin) {
        let previous = self.selection.replace(selection);
        if selection.is_none() {
            self.show_handles(false);
        }
        self.repaint();
        if previous == selection {
            return;
        }
        let controller = self.controller.borrow().clone();
        if let Some(controller) = controller {
            controller.notify();
        }
        if selection.is_some() && origin == Origin::User {
            let _ = self.focus.request_focus();
        } else if selection.is_none() && self.focus.has_primary_focus() {
            self.focus.unfocus();
        }
        if origin == Origin::User {
            self.report();
        }
    }

    /// Tell `on_selection_changed` what is selected now.
    fn report(&self) {
        let Some(callback) = self.on_changed.borrow().clone() else {
            return;
        };
        let Some(writer) = self.caps.borrow().writer.clone() else {
            return;
        };
        let content = self.content();
        writer.write(|cx| callback(cx, content.as_ref()));
    }

    /// Hand each participant its part of the selection and the handles' leaders.
    fn repaint(&self) {
        let document = self.document();
        let span = self
            .selection
            .get()
            .and_then(|selection| Self::span(&document, selection));
        let owner = self.owner();
        for (index, paragraph) in document.iter().enumerate() {
            let mut paint = ParticipantPaint::default();
            if let Some(span) = span
                && (span.start.0..=span.end.0).contains(&index)
            {
                let from = if index == span.start.0 {
                    span.start.1
                } else {
                    0
                };
                let to = if index == span.end.0 {
                    span.end.1
                } else {
                    paragraph.text.len()
                };
                paint.range = Some(from..to);
                if self.handles_shown.get()
                    && let Some(owner) = &owner
                {
                    for (link, (at, offset)) in self.links.iter().zip([span.start, span.end]) {
                        if at == index
                            && let Some(caret) = caret_rect(&paragraph.participant, owner, offset)
                        {
                            paint.leaders.push((*link, caret));
                        }
                    }
                }
            }
            paragraph.participant.set_paint(paint);
        }
        // A participant not laid out yet paints nothing.
        for participant in self.registry.participants() {
            if !document
                .iter()
                .any(|paragraph| paragraph.participant.id() == participant.id())
            {
                participant.set_paint(ParticipantPaint::default());
            }
        }
    }

    pub(super) fn select_all(&self, origin: Origin) {
        let document = self.document();
        let (Some(first), Some(last)) = (document.first(), document.last()) else {
            return;
        };
        self.set(
            Some(AreaSelection::new(
                SelectionPoint::new(first.participant.id(), 0),
                SelectionPoint::new(last.participant.id(), last.text.len()),
            )),
            origin,
        );
    }

    pub(super) fn select_checked(&self, selection: AreaSelection) -> Result<(), SelectionError> {
        let document = self.document();
        if document.is_empty() {
            return Err(SelectionError::NotAttached);
        }
        for point in [selection.anchor, selection.focus] {
            let paragraph = document
                .iter()
                .find(|paragraph| paragraph.participant.id() == point.paragraph)
                .ok_or(SelectionError::UnknownParagraph(point.paragraph))?;
            if !paragraph.text.is_char_boundary(point.offset) {
                return Err(SelectionError::NotABoundary {
                    paragraph: point.paragraph,
                    offset: point.offset,
                });
            }
        }
        self.set(Some(selection), Origin::App);
        Ok(())
    }

    /// A participant unmounted: an end on it moves to the nearest paragraph still inside the
    /// selection, and with none left the selection clears. The user hears of it after the frame,
    /// since a paragraph leaves while the tree is being torn down.
    fn paragraph_left(self: &Rc<Self>, id: SelectableId) {
        let Some(selection) = self.selection.get() else {
            return;
        };
        if selection.anchor.paragraph != id && selection.focus.paragraph != id {
            self.repaint_later();
            return;
        }
        let order = self.order.borrow().clone();
        let index_of = |paragraph: SelectableId| order.iter().position(|(id, _)| *id == paragraph);
        let (Some(anchor), Some(focus)) = (
            index_of(selection.anchor.paragraph),
            index_of(selection.focus.paragraph),
        ) else {
            self.set_after_frame(None);
            return;
        };
        let forward = anchor <= focus;
        let (start, end) = if forward {
            (selection.anchor, selection.focus)
        } else {
            (selection.focus, selection.anchor)
        };
        let (from, to) = (anchor.min(focus), anchor.max(focus));
        let survivors: Vec<(SelectableId, usize)> = order[from..=to]
            .iter()
            .copied()
            .filter(|(paragraph, _)| *paragraph != id && self.registry.get(*paragraph).is_some())
            .collect();
        let (Some(first), Some(last)) = (survivors.first(), survivors.last()) else {
            self.set_after_frame(None);
            return;
        };
        let start = if start.paragraph == id {
            SelectionPoint::new(first.0, 0)
        } else {
            start
        };
        let end = if end.paragraph == id {
            SelectionPoint::new(last.0, last.1)
        } else {
            end
        };
        let moved = if forward {
            AreaSelection::new(start, end)
        } else {
            AreaSelection::new(end, start)
        };
        self.set_after_frame(Some(moved));
    }

    /// Commit `selection` now and report it once the frame that unmounted a paragraph is done.
    fn set_after_frame(self: &Rc<Self>, selection: Option<AreaSelection>) {
        let _previous = self.selection.replace(selection);
        if selection.is_none() {
            self.handles_shown.set(false);
        }
        let post_frame = self.caps.borrow().post_frame.clone();
        let Some(post_frame) = post_frame else {
            return;
        };
        let core = Rc::downgrade(self);
        let scheduled = post_frame.schedule(move |_timing| {
            if let Some(core) = core.upgrade() {
                core.after_paragraph_left();
            }
        });
        if scheduled.is_err() {
            tracing::debug!("a SelectionArea's paragraph left after its runtime closed");
        }
    }

    fn after_paragraph_left(&self) {
        if self.selection.get().is_none() && self.focus.has_primary_focus() {
            self.focus.unfocus();
        }
        self.notify_handles();
        self.repaint();
        let controller = self.controller.borrow().clone();
        if let Some(controller) = controller {
            controller.notify();
        }
        self.report();
    }

    fn repaint_later(self: &Rc<Self>) {
        let post_frame = self.caps.borrow().post_frame.clone();
        if let Some(post_frame) = post_frame {
            let core = Rc::downgrade(self);
            let _ = post_frame.schedule(move |_timing| {
                if let Some(core) = core.upgrade() {
                    core.repaint();
                }
            });
        }
    }

    // ------------------------------------------------------------------
    // Geometry
    // ------------------------------------------------------------------

    /// The position nearest `global` (root coordinates): in the paragraph under it, or else in
    /// the nearest one, nearer by line first.
    fn point_at(&self, global: Offset<f64>) -> Option<SelectionPoint> {
        let owner = self.owner()?;
        self.document()
            .iter()
            .filter_map(|paragraph| {
                paragraph
                    .participant
                    .with_paragraph(&owner, |render, to_root| {
                        let (x, y) = to_root.try_inverse()?.transform_point(global.dx, global.dy);
                        let size = render.painter().size();
                        let gap = |at: f64, extent: f64| {
                            if at < 0.0 {
                                -at
                            } else if at > extent {
                                at - extent
                            } else {
                                0.0
                            }
                        };
                        let distance = (gap(y, size.height), gap(x, size.width));
                        let position = render.painter().get_position_for_offset(Offset::new(x, y));
                        Some((
                            distance,
                            SelectionPoint::new(paragraph.participant.id(), position.offset),
                        ))
                    })
            })
            .min_by(|(a, _), (b, _)| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, point)| point)
    }

    /// The bounds of `unit` around `point`.
    fn unit_at(&self, point: SelectionPoint, unit: Unit) -> (SelectionPoint, SelectionPoint) {
        let at = |offset| SelectionPoint::new(point.paragraph, offset);
        match unit {
            Unit::Character => (point, point),
            Unit::Paragraph => {
                let len = self
                    .order
                    .borrow()
                    .iter()
                    .find(|(id, _)| *id == point.paragraph)
                    .map_or(point.offset, |(_, len)| *len);
                (at(0), at(len))
            }
            Unit::Word => {
                let word = self.owner().and_then(|owner| {
                    let participant = self.registry.get(point.paragraph)?;
                    participant.with_paragraph(&owner, |render, _| {
                        Some(
                            render
                                .painter()
                                .get_word_boundary(TextPosition::downstream(point.offset)),
                        )
                    })
                });
                word.map_or((point, point), |word| (at(word.start), at(word.end)))
            }
        }
    }

    /// Where `a` and `b` fall in reading order.
    fn compare(&self, a: SelectionPoint, b: SelectionPoint) -> Ordering {
        let order = self.order.borrow();
        let index = |point: SelectionPoint| order.iter().position(|(id, _)| *id == point.paragraph);
        (index(a), a.offset).cmp(&(index(b), b.offset))
    }

    // ------------------------------------------------------------------
    // Pointer
    // ------------------------------------------------------------------

    /// A drag by `unit` started at `global`: select that unit, and extend from it.
    fn drag_started(&self, global: Offset<f64>, unit: Unit) {
        if self.shift.get()
            && let Some(selection) = self.selection.get()
        {
            self.drag.set(Some(DragOrigin {
                unit: Unit::Character,
                start: selection.anchor,
                end: selection.anchor,
            }));
            self.drag_moved(global);
            return;
        }
        let Some(point) = self.point_at(global) else {
            return;
        };
        let (start, end) = self.unit_at(point, unit);
        self.drag.set(Some(DragOrigin { unit, start, end }));
        self.set(Some(AreaSelection::new(start, end)), Origin::User);
    }

    /// The drag moved to `global`: select from its first unit to the unit there.
    fn drag_moved(&self, global: Offset<f64>) {
        let (Some(origin), Some(point)) = (self.drag.get(), self.point_at(global)) else {
            return;
        };
        let (start, end) = self.unit_at(point, origin.unit);
        let selection = if self.compare(start, origin.start) == Ordering::Less {
            AreaSelection::new(origin.end, start)
        } else {
            AreaSelection::new(origin.start, end)
        };
        self.set(Some(selection), Origin::User);
    }

    fn drag_ended(&self) {
        self.drag.set(None);
    }

    /// A click or tap of `count` at `global`.
    fn tapped(&self, global: Offset<f64>, count: u32) {
        if self.shift.get()
            && let Some(selection) = self.selection.get()
        {
            if let Some(point) = self.point_at(global) {
                self.set(
                    Some(AreaSelection::new(selection.anchor, point)),
                    Origin::User,
                );
            }
            return;
        }
        let unit = match count {
            0 | 1 => {
                self.set(None, Origin::User);
                return;
            }
            2 => Unit::Word,
            _ => Unit::Paragraph,
        };
        if let Some(point) = self.point_at(global) {
            let (start, end) = self.unit_at(point, unit);
            self.set(Some(AreaSelection::new(start, end)), Origin::User);
        }
    }

    /// A context menu is about to open: a press outside the selection selects the word under it,
    /// and a long press shows the handles.
    fn menu_opening(&self, _cx: &mut EventCx<'_>, request: &MenuRequest) {
        if let Some(global) = request.position()
            && let Some(point) = self.point_at(global)
        {
            let inside = self.selection.get().is_some_and(|selection| {
                let (start, end) =
                    if self.compare(selection.anchor, selection.focus) == Ordering::Greater {
                        (selection.focus, selection.anchor)
                    } else {
                        (selection.anchor, selection.focus)
                    };
                self.compare(start, point) != Ordering::Greater
                    && self.compare(point, end) != Ordering::Greater
                    && start != end
            });
            if !inside {
                let (start, end) = self.unit_at(point, Unit::Word);
                self.drag.set(Some(DragOrigin {
                    unit: Unit::Word,
                    start,
                    end,
                }));
                self.set(Some(AreaSelection::new(start, end)), Origin::User);
            }
        }
        if request.cause() == MenuCause::LongPress {
            self.show_handles(true);
        }
    }

    // ------------------------------------------------------------------
    // Handles
    // ------------------------------------------------------------------

    fn show_handles(&self, shown: bool) {
        if self.handles_shown.replace(shown) != shown {
            self.repaint();
            self.notify_handles();
        }
    }

    fn notify_handles(&self) {
        let hook = self.handles_changed.borrow().clone();
        if let Some(hook) = hook {
            hook(self.handles_shown.get());
        }
    }

    /// The `side` end of the selection and the other one.
    fn ends(&self, side: HandleSide) -> Option<(SelectionPoint, SelectionPoint)> {
        let selection = self.selection.get()?;
        let forward = self.compare(selection.anchor, selection.focus) != Ordering::Greater;
        let (start, end) = if forward {
            (selection.anchor, selection.focus)
        } else {
            (selection.focus, selection.anchor)
        };
        Some(match side {
            HandleSide::Start => (start, end),
            HandleSide::End => (end, start),
        })
    }

    /// The bottom centre of `point`'s caret, in root coordinates, and its line's height.
    fn caret_bottom(&self, point: SelectionPoint) -> Option<(Offset<f64>, f64)> {
        let owner = self.owner()?;
        let participant = self.registry.get(point.paragraph)?;
        let caret = caret_rect(&participant, &owner, point.offset)?;
        participant.with_paragraph(&owner, |_, to_root| {
            let (x, y) = to_root.transform_point(caret.left(), caret.bottom());
            Some((Offset::new(x, y), caret.height()))
        })
    }

    // ------------------------------------------------------------------
    // Keys
    // ------------------------------------------------------------------

    fn key(&self, _cx: &mut EventCx<'_>, event: &KeyEvent) -> KeyEventResult {
        if event.state() != KeyState::Down {
            return KeyEventResult::Ignored;
        }
        let Some(selection) = self.selection.get() else {
            return KeyEventResult::Ignored;
        };
        let modifiers = command_modifiers(event.modifiers);
        if !modifiers.contains(Modifiers::SHIFT) {
            return KeyEventResult::Ignored;
        }
        let by_word = is_word_jump_modifier(modifiers, TargetPlatform::current());
        let document = self.document();
        let Some(index) = document
            .iter()
            .position(|paragraph| paragraph.participant.id() == selection.focus.paragraph)
        else {
            return KeyEventResult::Ignored;
        };
        let text = &document[index].text;
        let offset = selection.focus.offset.min(text.len());
        let at = |index: usize, offset: usize| {
            SelectionPoint::new(document[index].participant.id(), offset)
        };
        let focus = match &event.key {
            Key::Named(NamedKey::ArrowRight) if offset == text.len() => {
                (index + 1 < document.len()).then(|| at(index + 1, 0))
            }
            Key::Named(NamedKey::ArrowRight) if by_word => {
                Some(at(index, next_word_boundary(text, offset)))
            }
            Key::Named(NamedKey::ArrowRight) => {
                Some(at(index, next_grapheme_boundary(text, offset)))
            }
            Key::Named(NamedKey::ArrowLeft) if offset == 0 => index
                .checked_sub(1)
                .map(|previous| at(previous, document[previous].text.len())),
            Key::Named(NamedKey::ArrowLeft) if by_word => {
                Some(at(index, prev_word_boundary(text, offset)))
            }
            Key::Named(NamedKey::ArrowLeft) => {
                Some(at(index, previous_grapheme_boundary(text, offset)))
            }
            Key::Named(NamedKey::Home) => self
                .line_edge(&document[index].participant, offset, false)
                .map(|edge| at(index, edge)),
            Key::Named(NamedKey::End) => self
                .line_edge(&document[index].participant, offset, true)
                .map(|edge| at(index, edge)),
            Key::Named(NamedKey::ArrowDown) => self.line_step(&document, index, offset, true),
            Key::Named(NamedKey::ArrowUp) => self.line_step(&document, index, offset, false),
            _ => return KeyEventResult::Ignored,
        };
        if let Some(focus) = focus {
            self.set(
                Some(AreaSelection::new(selection.anchor, focus)),
                Origin::User,
            );
        }
        KeyEventResult::Handled
    }

    /// The start or end of the line `offset` is on.
    fn line_edge(&self, participant: &Participant, offset: usize, end: bool) -> Option<usize> {
        let owner = self.owner()?;
        participant.with_paragraph(&owner, |render, _| {
            let lines = render.painter().get_line_metrics();
            let line = line_of(&lines, offset)?;
            Some(if end {
                line.end_index
            } else {
                line.start_index
            })
        })
    }

    /// The position a line below (or above) `offset`, at the same horizontal place, crossing
    /// into the next (or previous) paragraph from its last (or first) line.
    fn line_step(
        &self,
        document: &[Paragraph],
        index: usize,
        offset: usize,
        down: bool,
    ) -> Option<SelectionPoint> {
        let owner = self.owner()?;
        let participant = &document[index].participant;
        // The caret's horizontal place in root coordinates, and the next line within this
        // paragraph, if any.
        let (global_x, within) = participant.with_paragraph(&owner, |render, to_root| {
            let painter = render.painter();
            let caret = painter.get_offset_for_caret(TextPosition::downstream(offset));
            let lines = painter.get_line_metrics();
            let line = line_of(&lines, offset)?;
            let next = if down {
                lines.get(line.line_number + 1)
            } else {
                line.line_number
                    .checked_sub(1)
                    .and_then(|number| lines.get(number))
            };
            let within = next.map(|next| {
                painter
                    .get_position_for_offset(Offset::new(caret.dx, next.baseline))
                    .offset
            });
            let (x, _) = to_root.transform_point(caret.dx, caret.dy);
            Some((x, within))
        })?;
        if let Some(within) = within {
            return Some(SelectionPoint::new(participant.id(), within));
        }
        let neighbour = if down {
            document.get(index + 1)?
        } else {
            document.get(index.checked_sub(1)?)?
        };
        neighbour
            .participant
            .with_paragraph(&owner, |render, to_root| {
                let painter = render.painter();
                let lines = painter.get_line_metrics();
                let line = if down { lines.first()? } else { lines.last()? };
                let (_, top) = to_root.transform_point(0.0, line.baseline);
                let (x, y) = to_root.try_inverse()?.transform_point(global_x, top);
                let position = painter.get_position_for_offset(Offset::new(x, y));
                Some(SelectionPoint::new(
                    neighbour.participant.id(),
                    position.offset,
                ))
            })
    }

    /// Write the selection to the clipboard.
    fn copy(&self) -> ActionOutcome {
        let clipboard = self.caps.borrow().clipboard.clone();
        let (Some(clipboard), Some(content)) = (clipboard, self.content()) else {
            return ActionOutcome::NotPerformed;
        };
        if content.plain_text.is_empty() {
            return ActionOutcome::NotPerformed;
        }
        clipboard.write_text(content.plain_text);
        ActionOutcome::Performed
    }
}

/// The plain text a paragraph lays out.
fn plain_text(paragraph: &flui_objects::RenderParagraph) -> String {
    paragraph
        .painter()
        .text()
        .map(InlineSpan::to_plain_text)
        .unwrap_or_default()
}

/// The line holding `offset`: the last line starting at or before it.
fn line_of(
    lines: &[flui_painting::typography::LineMetrics],
    offset: usize,
) -> Option<&flui_painting::typography::LineMetrics> {
    lines
        .iter()
        .take_while(|line| line.start_index <= offset)
        .last()
        .or_else(|| lines.first())
}

/// The caret at `offset` of `participant`'s paragraph in its local space: zero wide, as tall as
/// its line.
fn caret_rect(participant: &Participant, owner: &PipelineCell, offset: usize) -> Option<Rect> {
    participant.with_paragraph(owner, |render, _| {
        let painter = render.painter();
        let caret = painter.get_offset_for_caret(TextPosition::downstream(offset));
        let lines = painter.get_line_metrics();
        let height = line_of(&lines, offset).map_or(0.0, |line| line.ascent + line.descent);
        let rect = Rect::from_ltwh(caret.dx, caret.dy, 0.0, height);
        (caret.dx.is_finite() && caret.dy.is_finite() && height.is_finite()).then_some(rect)
    })
}

/// `node`'s place in `tree`: the index among its siblings at each depth from the root, so
/// sorting places reads the tree in order.
fn tree_path(tree: &RenderTree, node: flui_foundation::RenderId) -> Vec<usize> {
    let mut path = Vec::new();
    let mut node = node;
    while let Some(parent) = tree.parent(node) {
        let index = tree
            .children(parent)
            .iter()
            .position(|child| *child == node)
            .unwrap_or(usize::MAX);
        path.push(index);
        node = parent;
    }
    path.reverse();
    path
}

// ============================================================================
// Intents
// ============================================================================

/// Answers the area's intents: select all, copy and clear.
struct AreaAction(Weak<AreaCore>);

impl Action<SelectAllTextIntent> for AreaAction {
    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &SelectAllTextIntent) -> ActionOutcome {
        let Some(core) = self.0.upgrade() else {
            return ActionOutcome::NotPerformed;
        };
        core.select_all(Origin::User);
        ActionOutcome::Performed
    }
}

impl Action<CopySelectionTextIntent> for AreaAction {
    fn is_enabled(&self, intent: &CopySelectionTextIntent) -> bool {
        *intent == CopySelectionTextIntent::Copy
            && self
                .0
                .upgrade()
                .is_some_and(|core| core.selection.get().is_some())
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &CopySelectionTextIntent) -> ActionOutcome {
        self.0
            .upgrade()
            .map_or(ActionOutcome::NotPerformed, |core| core.copy())
    }
}

impl Action<DismissIntent> for AreaAction {
    fn is_enabled(&self, _intent: &DismissIntent) -> bool {
        self.0
            .upgrade()
            .is_some_and(|core| core.selection.get().is_some())
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &DismissIntent) -> ActionOutcome {
        let Some(core) = self.0.upgrade() else {
            return ActionOutcome::NotPerformed;
        };
        core.set(None, Origin::User);
        ActionOutcome::Performed
    }
}

// ============================================================================
// Gestures
// ============================================================================

/// The area's pointer gestures over its content: mouse and pen through a tap-and-drag, touch
/// taps, and the drag that succeeds the context menu's long press.
#[derive(Clone)]
struct SelectionGestures {
    core: Rc<AreaCore>,
    child: BoxedView,
}

impl fmt::Debug for SelectionGestures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectionGestures").finish_non_exhaustive()
    }
}

impl View for SelectionGestures {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for SelectionGestures {
    type State = SelectionGesturesState;

    fn create_state(&self) -> Self::State {
        SelectionGesturesState {
            core: Rc::clone(&self.core),
            built_for: None,
            recognizers: None,
            tap_and_drag: Rc::new(RecognizerAttachment::default()),
            touch_tap: Rc::new(RecognizerAttachment::default()),
            long_press_drag: Rc::new(RecognizerAttachment::default()),
        }
    }
}

struct Recognizers {
    tap_and_drag: Rc<TapAndDragGestureRecognizer>,
    touch_tap: Rc<TapGestureRecognizer>,
    long_press_drag: Option<Rc<DragGestureRecognizer>>,
}

impl Recognizers {
    fn cancel(&self) {
        let _ = self.tap_and_drag.cancel();
        let _ = self.touch_tap.cancel();
        if let Some(drag) = &self.long_press_drag {
            let _ = drag.cancel();
        }
    }
}

struct SelectionGesturesState {
    core: Rc<AreaCore>,
    built_for: Option<(GesturePolicy, Option<Rc<SequencedBranch>>)>,
    recognizers: Option<Recognizers>,
    tap_and_drag: Rc<RecognizerAttachment<TapAndDragGestureRecognizer>>,
    touch_tap: Rc<RecognizerAttachment<TapGestureRecognizer>>,
    long_press_drag: Rc<RecognizerAttachment<DragGestureRecognizer>>,
}

impl fmt::Debug for SelectionGesturesState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectionGesturesState")
            .finish_non_exhaustive()
    }
}

impl SelectionGesturesState {
    fn build_recognizers(
        &mut self,
        core: &Rc<AreaCore>,
        policy: GesturePolicy,
        hold: Option<Rc<SequencedBranch>>,
    ) {
        if self.built_for.as_ref().is_some_and(|(built, built_hold)| {
            *built == policy && same_branch(built_hold.as_ref(), hold.as_ref())
        }) {
            return;
        }
        let GesturePolicy { arena, settings } = policy.clone();
        let weak = Rc::downgrade(core);
        let tap_and_drag = {
            let (up, start, update, end, cancel) = (
                weak.clone(),
                weak.clone(),
                weak.clone(),
                weak.clone(),
                weak.clone(),
            );
            let pressed: Rc<Cell<Option<Offset<f64>>>> = Rc::new(Cell::new(None));
            let (pressed_down, pressed_start) = (Rc::clone(&pressed), Rc::clone(&pressed));
            TapAndDragGestureRecognizer::builder(arena.clone())
                .settings(settings.clone())
                .allowed_devices(
                    PointerKindSet::MOUSE
                        .union(PointerKindSet::PEN)
                        .union(PointerKindSet::TRACKPAD)
                        .union(PointerKindSet::VIRTUAL),
                )
                .on_tap_down(move |details| {
                    pressed_down.set(Some(details.global_position));
                })
                .on_tap_up(move |details| {
                    if let Some(core) = up.upgrade() {
                        core.tapped(details.global_position, details.consecutive_tap_count);
                    }
                })
                .on_drag_start(move |details| {
                    if let Some(core) = start.upgrade() {
                        let at = pressed_start.get().unwrap_or(details.global_position);
                        let unit = match details.consecutive_tap_count {
                            0 | 1 => Unit::Character,
                            2 => Unit::Word,
                            _ => Unit::Paragraph,
                        };
                        core.drag_started(at, unit);
                        core.drag_moved(details.global_position);
                    }
                })
                .on_drag_update(move |details| {
                    if let Some(core) = update.upgrade() {
                        core.drag_moved(details.global_position);
                    }
                })
                .on_drag_end(move |_details| {
                    if let Some(core) = end.upgrade() {
                        core.drag_ended();
                    }
                })
                .on_cancel(move || {
                    if let Some(core) = cancel.upgrade() {
                        core.drag_ended();
                    }
                })
                .build()
        };
        let touch_tap = {
            let tapped = weak.clone();
            TapGestureRecognizer::builder(arena)
                .settings(settings.clone())
                .allowed_devices(PointerKindSet::TOUCH)
                .on_tap_up(move |details| {
                    if let Some(core) = tapped.upgrade() {
                        core.tapped(details.global_position, 1);
                    }
                })
                .build()
        };
        let long_press_drag = hold.as_ref().map(|hold| {
            let (update, end, cancel) = (weak.clone(), weak.clone(), weak.clone());
            DragGestureRecognizer::builder(hold.successor(), DragAxis::Free)
                .settings(settings)
                .on_update(move |details| {
                    if let Some(core) = update.upgrade() {
                        core.drag_moved(details.global_position);
                    }
                })
                .on_end(move |_details| {
                    if let Some(core) = end.upgrade() {
                        core.drag_ended();
                    }
                })
                .on_cancel(move || {
                    if let Some(core) = cancel.upgrade() {
                        core.drag_ended();
                    }
                })
                .build()
        });
        self.tap_and_drag.attach(&tap_and_drag);
        self.touch_tap.attach(&touch_tap);
        match &long_press_drag {
            Some(drag) => self.long_press_drag.attach(drag),
            None => self.long_press_drag.clear(),
        }
        let outgoing = self.recognizers.replace(Recognizers {
            tap_and_drag,
            touch_tap,
            long_press_drag,
        });
        self.built_for = Some((policy, hold));
        if let Some(outgoing) = outgoing {
            outgoing.cancel();
        }
    }
}

fn same_branch(a: Option<&Rc<SequencedBranch>>, b: Option<&Rc<SequencedBranch>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl ViewState<SelectionGestures> for SelectionGesturesState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    /// Rebuilds the recognizers when the arena, its settings or the menu's hold changed.
    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let core = Rc::clone(&self.core);
        self.build_recognizers(&core, GesturePolicy::of(ctx), MenuHoldScope::of(ctx));
    }

    fn dispose(&mut self) {
        self.tap_and_drag.clear();
        self.touch_tap.clear();
        self.long_press_drag.clear();
        if let Some(recognizers) = self.recognizers.take() {
            recognizers.cancel();
        }
    }

    fn build(&self, view: &SelectionGestures, _ctx: &dyn BuildContext) -> impl IntoView {
        let shift = Rc::downgrade(&view.core);
        // The whole area takes a press, not only its glyphs: a drag may start in a margin.
        Listener::new()
            .behavior(HitTestBehavior::Translucent)
            .on_pointer_down(move |_cx, dispatch| {
                if let (Some(core), PointerEvent::Down(press)) = (shift.upgrade(), dispatch.local) {
                    core.shift.set(press.modifiers.contains(Modifiers::SHIFT));
                }
            })
            .recognizer(&self.tap_and_drag)
            .recognizer(&self.touch_tap)
            .recognizer(&self.long_press_drag)
            .child(view.child.clone())
    }
}

// ============================================================================
// Handles
// ============================================================================

/// The two handle entries in the nearest overlay while the handles show.
#[derive(Default)]
struct HandleOverlay {
    overlay: Option<OverlayHandle>,
    entries: Rc<RefCell<Vec<OverlayEntry>>>,
}

impl HandleOverlay {
    /// Show and hide the handles with `core`, drawing them with `handles`.
    fn configure(&self, core: &Rc<AreaCore>, handles: SharedHandles) {
        let overlay = self.overlay.clone();
        let entries = Rc::clone(&self.entries);
        let weak = Rc::downgrade(core);
        let hook: HandlesHook = Rc::new(move |shown| {
            let removed: Vec<OverlayEntry> = entries.borrow_mut().drain(..).collect();
            for entry in removed {
                entry.remove();
            }
            if !shown {
                return;
            }
            let Some(overlay) = &overlay else {
                tracing::warn!(
                    "a SelectionArea has no Overlay above it, so its handles cannot show"
                );
                return;
            };
            for side in [HandleSide::Start, HandleSide::End] {
                let entry = handle_entry(weak.clone(), side, handles.clone());
                overlay.insert(&entry, &InsertPosition::Top);
                entries.borrow_mut().push(entry);
            }
        });
        let _previous = core.handles_changed.replace(Some(Rc::clone(&hook)));
        // A rebuild with new handles redraws the ones showing.
        if core.handles_shown.get() {
            hook(true);
        }
    }

    fn hide(&self) {
        let removed: Vec<OverlayEntry> = self.entries.borrow_mut().drain(..).collect();
        for entry in removed {
            entry.remove();
        }
    }
}

/// The overlay entry of the `side` handle: its glyph, following its end's leader, dragging it.
fn handle_entry(core: Weak<AreaCore>, side: HandleSide, handles: SharedHandles) -> OverlayEntry {
    OverlayEntry::new(move |ctx| {
        let Some(area) = core.upgrade() else {
            return crate::SizedBox::shrink().boxed();
        };
        let size = handles.checked_size();
        let link = area.links[match side {
            HandleSide::Start => 0,
            HandleSide::End => 1,
        }];
        // Where the pointer took hold, relative to the end's caret bottom, and the end that stays.
        let grab: Rc<Cell<Option<Grab>>> = Rc::new(Cell::new(None));
        let (start_core, update_core, end_core) = (core.clone(), core.clone(), core.clone());
        let (start_grab, update_grab, end_grab) = (Rc::clone(&grab), Rc::clone(&grab), grab);
        let glyph = crate::SizedBox::new(size.width, size.height).child(handles.0.build(side, ctx));
        let dragged = GestureDetector::new()
            .drag(
                DragSlot::pan()
                    .on_start(move |_cx, details| {
                        let Some(core) = start_core.upgrade() else {
                            return;
                        };
                        let Some((moving, fixed)) = core.ends(side) else {
                            return;
                        };
                        if let Some((bottom, height)) = core.caret_bottom(moving) {
                            start_grab.set(Some(Grab {
                                offset: details.global_position - bottom,
                                fixed,
                                line: height,
                            }));
                        }
                    })
                    .on_update(move |_cx, details| {
                        let (
                            Some(core),
                            Some(Grab {
                                offset,
                                fixed,
                                line,
                            }),
                        ) = (update_core.upgrade(), update_grab.get())
                        else {
                            return;
                        };
                        let target =
                            details.global_position - offset - Offset::new(0.0, line / 2.0);
                        if let Some(point) = core.point_at(target) {
                            core.set(Some(AreaSelection::new(fixed, point)), Origin::User);
                        }
                    })
                    .on_end(move |_cx, _details| {
                        end_grab.set(None);
                        let _ = &end_core;
                    }),
            )
            .child(glyph);
        crate::Stack::new(vec![
            crate::Positioned::new(LinkFollower::new(
                link,
                Alignment::BOTTOM_LEFT,
                Alignment::TOP_CENTER,
                dragged,
            ))
            .left(0.0)
            .top(0.0)
            .boxed(),
        ])
        .boxed()
    })
}

/// Where a finger took hold of a handle: its offset from the end's caret bottom, the end that
/// stays, and the height of the moving end's line.
#[derive(Debug, Clone, Copy)]
struct Grab {
    offset: Offset<f64>,
    fixed: SelectionPoint,
    line: f64,
}
