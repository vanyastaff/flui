//! Touch handles of an editable text: a handle under the caret after a tap
//! ([`CaretHandle`] decides whether, and for how long), and one under each
//! end of a selection a touch made. They go in the nearest
//! [`Overlay`](crate::Overlay), each pinned under its end by a layer link
//! the field's [`RenderEditable`](flui_objects::RenderEditable) publishes,
//! and look as a [`SelectionArea`](crate::SelectionArea)'s do.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, AnimationStatus, DrivenController, StatusSubscription, Vsync,
};
use flui_foundation::geometry::Offset;
use flui_foundation::notifier::ChangeNotifier;
use flui_foundation::panic::FirstPanic;
use flui_interaction::{
    GestureEndReason, GestureRecognizer, LongPressGestureRecognizer, PointerKindSet,
};
use flui_painting::typography::{TextDirection, TextPosition};
use flui_platform::TargetPlatform;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::layer::LayerLink;
use flui_rendering::pipeline::PipelineCell;
use flui_view::prelude::*;
use flui_view::{BoxedView, View, ViewExt};

use super::{CompositionCommit, source_offset_at_global, source_word_range_at_global};
use crate::interaction::GesturePolicy;
use crate::interaction::recognizer_attachment::RecognizerAttachment;
use crate::interaction::{DragSlot, GestureDetector, Listener};
use crate::overlay::{InsertPosition, OverlayEntry, OverlayHandle};
use crate::text::controller::TextEditingController;
use crate::text::selection_area::handles::{LinkFollower, Look};
use crate::text::text_store::with_editable_global;
use crate::{HandleInfo, HandleSide, Positioned, Stack};

/// Whether a tap in an editable text shows a handle under its caret, which
/// a drag moves, and how long it stays without a touch.
///
/// A tap with a touch places the caret; [`for_platform`](Self::for_platform)
/// follows each platform's own fields: Android shows the handle and hides it
/// after four seconds without a touch, Windows shows it until the next key,
/// and iOS and macOS show none (their caret moves by a long press). A handle
/// also goes on a key, a mouse press, the focus leaving, or a change of the
/// text or the selection the handle did not make.
///
/// # Examples
///
/// ```rust
/// # use std::time::Duration;
/// # use flui_widgets::CaretHandle;
/// let shown_briefly = CaretHandle::on_tap().hide_after(Duration::from_secs(2));
/// assert_eq!(shown_briefly.hides_after(), Some(Duration::from_secs(2)));
/// assert!(!CaretHandle::NEVER.shows_on_tap());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaretHandle {
    shows_on_tap: bool,
    hide_after: Option<Duration>,
}

impl CaretHandle {
    /// No caret handle: a tap places the caret and shows nothing.
    pub const NEVER: Self = Self {
        shows_on_tap: false,
        hide_after: None,
    };

    /// How long an Android field shows its caret handle without a touch.
    const ANDROID_TIMEOUT: Duration = Duration::from_secs(4);

    /// A handle shown by a tap, until a key, a press elsewhere or the
    /// focus leaving takes it.
    #[must_use]
    pub const fn on_tap() -> Self {
        Self {
            shows_on_tap: true,
            hide_after: None,
        }
    }

    /// Also hide the handle once `timeout` has passed without a touch on it.
    #[must_use]
    pub const fn hide_after(mut self, timeout: Duration) -> Self {
        self.hide_after = Some(timeout);
        self
    }

    /// The caret handle `platform`'s own text fields show.
    #[must_use]
    pub const fn for_platform(platform: TargetPlatform) -> Self {
        match platform {
            TargetPlatform::iOS | TargetPlatform::MacOS => Self::NEVER,
            TargetPlatform::Windows => Self::on_tap(),
            _ => Self::on_tap().hide_after(Self::ANDROID_TIMEOUT),
        }
    }

    /// Whether a tap shows the handle.
    #[must_use]
    pub const fn shows_on_tap(&self) -> bool {
        self.shows_on_tap
    }

    /// How long the handle stays without a touch, if it hides by itself.
    #[must_use]
    pub const fn hides_after(&self) -> Option<Duration> {
        self.hide_after
    }
}

