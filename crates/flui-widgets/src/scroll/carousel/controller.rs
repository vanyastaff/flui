//! [`CarouselController`]: the active item of one mounted carousel, and moves to another.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{SharedCurve, curves};
use flui_foundation::MutatedDuringBuild;
use flui_objects::{CarouselGeometry, CarouselItemFrames, CarouselLayout};
use flui_view::{EventCx, EventOutcome};

use crate::scroll::ScrollController;
use crate::support::{ValueCallback, value_callback};

/// How long [`CarouselController::animate_to_item`] takes.
const MOVE_DURATION: Duration = Duration::from_millis(400);

/// The active item of one mounted [`Carousel`](super::Carousel), moves to another item, and
/// pausing its autoplay.
///
/// The active item is the one resting nearest the scroll offset: the item on the first large
/// keyline, or on the centre one of a centred hero layout; on a tie the lower index. It follows
/// the scroll while it happens, and [`on_index_changed`](Self::on_index_changed) hears each
/// change once, after the frame that committed it, with no borrow of the controller or the
/// carousel held, so the callback may move the carousel again, replace itself or unmount the
/// carousel.
///
/// A controller belongs to at most one mounted carousel, as a scroll controller belongs to
/// one position: a second carousel given it while the first is mounted logs a warning and runs
/// on a controller of its own, and the first keeps it. Clones share the same controller.
#[derive(Clone, Default)]
pub struct CarouselController {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    attachment: RefCell<Option<Attachment>>,
    next_token: Cell<u64>,
    index: Cell<Option<usize>>,
    /// An item asked for before a carousel took the controller.
    pending: Cell<Option<usize>>,
    callback: RefCell<Option<ValueCallback<usize>>>,
    paused: Cell<bool>,
    /// Told when [`CarouselController::pause`] or [`resume`](CarouselController::resume) changes
    /// the pause.
    pause_listeners: RefCell<Vec<PauseListener>>,
}

/// A listener told of pause changes, by the id that removes it.
type PauseListener = (u64, Rc<dyn Fn()>);

/// The carousel a controller drives.
#[derive(Clone)]
pub(super) struct Attachment {
    pub(super) token: u64,
    pub(super) scroll: ScrollController,
    pub(super) frames: CarouselItemFrames,
    pub(super) layout: CarouselLayout,
    pub(super) spacing: f64,
    pub(super) count: usize,
    pub(super) reduced_motion: bool,
}

impl Attachment {
    fn geometry(&self) -> Option<CarouselGeometry> {
        let position = self.scroll.position();
        if !position.has_applied_viewport_dimension() {
            return None;
        }
        CarouselGeometry::try_new(
            &self.layout,
            position.viewport_dimension(),
            self.spacing,
            self.count,
        )
    }
}

impl fmt::Debug for CarouselController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CarouselController")
            .field("attached", &self.inner.attachment.borrow().is_some())
            .field("index", &self.inner.index.get())
            .field("paused", &self.inner.paused.get())
            .finish_non_exhaustive()
    }
}

impl CarouselController {
    /// A controller no carousel has taken yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The active item; `None` before a carousel takes the controller, after it lets go, and
    /// while the carousel has no items.
    #[must_use]
    pub fn index(&self) -> Option<usize> {
        let attached = self
            .inner
            .attachment
            .borrow()
            .as_ref()
            .is_some_and(|attachment| attachment.count > 0);
        attached.then(|| self.inner.index.get()).flatten()
    }

    /// Scrolls to item `index` (the last item past the end) over a short animation, through
    /// the carousel's scroll position as a programmatic scroll: any user input takes over.
    /// Under reduced motion the move is immediate. Before the carousel is laid out it opens
    /// on that item instead, the last such call winning; with no items it does nothing.
    ///
    /// # Errors
    ///
    /// [`MutatedDuringBuild`] when called from a `build` while the carousel is laid out: the
    /// position does not move (ADR-0086 §7).
    pub fn animate_to_item(&self, index: usize) -> Result<(), MutatedDuringBuild> {
        self.move_to(index, true)
    }

    /// [`animate_to_item`](Self::animate_to_item) without the animation.
    ///
    /// # Errors
    ///
    /// As [`animate_to_item`](Self::animate_to_item).
    pub fn jump_to_item(&self, index: usize) -> Result<(), MutatedDuringBuild> {
        self.move_to(index, false)
    }

