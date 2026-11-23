//! The viewer's leaf of the nested scroll channel: the contact and contactless
//! sequences it dispatches through the chain, and the gesture bookkeeping that
//! spans the scale updates of one interaction.

#[cfg(doc)]
use super::types::PanAxis;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_foundation::InputSequenceId;
use flui_foundation::geometry::{Axis, Offset, Point, Vec2};
use flui_foundation::panic::FirstPanic;
use flui_interaction::gesture::GestureSettingsProvider;
use flui_interaction::gesture::RemainderHandoff;
use flui_platform::input::keyboard::Code;
use flui_platform::input::pointer::PointerKind;

use super::InteractiveViewer;
use super::types::{EndCallback, StartCallback, UpdateCallback};
use crate::scroll::nested::{
    ChainNode, PlaneSource, ScrollCause, ScrollDevice, ScrollEndReason, ScrollSequence,
    SequenceSource,
};

/// The in-flight gesture's bookkeeping across the scale updates of one
/// interaction. It lives in the element's state, so a rebuild that swaps in
/// fresh closures mid-gesture continues the same gesture.
#[derive(Debug)]
pub(super) struct GestureTracking {
    /// Viewport-local position at the most recent transform `on_start`.
    /// `None` between gestures.
    pub(super) pan_start_local: Cell<Option<Offset<f64>>>,
    /// The axis a [`PanAxis::Aligned`] drag has locked to, established from
    /// the first non-zero movement of the gesture. `None` before that, and
    /// reset to `None` at the end of every gesture.
    pub(super) current_axis: Cell<Option<Axis>>,
    pub(super) scale: Cell<f64>,
    pub(super) rotation: Cell<f64>,
    /// The gesture is a native trackpad pinch, not contacts: it transforms
    /// the viewer directly instead of through the channel.
    pub(super) native: Cell<bool>,
}

impl Default for GestureTracking {
    fn default() -> Self {
        Self {
            pan_start_local: Cell::new(None),
            current_axis: Cell::new(None),
            scale: Cell::new(1.0),
            rotation: Cell::new(0.0),
            native: Cell::new(false),
        }
    }
}

impl GestureTracking {
    pub(super) fn reset(&self) {
        self.pan_start_local.set(None);
        self.current_axis.set(None);
        self.scale.set(1.0);
        self.rotation.set(0.0);
    }
}

#[derive(Clone, Default)]
pub(super) struct ViewerCallbacks {
    pub(super) start: Option<StartCallback>,
    pub(super) update: Option<UpdateCallback>,
    pub(super) end: Option<EndCallback>,
}

impl ViewerCallbacks {
    pub(super) fn of(view: &InteractiveViewer) -> Self {
        Self {
            start: view.on_interaction_start.clone(),
            update: view.on_interaction_update.clone(),
            end: view.on_interaction_end.clone(),
        }
    }
}

/// The viewer's place in the nested scroll channel: the chain above it, the
/// binding's sequence issuer, and the sequences it leads.
#[derive(Default)]
pub(super) struct ViewerLeaf {
    pub(super) parent: RefCell<Option<Rc<ChainNode>>>,
    pub(super) sequences: SequenceSource,
    pub(super) settings: RefCell<GestureSettingsProvider>,
    /// The device of the last contact that went down on the viewer.
    pub(super) device: Cell<Option<ScrollDevice>>,
    /// The live contact's sequence: from its pan's start until it lifts.
    pub(super) contact: RefCell<Option<Rc<ScrollSequence>>>,
    /// The latest contactless sequence: a wheel latch, or a key held down.
    pub(super) contactless: RefCell<Option<Contactless>>,
    /// The plane the viewer's deltas are local to: its own box, outside its
    /// transform.
    pub(super) plane: PlaneSource,
}

/// The input a contactless step of the viewer comes from.
#[derive(Debug, Clone, Copy)]
pub(super) enum ContactlessInput {
    /// A key, whose auto-repeats continue its sequence.
    Key(Code),
    /// A wheel or trackpad packet pointing at this point of the viewer's box.
    Wheel(Offset<f64>),
}

pub(super) struct Contactless {
    pub(super) sequence: Rc<ScrollSequence>,
    /// The key whose press began it; its repeats continue it.
    pub(super) key: Option<Code>,
}