impl Default for CaretHandle {
    /// The handle of the platform the app is built for.
    fn default() -> Self {
        Self::for_platform(TargetPlatform::current())
    }
}

/// Which handles a field shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shown {
    /// The one under the caret.
    Caret,
    /// One under each end of the selection.
    Selection,
}

/// What a field's handles reach in the field: its render object, its
/// controller and how its text is masked. Set once the field is mounted.
pub(super) struct HandleParts {
    pub(super) pipeline: Option<PipelineCell>,
    pub(super) inner_anchor: flui_objects::SubtreeAnchor,
    pub(super) controller: Rc<RefCell<TextEditingController>>,
    pub(super) obscure: Rc<Cell<bool>>,
    pub(super) obscuring_character: Rc<Cell<char>>,
    /// Rebuilds the field, which publishes the links of shown handles.
    pub(super) repaint: ChangeNotifier,
    pub(super) commit: CompositionCommit,
    pub(super) post_frame: Option<flui_scheduler::PostFrameHandle>,
}

/// A field's handles: which are shown, the links the field publishes its
/// ends under, and the overlay entry that holds them while shown.
pub(super) struct FieldHandles {
    shown: Cell<Option<Shown>>,
    links: [LayerLink; 2],
    overlay: RefCell<Option<OverlayHandle>>,
    entry: RefCell<Option<OverlayEntry>>,
    look: RefCell<Option<Look>>,
    caret_handle: Cell<CaretHandle>,
    /// Where a handle drag holds its end, relative to the contact.
    grip: Cell<Option<(HandleSide, Offset<f64>)>>,
    /// The controller's selection, caret and revision the handles were last
    /// shown or moved for: any other change, an edit of the text that keeps
    /// the offsets included, takes them away.
    expected: RefCell<Option<(Range<usize>, usize, u64)>>,
    /// Set while the handles change the controller themselves.
    moving: Cell<bool>,
    /// Set while a finger rests on a handle.
    held: Cell<bool>,
    parts: RefCell<Option<HandleParts>>,
    /// Hides the caret handle once its time without a touch is up.
    timer: RefCell<DrivenController>,
    timer_status: RefCell<Option<StatusSubscription>>,
    vsync: RefCell<Option<Vsync>>,
    /// The word a long press selected, which the same touch extends from.
    pressed_word: RefCell<Option<Range<usize>>>,
    /// The field's long-press recognizer, which [`Self::abandon`] cancels.
    long_press: RefCell<Weak<LongPressGestureRecognizer>>,
    warned: Cell<bool>,
}

impl FieldHandles {
    pub(super) fn new() -> Rc<Self> {
        Rc::new(Self {
            shown: Cell::new(None),
            links: [LayerLink::new(), LayerLink::new()],
            overlay: RefCell::new(None),
            entry: RefCell::new(None),
            look: RefCell::new(None),
            caret_handle: Cell::new(CaretHandle::default()),
            grip: Cell::new(None),
            expected: RefCell::new(None),
            moving: Cell::new(false),
            held: Cell::new(false),
            parts: RefCell::new(None),
            timer: RefCell::new(
                AnimationController::builder(CaretHandle::ANDROID_TIMEOUT)
                    .debug_label("EditableText.caret_handle")
                    .build_on(None),
            ),
            timer_status: RefCell::new(None),
            vsync: RefCell::new(None),
            pressed_word: RefCell::new(None),
            long_press: RefCell::new(Weak::new()),
            warned: Cell::new(false),
        })
    }