    fn move_to(&self, index: usize, animate: bool) -> Result<(), MutatedDuringBuild> {
        let attachment = self.inner.attachment.borrow().clone();
        let Some(attachment) = attachment else {
            self.inner.pending.set(Some(index));
            return Ok(());
        };
        if attachment.count == 0 {
            return Ok(());
        }
        let index = index.min(attachment.count - 1);
        let Some(geometry) = attachment.geometry() else {
            attachment.frames.open_at(index);
            return Ok(());
        };
        let target = geometry.rest_offset(index);
        if animate && !attachment.reduced_motion {
            attachment.scroll.animate_to(
                target,
                MOVE_DURATION,
                SharedCurve::new(curves::EASE_IN_OUT),
            )
        } else {
            attachment.scroll.jump_to(target)
        }
    }

    /// Stops the carousel's autoplay until [`resume`](Self::resume) or the user's press on its
    /// pause control.
    pub fn pause(&self) {
        self.set_paused(true);
    }

    /// Lets the carousel's autoplay run again.
    pub fn resume(&self) {
        self.set_paused(false);
    }

    /// Whether autoplay is paused through this controller or the carousel's pause control.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.inner.paused.get()
    }

    pub(super) fn set_paused(&self, paused: bool) {
        if self.inner.paused.replace(paused) == paused {
            return;
        }
        // Cloned out: a listener that pauses again or drops its carousel runs with no borrow.
        let listeners: Vec<_> = self
            .inner
            .pause_listeners
            .borrow()
            .iter()
            .map(|(_, listener)| Rc::clone(listener))
            .collect();
        for listener in listeners {
            listener();
        }
    }

    /// Calls `callback` with the new active item after each change, once per change; it
    /// replaces any callback set before.
    pub fn on_index_changed<F, R>(&self, callback: F)
    where
        F: Fn(&mut EventCx<'_>, usize) -> R + 'static,
        R: EventOutcome,
    {
        *self.inner.callback.borrow_mut() = Some(value_callback(callback));
    }

    /// The callback installed now.
    pub(super) fn callback(&self) -> Option<ValueCallback<usize>> {
        self.inner.callback.borrow().clone()
    }

    /// Takes the controller for a carousel; `None` while another carousel holds it.
    pub(super) fn attach(&self, mut attachment: Attachment) -> Option<u64> {
        if self.inner.attachment.borrow().is_some() {
            return None;
        }
        let token = self.inner.next_token.get() + 1;
        self.inner.next_token.set(token);
        attachment.token = token;
        if let Some(index) = self.inner.pending.take()
            && attachment.count > 0
        {
            attachment.frames.open_at(index.min(attachment.count - 1));
        }
        *self.inner.attachment.borrow_mut() = Some(attachment);
        Some(token)
    }

    /// Replaces what the carousel holding `token` drives, after it rebuilt.
    pub(super) fn update(&self, attachment: Attachment) {
        let mut current = self.inner.attachment.borrow_mut();
        if current
            .as_ref()
            .is_some_and(|current| current.token == attachment.token)
        {
            *current = Some(attachment);
        }
    }

    /// The carousel holding `token` lets go.
    pub(super) fn detach(&self, token: u64) {
        let outgoing = {
            let mut current = self.inner.attachment.borrow_mut();
            if current
                .as_ref()
                .is_some_and(|current| current.token == token)
            {
                self.inner.index.set(None);
                current.take()
            } else {
                None
            }
        };
        drop(outgoing);
    }

    /// Records `index` as active for the carousel holding `token`; whether it changed.
    pub(super) fn commit(&self, token: u64, index: usize) -> bool {
        let holds = self
            .inner
            .attachment
            .borrow()
            .as_ref()
            .is_some_and(|attachment| attachment.token == token);
        holds && self.inner.index.replace(Some(index)) != Some(index)
    }

    pub(super) fn add_pause_listener(&self, listener: Rc<dyn Fn()>) -> u64 {
        let id = self.inner.next_token.get() + 1;
        self.inner.next_token.set(id);
        self.inner.pause_listeners.borrow_mut().push((id, listener));
        id
    }

    pub(super) fn remove_pause_listener(&self, id: u64) {
        let removed = {
            let mut listeners = self.inner.pause_listeners.borrow_mut();
            listeners
                .iter()
                .position(|(listener_id, _)| *listener_id == id)
                .map(|position| listeners.remove(position))
        };
        drop(removed);
    }

    pub(super) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}