impl ViewerLeaf {
    pub(super) fn mint(&self) -> Option<InputSequenceId> {
        self.sequences.mint()
    }

    /// The sequence the binding is dispatching input of, or a fresh one.
    pub(super) fn dispatched_or_mint(&self) -> Option<InputSequenceId> {
        self.sequences.dispatched_or_mint()
    }

    pub(super) fn remainder_rule(&self, device: ScrollDevice) -> RemainderHandoff {
        match device {
            ScrollDevice::Pointer { kind, .. } => self
                .settings
                .borrow()
                .snapshot_for(kind)
                .remainder_handoff(),
            _ => RemainderHandoff::NextSequence,
        }
    }

    /// Snapshot the chain above the viewer for `id` and tell it the
    /// sequence started.
    pub(super) fn begin(
        &self,
        id: InputSequenceId,
        device: ScrollDevice,
        contact: bool,
        failure: &mut FirstPanic,
    ) -> Rc<ScrollSequence> {
        let parent = self.parent.borrow().clone();
        let sequence = ScrollSequence::above(
            parent.as_ref(),
            self.plane.clone(),
            id,
            device,
            contact,
            self.remainder_rule(device),
        );
        sequence.start(failure);
        sequence
    }

    pub(super) fn end_contactless(&self, reason: ScrollEndReason, failure: &mut FirstPanic) {
        let outgoing = self.contactless.borrow_mut().take();
        if let Some(outgoing) = outgoing {
            outgoing.sequence.finish_input();
            outgoing.sequence.end(reason, failure);
        }
    }

    /// A contact's pan started on the viewer: its sequence begins, and the
    /// contactless one it interrupts ends.
    pub(super) fn begin_contact(&self) {
        let mut failure = FirstPanic::new();
        let outgoing = self.contact.borrow_mut().take();
        if let Some(outgoing) = outgoing {
            outgoing.cancel_contact();
            outgoing.end(ScrollEndReason::Cancelled, &mut failure);
        }
        self.end_contactless(ScrollEndReason::Released, &mut failure);
        if let Some(id) = self.dispatched_or_mint() {
            let device = self.device.get().unwrap_or(ScrollDevice::Pointer {
                kind: PointerKind::Touch,
                device: None,
            });
            let sequence = self.begin(id, device, true, &mut failure);
            *self.contact.borrow_mut() = Some(sequence);
        }
        failure.resume();
    }

    /// One delta of the live contact at `focal` (the viewer's box) through
    /// the chain; without a sequence (identities exhausted) the viewer alone
    /// takes it.
    pub(super) fn contact_step(
        &self,
        available: Vec2,
        focal: Offset<f64>,
        consume: impl FnOnce(Vec2) -> Vec2,
    ) -> Vec2 {
        let sequence = self.contact.borrow().clone();
        let Some(sequence) = sequence.filter(|sequence| sequence.is_live()) else {
            return consume(available);
        };
        sequence.aim(Point::new(focal.dx, focal.dy));
        let mut failure = FirstPanic::new();
        let consumed =
            sequence.dispatch(0, ScrollCause::UserInput, available, consume, &mut failure);
        failure.resume();
        consumed
    }

    /// The axes the live contact's viewer holds.
    pub(super) fn contact_holds(&self) -> [bool; 2] {
        self.contact
            .borrow()
            .as_ref()
            .map_or([false, false], |sequence| sequence.leaf_holds())
    }

    /// The contact lifted with `velocity` (physical channel axes): the
    /// release settles every scrollable the pan moved. Returns the sequence,
    /// for the viewer's own inertia, and the axes the viewer holds. A
    /// participant's failure stays in `failure`: the caller resumes it only
    /// once the sequence has a run or an end.
    pub(super) fn release(
        &self,
        velocity: Vec2,
        failure: &mut FirstPanic,
    ) -> Option<(Rc<ScrollSequence>, [bool; 2])> {
        let sequence = self.contact.borrow_mut().take()?;
        sequence.release(velocity, failure);
        let held = sequence.leaf_holds();
        Some((sequence, held))
    }

    /// The contact was cancelled: its end settles every scrollable the pan
    /// moved from rest.
    pub(super) fn cancel(&self) {
        let Some(sequence) = self.contact.borrow_mut().take() else {
            return;
        };
        let mut failure = FirstPanic::new();
        sequence.cancel_contact();
        sequence.end(ScrollEndReason::Cancelled, &mut failure);
        failure.resume();
    }