    /// Mounts the handles on the field: what they reach, and the timer that
    /// hides a caret handle.
    pub(super) fn mount(self: &Rc<Self>, parts: HandleParts) {
        let post_frame = parts.post_frame.clone();
        *self.parts.borrow_mut() = Some(parts);
        let handles = Rc::downgrade(self);
        let subscription =
            self.timer
                .borrow()
                .controller()
                .subscribe_status(Rc::new(move |status| {
                    if status != AnimationStatus::ForwardSettled {
                        return;
                    }
                    let handles = handles.clone();
                    let hide = move |_timing: &flui_scheduler::FrameTiming| {
                        let Some(handles) = handles.upgrade() else {
                            return;
                        };
                        // A stopped timer settles too: only one whose time
                        // ran out, with no finger on the handle, hides it.
                        let ran_out = handles.timer.borrow().controller().value() >= 1.0;
                        if handles.shown.get() == Some(Shown::Caret)
                            && handles.grip.get().is_none()
                            && !handles.held.get()
                            && ran_out
                        {
                            let mut failure = FirstPanic::new();
                            handles.hide(&mut failure);
                            failure.resume();
                        }
                    };
                    // Out of the timer's own delivery: hiding rebuilds the field.
                    if let Some(post_frame) = &post_frame {
                        let _ = post_frame.schedule(hide);
                    }
                }));
        *self.timer_status.borrow_mut() = Some(subscription);
    }

    /// The links the field publishes its ends under: the caret under the
    /// first while the caret handle shows, both while the selection's do.
    pub(super) fn published_links(&self) -> (Option<LayerLink>, Option<LayerLink>) {
        match self.shown.get() {
            Some(Shown::Caret) => (Some(self.links[0]), None),
            Some(Shown::Selection) => (Some(self.links[0]), Some(self.links[1])),
            None => (None, None),
        }
    }

    /// Follows the overlay the handles go in and the vsync their timer
    /// runs on; a moved overlay takes shown handles away.
    pub(super) fn set_scope(&self, overlay: Option<OverlayHandle>, vsync: Option<Vsync>) {
        let moved = match (self.overlay.borrow().as_ref(), overlay.as_ref()) {
            (Some(old), Some(new)) => !old.is_same(new),
            (None, None) => false,
            _ => true,
        };
        let mut failure = FirstPanic::new();
        if moved {
            self.hide(&mut failure);
            *self.overlay.borrow_mut() = overlay;
        }
        if let Err(error) = self.timer.borrow_mut().rebind(vsync.as_ref()) {
            tracing::error!(%error, "an editable text's caret handle lost its frame registry");
        }
        *self.vsync.borrow_mut() = vsync;
        failure.resume();
    }

    /// Sets what the handles look like and when the caret's shows,
    /// rebuilding shown handles when the look changed.
    pub(super) fn set_look(&self, look: Look, caret_handle: CaretHandle) {
        self.caret_handle.set(caret_handle);
        let old = self.look.borrow_mut().replace(look.clone());
        let mut failure = FirstPanic::new();
        if self.shown.get() == Some(Shown::Caret) && !caret_handle.shows_on_tap() {
            self.hide(&mut failure);
        } else if !old.is_some_and(|old| old.same(&look)) {
            self.refresh(&mut failure);
        }
        failure.resume();
    }

    /// Shows the handles a touch left: the caret's while nothing is
    /// selected, when the field shows one, else the selection's. Without an
    /// overlay there are none, which a debug build reports once.
    pub(super) fn show_for_touch(self: &Rc<Self>, failure: &mut FirstPanic) {
        let Some((selection, caret, revision)) = self.controller_state() else {
            return;
        };
        let shown = if selection.is_empty() {
            if !self.caret_handle.get().shows_on_tap() {
                self.hide(failure);
                return;
            }
            Shown::Caret
        } else {
            Shown::Selection
        };
        let overlay = self.overlay.borrow().clone();
        let Some(overlay) = overlay else {
            if cfg!(debug_assertions) && !self.warned.replace(true) {
                tracing::warn!("an EditableText shows no touch handles: no Overlay is above it");
            }
            return;
        };
        *self.expected.borrow_mut() = Some((selection, caret, revision));
        self.shown.set(Some(shown));
        self.restart_timer();
        let entry = if self.entry.borrow().is_none() {
            let handles = Rc::downgrade(self);
            let entry = OverlayEntry::new(move |_ctx| {
                HandlesView {
                    handles: handles.clone(),
                }
                .boxed()
            });
            *self.entry.borrow_mut() = Some(entry.clone());
            Some(entry)
        } else {
            None
        };
        // The handles are recorded before the overlay and the field hear of
        // them, so a wake that fails there leaves state `hide` clears.
        match entry {
            Some(entry) => {
                failure.contain(|| overlay.insert(&entry, &InsertPosition::Top));
            }
            None => self.refresh(failure),
        }
        self.repaint_field(failure);
    }

