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
use flui_foundation::notifier::{ChangeNotifier, Listenable};
use flui_foundation::panic::FirstPanic;
use flui_interaction::HapticsHandle;
use flui_interaction::{
    GestureEndReason, GestureRecognizer, LongPressGestureRecognizer, PointerKindSet,
};
use flui_painting::typography::{TextDirection, TextPosition};
use flui_platform::TargetPlatform;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::layer::LayerLink;
use flui_rendering::pipeline::{LeaderWatch, PipelineCell};
use flui_view::prelude::*;
use flui_view::{BoxedView, View, ViewExt};

use super::{
    CompositionCommit, EditableText, source_offset_at_global, source_word_range_at_global,
};
#[cfg(doc)]
use crate::DefaultSelectionControls;
use crate::interaction::GesturePolicy;
use crate::interaction::actions::ActionChain;
use crate::interaction::recognizer_attachment::RecognizerAttachment;
use crate::interaction::{DragSlot, GestureDetector, Listener};
use crate::overlay::{InsertPosition, OverlayEntry, OverlayHandle};
use crate::text::controller::TextEditingController;
use crate::text::selection_area::handles::{LinkFollower, Look, play_step};
use crate::text::text_store::with_editable_global;
use crate::text::{
    MagnifierChoice, MagnifierInfo, MenuHost, MenuSettings, PopupOrigin, SelectionMagnifier,
    ToolbarAnchors, editing_commands,
};
use crate::{HandleInfo, HandleSide, Positioned, SelectionHandles, Stack};

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

impl EditableText {
    /// A touch selection (a long press, which the same touch extends by
    /// words, a double tap or a drag) shows a handle under each end, and a
    /// tap one under the caret as [`Self::caret_handle`] says; dragging a
    /// handle moves its end. They show in the nearest
    /// [`Overlay`](crate::Overlay), and none without one.
    #[must_use]
    pub fn handles(mut self, handles: impl SelectionHandles) -> Self {
        self.handles = Some(Rc::new(handles));
        self
    }

    /// Show `magnifier` while a finger drags a handle, in place of the
    /// nearest [`DefaultSelectionControls`]' or the platform's
    /// ([`PlainMagnifier::for_platform`](crate::text::PlainMagnifier::for_platform)).
    #[must_use]
    pub fn magnifier(mut self, magnifier: impl SelectionMagnifier) -> Self {
        self.magnifier = Some(MagnifierChoice::Show(Rc::new(magnifier)));
        self
    }

    /// Show no magnifier while a finger drags a handle, though a
    /// [`DefaultSelectionControls`] above or the platform gives one.
    #[must_use]
    pub fn no_magnifier(mut self) -> Self {
        self.magnifier = Some(MagnifierChoice::Hide);
        self
    }