    /// Whether `id` is the contactless sequence the viewer leads now.
    pub(super) fn leads(&self, id: InputSequenceId) -> bool {
        self.contactless
            .borrow()
            .as_ref()
            .is_some_and(|current| current.sequence.id() == id && !current.sequence.is_ended())
    }

    /// One step of contactless sequence `id`: the one the viewer leads, or a
    /// new one that ends it.
    pub(super) fn contactless_step(
        &self,
        id: InputSequenceId,
        device: ScrollDevice,
        input: ContactlessInput,
        cause: ScrollCause,
        available: Vec2,
        consume: impl FnOnce(Vec2) -> Vec2,
    ) -> Vec2 {
        let mut failure = FirstPanic::new();
        let current = self
            .contactless
            .borrow()
            .as_ref()
            // An id is one sequence: a packet still naming one whose latch
            // lapsed pans the viewer alone.
            .filter(|current| current.sequence.id() == id)
            .map(|current| Rc::clone(&current.sequence));
        let sequence = if let Some(sequence) = current {
            sequence
        } else {
            self.end_contactless(ScrollEndReason::Released, &mut failure);
            let sequence = self.begin(id, device, false, &mut failure);
            *self.contactless.borrow_mut() = Some(Contactless {
                sequence: Rc::clone(&sequence),
                key: match input {
                    ContactlessInput::Key(code) => Some(code),
                    ContactlessInput::Wheel(_) => None,
                },
            });
            sequence
        };
        if let ContactlessInput::Wheel(pointer) = input {
            sequence.aim(Point::new(pointer.dx, pointer.dy));
        }
        let consumed = sequence.dispatch(0, cause, available, consume, &mut failure);
        failure.resume();
        consumed
    }

    /// A wheel or trackpad packet reached the viewer, panning it or not:
    /// the sequence it leads for the packet's latch ends where the binding
    /// says the latch ends (ADR-0209 §5).
    pub(super) fn wheel_latch(&self) {
        self.sequences.follow_dispatched_latch(|id| {
            self.contactless
                .borrow()
                .as_ref()
                .filter(|current| current.key.is_none() && current.sequence.id() == id)
                .map(|current| Rc::clone(&current.sequence))
        });
    }

    /// The key sequence `code` held is continued by its auto-repeat.
    pub(super) fn key_sequence(&self, code: Code) -> Option<InputSequenceId> {
        self.contactless
            .borrow()
            .as_ref()
            .filter(|current| current.key == Some(code) && !current.sequence.is_ended())
            .map(|current| current.sequence.id())
    }

    /// `code` was released: the sequence it held ends.
    pub(super) fn key_released(&self, code: Code) {
        if self.key_sequence(code).is_some() {
            let mut failure = FirstPanic::new();
            self.end_contactless(ScrollEndReason::Released, &mut failure);
            failure.resume();
        }
    }

    /// A one-step sequence of its own: an assistive scroll action.
    pub(super) fn one_shot(
        &self,
        device: ScrollDevice,
        available: Vec2,
        consume: impl FnOnce(Vec2) -> Vec2,
    ) -> Vec2 {
        let Some(id) = self.dispatched_or_mint() else {
            return consume(available);
        };
        let mut failure = FirstPanic::new();
        self.end_contactless(ScrollEndReason::Released, &mut failure);
        let sequence = self.begin(id, device, false, &mut failure);
        let consumed =
            sequence.dispatch(0, ScrollCause::UserInput, available, consume, &mut failure);
        sequence.finish_input();
        sequence.end(ScrollEndReason::Released, &mut failure);
        failure.resume();
        consumed
    }

    /// The viewer went away: what it leads ends cancelled.
    pub(super) fn retire(&self) {
        let mut failure = FirstPanic::new();
        // Taken before its end tells a participant: no borrow is held while
        // user code runs.
        let contact = self.contact.borrow_mut().take();
        if let Some(contact) = contact {
            contact.cancel_contact();
            contact.end(ScrollEndReason::Cancelled, &mut failure);
        }
        self.end_contactless(ScrollEndReason::Cancelled, &mut failure);
        failure.resume();
    }
}