    /// The field stopped taking touches here (it was disabled, given another
    /// controller, or lost the focus): the handles go, and a long press
    /// waiting for its deadline or under way is cancelled, so it selects
    /// nothing later. The handles go first, so the cancelled press finds no
    /// word to show handles for.
    pub(super) fn abandon(&self, failure: &mut FirstPanic) {
        self.hide(failure);
        let recognizer = self.long_press.borrow().upgrade();
        if let Some(recognizer) = recognizer {
            failure.contain(|| {
                let _ = recognizer.cancel();
            });
        }
    }

    /// Takes the handles away. Returns whether any were shown.
    pub(super) fn hide(&self, failure: &mut FirstPanic) -> bool {
        let was_shown = self.take_down(failure);
        if was_shown {
            self.repaint_field(failure);
        }
        was_shown
    }

    /// The field is going away: its handles leave the overlay, its timer
    /// stops for good, and the field, being unmounted, is not rebuilt.
    pub(super) fn retire(&self, failure: &mut FirstPanic) {
        self.take_down(failure);
        self.timer_status.borrow_mut().take();
        if let Err(error) = self.timer.borrow_mut().rebind(None) {
            tracing::error!(%error, "an editable text's caret handle lost its frame registry");
        }
        self.parts.borrow_mut().take();
    }

    fn take_down(&self, failure: &mut FirstPanic) -> bool {
        self.grip.set(None);
        self.held.set(false);
        self.pressed_word.borrow_mut().take();
        *self.expected.borrow_mut() = None;
        let _ = self.timer.borrow().controller().stop();
        let was_shown = self.shown.replace(None).is_some();
        let entry = self.entry.borrow_mut().take();
        if let Some(entry) = entry {
            failure.contain(|| entry.remove());
        }
        was_shown
    }

    /// The controller changed: shown handles stay only for a change they
    /// made themselves.
    pub(super) fn controller_changed(&self, failure: &mut FirstPanic) {
        if self.moving.get() || self.shown.get().is_none() {
            return;
        }
        let current = self.controller_state();
        if current != *self.expected.borrow() {
            self.hide(failure);
        }
    }

    /// A touch long press at `global` selects the word under it (the whole
    /// text of an obscured field). Returns whether it selected.
    pub(super) fn long_pressed(&self, global: Offset<f64>) -> bool {
        let Some(word) = self.word_at(global) else {
            return false;
        };
        *self.pressed_word.borrow_mut() = Some(word.clone());
        self.move_controller(|controller| controller.set_selection(word.start, word.end));
        true
    }

    /// The long-pressing touch moved to `global`: the selection runs from
    /// the pressed word to the word there, keeping the pressed word.
    pub(super) fn long_press_moved(&self, global: Offset<f64>) {
        let Some(pressed) = self.pressed_word.borrow().clone() else {
            return;
        };
        let Some(word) = self.word_at(global) else {
            return;
        };
        let (anchor, extent) = if word.start < pressed.start {
            (pressed.end, word.start)
        } else {
            (pressed.start, word.end.max(pressed.end))
        };
        self.move_controller(|controller| controller.set_selection(anchor, extent));
    }

    /// The long-pressing touch lifted or was cancelled.
    pub(super) fn long_press_ended(self: &Rc<Self>, failure: &mut FirstPanic) {
        if self.pressed_word.borrow_mut().take().is_some() {
            self.show_for_touch(failure);
        }
    }

    /// The word under `global`, or the whole text of an obscured field.
    fn word_at(&self, global: Offset<f64>) -> Option<Range<usize>> {
        let parts = self.parts.borrow();
        let parts = parts.as_ref()?;
        let source = parts.controller.borrow().text();
        let obscuring = parts.obscure.get().then(|| parts.obscuring_character.get());
        let word = source_word_range_at_global(
            parts.pipeline.as_ref()?,
            &parts.inner_anchor,
            global,
            obscuring,
            &source,
        )?;
        Some(if obscuring.is_some() {
            0..source.len()
        } else {
            word
        })
    }