    /// Whether a tap shows a handle under the caret, in place of the
    /// nearest [`DefaultSelectionControls`]' choice or the platform's
    /// ([`CaretHandle::default`]).
    #[must_use]
    pub fn caret_handle(mut self, caret_handle: CaretHandle) -> Self {
        self.caret_handle = Some(caret_handle);
        self
    }
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
    /// While the handles change the controller themselves, the revision
    /// their own change makes. A change a listener makes in reply has a
    /// later revision and counts as an outside edit.
    own_revision: Cell<Option<u64>>,
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
    /// Rebuild shown handles when the ends they follow change after a
    /// layout, so a handle sized from its line follows a new line height.
    watches: RefCell<Vec<LeaderWatch>>,
    warned: Cell<bool>,
    /// What a handle drag shows over the dragged end, resolved at the
    /// field's last build.
    magnifier: RefCell<Option<Rc<dyn SelectionMagnifier>>>,
    /// How far along the line from the dragged end the drag's magnifier
    /// sits: where the finger holds the end, less where the end is.
    magnifier_along: Cell<f64>,
    /// What a handle drag plays a step over a character with.
    haptics: RefCell<Option<HapticsHandle>>,
    /// The field's selection menu, whose toolbar shows over shown handles.
    menu: Rc<MenuHost>,
    /// Whether the finger on a handle has dragged it since it touched.
    touch_dragged: Cell<bool>,
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
            own_revision: Cell::new(None),
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
            watches: RefCell::new(Vec::new()),
            warned: Cell::new(false),
            menu: Rc::new(MenuHost::default()),
            touch_dragged: Cell::new(false),
            magnifier: RefCell::new(None),
            magnifier_along: Cell::new(0.0),
            haptics: RefCell::new(None),
        })
    }

    /// The field's selection menu.
    pub(super) fn menu(&self) -> &Rc<MenuHost> {
        &self.menu
    }

    /// Shows the menu's toolbar over shown handles, when the field has a
    /// menu; it follows its items' enablement while it shows.
    fn show_toolbar(self: &Rc<Self>, failure: &mut FirstPanic) {
        if self.shown.get().is_none() {
            return;
        }
        let handles = Rc::downgrade(self);
        let changed: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(handles) = handles.upgrade() {
                let mut failure = FirstPanic::new();
                handles.refresh(&mut failure);
                failure.resume();
            }
        });
        if self.menu.start_toolbar(&changed) {
            self.refresh(failure);
        }
    }

    /// Resolves the field's menu in its build, its items performing
    /// through `chain`, the field's actions: a retired popup closes after
    /// the frame, and a shown toolbar shows the new items, follows their
    /// enablement, or goes with the look.
    pub(super) fn menu_configured(
        self: &Rc<Self>,
        ctx: &dyn BuildContext,
        settings: &MenuSettings,
        chain: Option<ActionChain>,
        origin: &PopupOrigin,
    ) {
        let editing = editing_commands(ctx);
        let owner = "an EditableText";
        (self.menu).configure(ctx, settings, &editing, chain, origin, owner);
        let post_frame = self
            .parts
            .borrow()
            .as_ref()
            .and_then(|parts| parts.post_frame.clone());
        if !self.menu.configured(post_frame.as_ref()) {
            return;
        }
        let mut failure = FirstPanic::new();
        // Taken away first, so a menu turned off leaves no toolbar.
        self.hide_toolbar(&mut failure);
        self.show_toolbar(&mut failure);
        failure.resume();
    }

    fn hide_toolbar(&self, failure: &mut FirstPanic) {
        if self.menu.stop_toolbar() {
            self.refresh(failure);
        }
    }

    /// The caret's handle was tapped: the toolbar shows, or goes when it
    /// showed, as the menu's policy lets a tap show it.
    fn caret_tapped(self: &Rc<Self>) {
        let mut failure = FirstPanic::new();
        if self.menu.toolbar_shown() {
            self.hide_toolbar(&mut failure);
        } else if self.menu.policy().on_caret_handle_tap() {
            self.show_toolbar(&mut failure);
        }
        failure.resume();
    }

    /// Mounts the handles on the field: what they reach, and the timer that
    /// hides a caret handle.
    pub(super) fn mount(self: &Rc<Self>, parts: HandleParts) {
        let post_frame = parts.post_frame.clone();
        let pipeline = parts.pipeline.clone();
        *self.parts.borrow_mut() = Some(parts);
        if let (Some(pipeline), Some(post_frame)) = (&pipeline, &post_frame) {
            let watches: Vec<LeaderWatch> = self
                .links
                .iter()
                .enumerate()
                .map(|(index, &link)| {
                    let watch = pipeline.watch_leader(link, post_frame.clone());
                    let handles = Rc::downgrade(self);
                    // The handles follow a move by themselves; their look
                    // depends on the leader's size, and the toolbar's
                    // placement against the overlay on where it stands too.
                    let size = Cell::new(watch.geometry().map(|geometry| geometry.size));
                    watch.add_listener(Rc::new(move || {
                        let Some(handles) = handles.upgrade() else {
                            return;
                        };
                        let geometry = handles
                            .watches
                            .borrow()
                            .get(index)
                            .and_then(LeaderWatch::geometry);
                        let resized = size.replace(geometry.map(|geometry| geometry.size))
                            != geometry.map(|geometry| geometry.size);
                        let placed = resized || handles.menu.toolbar_shown();
                        if handles.shown.get().is_some() && placed {
                            let mut failure = FirstPanic::new();
                            handles.refresh(&mut failure);
                            failure.resume();
                        }
                    }));
                    watch
                })
                .collect();
            *self.watches.borrow_mut() = watches;
        }
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
    pub(super) fn set_scope(
        &self,
        overlay: Option<OverlayHandle>,
        vsync: Option<Vsync>,
        haptics: Option<HapticsHandle>,
    ) {
        *self.haptics.borrow_mut() = haptics;
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
    pub(super) fn set_look(self: &Rc<Self>, look: Look, caret_handle: CaretHandle) {
        let retimed =
            self.caret_handle.replace(caret_handle).hides_after() != caret_handle.hides_after();
        let old = self.look.borrow_mut().replace(look.clone());
        let mut failure = FirstPanic::new();
        if self.shown.get() == Some(Shown::Caret) && !caret_handle.shows_on_tap() {
            self.hide(&mut failure);
        } else {
            // A shown caret handle runs on the new timeout from now, or
            // stops running out; an unchanged policy keeps its time. This
            // runs in the field's build, where the timer can't change, so
            // after the frame.
            let post_frame = self
                .parts
                .borrow()
                .as_ref()
                .and_then(|parts| parts.post_frame.clone());
            if retimed && let Some(post_frame) = post_frame {
                let handles = Rc::downgrade(self);
                let _ = post_frame.schedule(move |_timing: &flui_scheduler::FrameTiming| {
                    if let Some(handles) = handles.upgrade() {
                        handles.restart_timer();
                    }
                });
            }
            if !old.is_some_and(|old| old.same(&look)) {
                self.refresh(&mut failure);
            }
        }
        failure.resume();
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
        // A new touch takes the toolbar the last one showed.
        self.menu.stop_toolbar();
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
        self.menu.stop_toolbar();
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
        let own = self.own_revision.get();
        if own.is_some() && own == self.controller_state().map(|(.., revision)| revision) {
            return;
        }
        // An edit during a long press, whose handles show only once it
        // lifts, ends the press: the word it holds is the old text's. The
        // word goes first, so the cancel finds nothing to show handles for.
        if self.shown.get().is_none() {
            if self.pressed_word.borrow_mut().take().is_some() {
                let recognizer = self.long_press.borrow().upgrade();
                if let Some(recognizer) = recognizer {
                    failure.contain(|| {
                        let _ = recognizer.cancel();
                    });
                }
            }
            return;
        }
        let current = self.controller_state();
        if current != *self.expected.borrow() {
            self.hide(failure);
        }
    }

    /// A touch long press at `global` selects the word under it (the whole
    /// text of an obscured field). Returns whether it selected.
    /// A panic from the controller's listeners is held in `failure`: the
    /// press still selected, so the caller still claims the contact.
    pub(super) fn long_pressed(&self, global: Offset<f64>, failure: &mut FirstPanic) -> bool {
        let Some(word) = self.word_at(global) else {
            return false;
        };
        *self.pressed_word.borrow_mut() = Some(word.clone());
        failure.contain(|| {
            self.move_controller(|controller| controller.set_selection(word.start, word.end));
        });
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
            if self.shown.get() == Some(Shown::Selection) && self.menu.policy().after_long_press() {
                self.show_toolbar(failure);
            }
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
        let controller = controller.borrow().clone();
        let own = controller.revision().wrapping_add(1);
        let outer = self.own_revision.replace(Some(own));
        let mut failure = FirstPanic::new();
        failure.contain(|| change(&controller));
        self.own_revision.set(outer);
        // A listener that edited in reply already took the handles down;
        // they only follow a controller left as their own change made it.
        if self.shown.get().is_some() && controller.revision() == own {
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
        self.touch_dragged.set(true);
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
        let mut failure = FirstPanic::new();
        self.hide_toolbar(&mut failure);
        failure.resume();
        let grip = self.end_line(side).map(|(middle, _)| middle - global);
        self.grip.set(grip.map(|grip| (side, grip)));
        self.magnifier_along.set(0.0);
        // The magnifier shows over the end the drag holds.
        let mut failure = FirstPanic::new();
        self.refresh_magnifier(&mut failure);
        failure.resume();
    }

    /// `side`'s handle was dragged to `global`: its end moves there, and a
    /// selection's end never passes the other. Each step of the end plays
    /// a selection click, and the magnifier stays under the finger; both
    /// happen though a listener of the controller fails, whose first
    /// failure is resumed after.
    fn dragged(&self, side: HandleSide, global: Offset<f64>) {
        let Some((gripped, grip)) = self.grip.get() else {
            return;
        };
        if gripped != side {
            return;
        }
        let held = global + grip;
        let before = self.controller_state();
        let mut failure = FirstPanic::new();
        if let (Some(at), Some((selection, _, _))) = (self.source_offset_at(held), &before) {
            failure.contain(|| match side {
                HandleSide::Start if at < selection.end => {
                    self.move_controller(|controller| controller.set_selection(selection.end, at));
                }
                HandleSide::End if at > selection.start => {
                    self.move_controller(|controller| {
                        controller.set_selection(selection.start, at);
                    });
                }
                HandleSide::Collapsed => {
                    self.move_controller(|controller| controller.set_caret_byte_offset(at));
                }
                _ => {}
            });
        }
        let moved =
            |state: Option<(Range<usize>, usize, u64)>| state.map(|(s, caret, _)| (s, caret));
        if moved(self.controller_state()) != moved(before) {
            let haptics = self.haptics.borrow().clone();
            failure.contain(|| play_step(haptics.as_ref()));
        }
        failure.contain(|| self.follow_finger(side, held));
        failure.resume();
    }

    /// `side`'s handle was let go.
    fn drag_ended(self: &Rc<Self>, side: HandleSide) {
        if self.grip.get().is_some_and(|(gripped, _)| gripped == side) {
            self.grip.set(None);
            self.magnifier_along.set(0.0);
            let mut failure = FirstPanic::new();
            // The magnifier goes with the drag.
            self.refresh_magnifier(&mut failure);
            self.restart_timer();
            if self.menu.policy().after_handle_drag() {
                self.show_toolbar(&mut failure);
            }
            failure.resume();
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
        self.caret_line(|editable| {
            let selection = editable.selection().filter(|range| !range.is_empty());
            end_position(side, selection.cloned(), editable.caret_byte_offset())
        })
    }

    /// Where the controller now puts `side`'s end, laid out as the field
    /// last was: what the next layout moves the end's leader to while the
    /// text stays as it is, as it does through a handle drag.
    fn controller_end_line(&self, side: HandleSide) -> Option<(Offset<f64>, f64)> {
        let (selection, caret, _) = self.controller_state()?;
        let selection = Some(selection).filter(|range| !range.is_empty());
        self.caret_line(|_| end_position(side, selection, caret))
    }

    /// The middle of the caret at the position `pick` chooses, in
    /// render-root coordinates, and the caret's height.
    fn caret_line(
        &self,
        pick: impl FnOnce(&flui_objects::RenderEditable) -> TextPosition,
    ) -> Option<(Offset<f64>, f64)> {
        let parts = self.parts.borrow();
        let parts = parts.as_ref()?;
        with_editable_global(
            parts.pipeline.as_ref()?,
            &parts.inner_anchor,
            |editable, to_root| {
                let caret = editable.caret_rect_at(pick(editable))?;
                let (x, y) =
                    to_root.transform_point(caret.left(), caret.top() + caret.height() / 2.0);
                Some((Offset::new(x, y), caret.height()))
            },
        )
    }

    /// Keeps a drag's magnifier under the finger, which holds `side`'s end
    /// at `held`: it sits that far along the line from where the end now
    /// goes, so it moves between characters while the end steps over them.
    fn follow_finger(&self, side: HandleSide, held: Offset<f64>) {
        let Some((end, _)) = self.controller_end_line(side) else {
            return;
        };
        let along = held.dx - end.dx;
        if self.magnifier_along.replace(along) != along {
            let mut failure = FirstPanic::new();
            self.refresh_magnifier(&mut failure);
            failure.resume();
        }
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
            self.touch_dragged.set(false);
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
                    if handles.long_pressed(details.global_position, &mut failure) {
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
                    let mut failure = FirstPanic::new();
                    failure.contain(|| handles.long_press_moved(details.global_position));
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
                let mut shown: Vec<BoxedView> = sides
                    .iter()
                    .filter_map(|&(side, link)| handle(&handles, side, link, ctx))
                    .collect();
                shown.extend(drag_magnifier(&handles, ctx));
                if handles.menu.toolbar_shown() {
                    shown.extend(toolbar(&handles));
                }
                shown
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
                        // The drag ends though that last move's listeners
                        // panic; the first failure resumes after.
                        let mut failure = FirstPanic::new();
                        if details.reason == GestureEndReason::Completed {
                            failure.contain(|| handles.dragged(side, details.global_position));
                        }
                        failure.contain(|| handles.drag_ended(side));
                        failure.resume();
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
                // A finger that lifts from the caret's handle without
                // dragging it taps it.
                if side == HandleSide::Collapsed && !handles.touch_dragged.get() {
                    handles.caret_tapped();
                }
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

/// The magnifier over the end a handle drag holds, if the field shows one.
fn drag_magnifier(handles: &Rc<FieldHandles>, ctx: &dyn BuildContext) -> Option<BoxedView> {
    let (side, _) = handles.grip.get()?;
    let look = handles.magnifier.borrow().clone()?;
    let (_, line_height) = handles.end_line(side)?;
    let link = match side {
        HandleSide::End => handles.links[1],
        HandleSide::Start | HandleSide::Collapsed => handles.links[0],
    };
    let info = MagnifierInfo::new(side, TextDirection::Ltr, line_height);
    let along = handles.magnifier_along.get();
    Some(crate::text::magnifier(
        look.as_ref(),
        &info,
        link,
        along,
        ctx,
    ))
}

/// The menu's toolbar over the selection's start (the caret while the
/// caret's handle shows), or under its end where there is no room above:
/// see [`crate::text::toolbar`]. Choosing an item takes it away.
fn toolbar(handles: &Rc<FieldHandles>) -> Option<BoxedView> {
    let (first, last) = match handles.shown.get()? {
        Shown::Caret => (HandleSide::Collapsed, HandleSide::Collapsed),
        Shown::Selection => (HandleSide::Start, HandleSide::End),
    };
    let start = handles.end_line(first)?;
    let end = handles.end_line(last)?;
    let overlay = handles.overlay.borrow().clone()?;
    let (pipeline, post_frame) = {
        let parts = handles.parts.borrow();
        let parts = parts.as_ref()?;
        (parts.pipeline.clone()?, parts.post_frame.clone())
    };
    let anchors = ToolbarAnchors::in_overlay(&overlay, &pipeline, start, end)?;
    let weak = Rc::downgrade(handles);
    let hide: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(handles) = weak.upgrade() {
            let mut failure = FirstPanic::new();
            handles.hide_toolbar(&mut failure);
            failure.resume();
        }
    });
    crate::text::toolbar(&handles.menu, anchors, handles.links[0], post_frame, hide)
}

/// Where `side`'s end of `selection` (non-empty) sits, or the caret at
/// `caret` without one: the start before its first byte, the end after its
/// last.
fn end_position(side: HandleSide, selection: Option<Range<usize>>, caret: usize) -> TextPosition {
    match (side, selection) {
        (HandleSide::Start, Some(range)) => TextPosition::downstream(range.start),
        (HandleSide::End, Some(range)) => TextPosition::upstream(range.end),
        _ => TextPosition::downstream(caret),
    }
}