    /// Changes the controller as the handles' own move, then follows what
    /// it holds. Owner code the controller runs sees no borrow held.
    fn move_controller(&self, change: impl FnOnce(&TextEditingController)) {
        let controller = self
            .parts
            .borrow()
            .as_ref()
            .map(|parts| Rc::clone(&parts.controller));
        let Some(controller) = controller else {
            return;
        };
        let was_moving = self.moving.replace(true);
        let controller = controller.borrow().clone();
        let mut failure = FirstPanic::new();
        failure.contain(|| change(&controller));
        self.moving.set(was_moving);
        if self.shown.get().is_some() {
            *self.expected.borrow_mut() = self.controller_state();
            let shown = match self.expected.borrow().as_ref() {
                Some((selection, _, _)) if !selection.is_empty() => Shown::Selection,
                _ => Shown::Caret,
            };
            if self.shown.replace(Some(shown)) != Some(shown) {
                self.repaint_field(&mut failure);
            }
            self.refresh(&mut failure);
        }
        failure.resume();
    }

    /// A drag of `side`'s handle began with the contact at `global`: the
    /// end keeps the distance it had from the contact, and a composition is
    /// committed first.
    fn drag_started(&self, side: HandleSide, global: Offset<f64>) {
        let commit = self
            .parts
            .borrow()
            .as_ref()
            .map(|parts| parts.commit.clone());
        if let Some(commit) = commit {
            let mut calls = flui_platform::text_store::OwnerCalls::new();
            commit.run(&mut calls);
            calls.resume();
        }
        let _ = self.timer.borrow().controller().stop();
        let grip = self.end_line(side).map(|(middle, _)| middle - global);
        self.grip.set(grip.map(|grip| (side, grip)));
    }

    /// `side`'s handle was dragged to `global`: its end moves there, and a
    /// selection's end never passes the other.
    fn dragged(&self, side: HandleSide, global: Offset<f64>) {
        let Some((gripped, grip)) = self.grip.get() else {
            return;
        };
        if gripped != side {
            return;
        }
        let Some(at) = self.source_offset_at(global + grip) else {
            return;
        };
        let Some((selection, _, _)) = self.controller_state() else {
            return;
        };
        match side {
            HandleSide::Start if at < selection.end => {
                self.move_controller(|controller| controller.set_selection(selection.end, at));
            }
            HandleSide::End if at > selection.start => {
                self.move_controller(|controller| controller.set_selection(selection.start, at));
            }
            HandleSide::Collapsed => {
                self.move_controller(|controller| controller.set_caret_byte_offset(at));
            }
            _ => {}
        }
    }

    /// `side`'s handle was let go.
    fn drag_ended(&self, side: HandleSide) {
        if self.grip.get().is_some_and(|(gripped, _)| gripped == side) {
            self.grip.set(None);
            self.restart_timer();
        }
    }

    fn source_offset_at(&self, global: Offset<f64>) -> Option<usize> {
        let parts = self.parts.borrow();
        let parts = parts.as_ref()?;
        let source = parts.controller.borrow().text();
        let obscuring = parts.obscure.get().then(|| parts.obscuring_character.get());
        source_offset_at_global(
            parts.pipeline.as_ref()?,
            &parts.inner_anchor,
            global,
            obscuring,
            &source,
        )
    }

    /// Where `side`'s end sits: the middle of the caret there, in
    /// render-root coordinates, and the caret's height.
    fn end_line(&self, side: HandleSide) -> Option<(Offset<f64>, f64)> {
        let parts = self.parts.borrow();
        let parts = parts.as_ref()?;
        with_editable_global(
            parts.pipeline.as_ref()?,
            &parts.inner_anchor,
            |editable, to_root| {
                let selection = editable.selection().filter(|range| !range.is_empty());
                let position = match (side, selection) {
                    (HandleSide::Start, Some(range)) => TextPosition::downstream(range.start),
                    (HandleSide::End, Some(range)) => TextPosition::upstream(range.end),
                    _ => TextPosition::downstream(editable.caret_byte_offset()),
                };
                let caret = editable.caret_rect_at(position)?;
                let (x, y) =
                    to_root.transform_point(caret.left(), caret.top() + caret.height() / 2.0);
                Some((Offset::new(x, y), caret.height()))
            },
        )
    }

    fn controller_state(&self) -> Option<(Range<usize>, usize, u64)> {
        let parts = self.parts.borrow();
        let controller = parts.as_ref()?.controller.borrow();
        Some((
            controller.selection(),
            controller.caret_byte_offset(),
            controller.revision(),
        ))
    }

    /// A finger touched (`true`) or left (`false`) a handle: while one rests
    /// on it the caret handle's time stands still, and it runs again from
    /// the start when the finger lifts without dragging.
    fn held(&self, touching: bool) {
        self.held.set(touching);
        if touching {
            let _ = self.timer.borrow().controller().stop();
        } else if self.grip.get().is_none() {
            self.restart_timer();
        }
    }

    /// Runs the caret handle's time without a touch from the start, or
    /// stops it when nothing hides by itself or a finger rests on it.
    fn restart_timer(&self) {
        let timer = self.timer.borrow();
        let controller = timer.controller();
        match self.caret_handle.get().hides_after() {
            Some(timeout)
                if self.shown.get() == Some(Shown::Caret)
                    && !self.held.get()
                    && self.vsync.borrow().is_some() =>
            {
                controller.set_duration(timeout);
                let _ = controller.forward_from(Some(0.0));
            }
            _ => {
                let _ = controller.stop();
            }
        }
    }

    fn repaint_field(&self, failure: &mut FirstPanic) {
        let repaint = self
            .parts
            .borrow()
            .as_ref()
            .map(|parts| parts.repaint.clone());
        if let Some(repaint) = repaint {
            failure.contain(|| repaint.notify_listeners());
        }
    }

    fn refresh(&self, failure: &mut FirstPanic) {
        let entry = self.entry.borrow().clone();
        if let Some(entry) = entry {
            failure.contain(|| entry.mark_needs_build());
        }
    }
}

/// The touch long press of a field, built for the arena it is in.
pub(super) struct FieldLongPress {
    built_for: Option<GesturePolicy>,
    recognizer: Option<Rc<LongPressGestureRecognizer>>,
    pub(super) attachment: Rc<RecognizerAttachment<LongPressGestureRecognizer>>,
}

impl FieldLongPress {
    pub(super) fn new() -> Self {
        Self {
            built_for: None,
            recognizer: None,
            attachment: Rc::new(RecognizerAttachment::default()),
        }
    }

    /// Builds the recognizer for `policy`, once per policy. A long press
    /// selects through `handles`; `silence` stops the field's own drag for
    /// that contact.
    pub(super) fn build(
        &mut self,
        policy: GesturePolicy,
        handles: &Rc<FieldHandles>,
        silence: Rc<dyn Fn()>,
    ) {
        if self.built_for.as_ref() == Some(&policy) {
            return;
        }
        let GesturePolicy { arena, settings } = policy.clone();
        let weak = Rc::downgrade(handles);
        let (pressed, moved, released, cancelled) =
            (weak.clone(), weak.clone(), weak.clone(), weak);
        let recognizer = LongPressGestureRecognizer::builder(arena)
            .settings(settings)
            .allowed_devices(PointerKindSet::TOUCH)
            .on_long_press_start(move |details| {
                if let Some(handles) = pressed.upgrade() {
                    let mut failure = FirstPanic::new();
                    handles.hide(&mut failure);
                    if handles.long_pressed(details.global_position) {
                        silence();
                    }
                    failure.resume();
                }
            })
            .on_long_press_move_update(move |details| {
                if let Some(handles) = moved.upgrade() {
                    handles.long_press_moved(details.global_position);
                }
            })
            .on_long_press_end(move |details| {
                if let Some(handles) = released.upgrade() {
                    handles.long_press_moved(details.global_position);
                    let mut failure = FirstPanic::new();
                    handles.long_press_ended(&mut failure);
                    failure.resume();
                }
            })
            .on_long_press_cancel(move |_details| {
                if let Some(handles) = cancelled.upgrade() {
                    let mut failure = FirstPanic::new();
                    handles.long_press_ended(&mut failure);
                    failure.resume();
                }
            })
            .build();
        self.attachment.attach(&recognizer);
        *handles.long_press.borrow_mut() = Rc::downgrade(&recognizer);
        let outgoing = self.recognizer.replace(recognizer);
        self.built_for = Some(policy);
        if let Some(outgoing) = outgoing {
            let _ = outgoing.cancel();
        }
    }

    pub(super) fn dispose(&mut self) {
        self.attachment.clear();
        if let Some(recognizer) = self.recognizer.take() {
            let _ = recognizer.cancel();
        }
    }
}

/// The shown handles, each following its end.
#[derive(Clone)]
struct HandlesView {
    handles: Weak<FieldHandles>,
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
        let shown: Vec<BoxedView> = self
            .handles
            .upgrade()
            .map(|handles| {
                let sides: &[(HandleSide, usize)] = match handles.shown.get() {
                    Some(Shown::Caret) => &[(HandleSide::Collapsed, 0)],
                    Some(Shown::Selection) => &[(HandleSide::Start, 0), (HandleSide::End, 1)],
                    None => &[],
                };
                sides
                    .iter()
                    .filter_map(|&(side, link)| handle(&handles, side, link, ctx))
                    .collect()
            })
            .unwrap_or_default();
        Stack::new(shown)
    }
}

/// `side`'s handle under the end published under link `link`, which a drag
/// moves.
fn handle(
    handles: &Rc<FieldHandles>,
    side: HandleSide,
    link: usize,
    ctx: &dyn BuildContext,
) -> Option<BoxedView> {
    let look = handles.look.borrow().clone()?;
    let (_, line_height) = handles.end_line(side)?;
    let info = HandleInfo {
        side,
        text_direction: TextDirection::Ltr,
        line_height,
    };
    let (start, update, end) = (
        Rc::downgrade(handles),
        Rc::downgrade(handles),
        Rc::downgrade(handles),
    );
    let dragged = GestureDetector::new()
        .behavior(HitTestBehavior::Opaque)
        .drag(
            DragSlot::pan()
                .on_start(move |_cx, details| {
                    if let Some(handles) = start.upgrade() {
                        handles.drag_started(side, details.global_position);
                    }
                })
                .on_update(move |_cx, details| {
                    if let Some(handles) = update.upgrade() {
                        handles.dragged(side, details.global_position);
                    }
                })
                .on_end(move |_cx, details| {
                    if let Some(handles) = end.upgrade() {
                        // A release ends where the finger lifted, which no
                        // update may have reported yet.
                        if details.reason == GestureEndReason::Completed {
                            handles.dragged(side, details.global_position);
                        }
                        handles.drag_ended(side);
                    }
                }),
        )
        .child(look.handles().build(&info, ctx));
    // A finger resting on the handle keeps it, though it never drags: its
    // time runs again from the start once the finger lifts.
    let (down, up, cancel) = (
        Rc::downgrade(handles),
        Rc::downgrade(handles),
        Rc::downgrade(handles),
    );
    let held = Listener::new()
        .on_pointer_down(move |_cx, _dispatch| {
            if let Some(handles) = down.upgrade() {
                handles.held(true);
            }
        })
        .on_pointer_up(move |_cx, _dispatch| {
            if let Some(handles) = up.upgrade() {
                handles.held(false);
            }
        })
        .on_pointer_cancel(move |_cx, _dispatch| {
            if let Some(handles) = cancel.upgrade() {
                handles.held(false);
            }
        })
        .child(dragged);
    Some(
        Positioned::new(LinkFollower::new(handles.links[link], held))
            .left(0.0)
            .top(0.0)
            .boxed(),
    )
}
