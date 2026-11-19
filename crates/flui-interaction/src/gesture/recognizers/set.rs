//! Weak, ordered recognizer attachment shared by pointer listeners.
use super::{
    callback_containment::finish_containment,
    recognizer::{GestureRecognizer, admits_device},
};
use crate::containment::keep_first;
use crate::{
    containment::{FirstPanic, caught},
    events::{PointerEvent, TouchpadSwipePhase},
    pointer::PointerDispatch,
};
use smallvec::SmallVec;
use std::rc::{Rc, Weak};

/// Whether a contact's Down reaches a recognizer: the predicate a recognizer
/// is attached with ([`RecognizerSet::attach_when`]), and the one a widget's
/// contact admission holds. Asked of the Down only; the contact's later
/// events reach the recognizer whatever it answered for a Down it admitted.
///
/// It must be pure: a platform asks it too, between owner turns and outside
/// the UI runtime's scope ([`RecognizerSet::claims_press`] through a press
/// probe), where no signal can be read or written.
pub type DownAdmission = Rc<dyn Fn(PointerDispatch<'_>) -> bool>;

#[derive(Clone)]
struct Attached {
    recognizer: Weak<dyn GestureRecognizer>,
    admit: Option<DownAdmission>,
}

/// How `recognizer`, attached as `entry`, takes the Down `dispatch` carries
/// from a contact of `kind`: its device admitted, its own answer
/// ([`GestureRecognizer::takes_down`]), then its attachment's admission. The
/// one composition both [`RecognizerSet::dispatch`] and
/// [`RecognizerSet::claims_press`] ask, so delivery and a platform's press
/// question cannot disagree. The recognizer's answer is asked first: it is
/// pure, and the admission is user code.
fn admitted_take(
    entry: &Attached,
    recognizer: &dyn GestureRecognizer,
    dispatch: PointerDispatch<'_>,
    kind: crate::events::PointerKind,
) -> Option<super::recognizer::DownTake> {
    if !admits_device(recognizer.allowed_devices(), kind) {
        return None;
    }
    let take = recognizer.takes_down(dispatch)?;
    entry
        .admit
        .as_ref()
        .is_none_or(|admit| admit(dispatch))
        .then_some(take)
}

/// Ordered weak recognizer attachments; widget state retains strong ownership.
///
/// One attachment is held inline: a listener that feeds a single recognizer,
/// or a single group of them, allocates nothing for its set.
#[derive(Clone, Default)]
pub struct RecognizerSet {
    entries: SmallVec<[Attached; 1]>,
}

impl std::fmt::Debug for RecognizerSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecognizerSet")
            .field("attachments", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl RecognizerSet {
    /// Attach without retaining the recognizer's owning state.
    pub fn attach<R: GestureRecognizer + 'static>(&mut self, recognizer: &Rc<R>) {
        self.attach_inner(recognizer, None);
    }
    /// Filter Down admission only; terminal events still reach the recognizer.
    pub fn attach_when<R: GestureRecognizer + 'static>(
        &mut self,
        recognizer: &Rc<R>,
        admit: impl Fn(PointerDispatch<'_>) -> bool + 'static,
    ) {
        self.attach_inner(recognizer, Some(Rc::new(admit)));
    }
    fn attach_inner<R: GestureRecognizer + 'static>(
        &mut self,
        recognizer: &Rc<R>,
        admit: Option<DownAdmission>,
    ) {
        let erased: Rc<dyn GestureRecognizer> = recognizer.clone();
        self.entries.push(Attached {
            recognizer: Rc::downgrade(&erased),
            admit,
        });
    }
    /// Deliver in attachment order, resuming the first failure after all peers.
    pub fn dispatch(&self, dispatch: PointerDispatch<'_>) {
        let incoming_failure = std::thread::panicking();
        if incoming_failure {
            return;
        }
        let mut first = FirstPanic::new();
        for entry in &self.entries {
            let Some(recognizer) = entry.recognizer.upgrade() else {
                continue;
            };
            let candidate = caught(|| match dispatch.local {
                PointerEvent::Down(down) => {
                    // The seam holds the gate: a recognizer is offered only a
                    // Down it takes, whether or not its own `add_pointer`
                    // asks again.
                    if admitted_take(entry, &*recognizer, dispatch, down.pointer.kind).is_some() {
                        recognizer.add_pointer(dispatch);
                    }
                }
                PointerEvent::TouchpadSwipe(swipe) if swipe.phase == TouchpadSwipePhase::Start => {
                    if admits_device(recognizer.allowed_devices(), swipe.pointer().kind)
                        && entry.admit.as_ref().is_none_or(|admit| admit(dispatch))
                    {
                        recognizer.add_touchpad_swipe(dispatch);
                    }
                }
                _ => recognizer.handle_event(dispatch),
            });
            keep_first(&mut first, candidate, "recognizer attachment");
            let candidate = caught(|| drop(recognizer));
            keep_first(&mut first, candidate, "recognizer attachment retirement");
        }
        finish_containment(first, incoming_failure);
    }
    /// Whether a touchpad pan feeds any live recognizer here (ADR-0251 §2).
    #[must_use]
    pub fn feeds_scroll_contact(&self) -> bool {
        self.entries.iter().any(|entry| {
            entry
                .recognizer
                .upgrade()
                .is_some_and(|recognizer| recognizer.feeds_scroll_contact())
        })
    }
    /// Whether a live recognizer here would contend for the press the Down
    /// `dispatch` carries, asked as [`Self::dispatch`] would admit it and
    /// without delivering anything: the recognizer admits the contact's
    /// device, takes the Down as a press ([`GestureRecognizer::takes_down`]
    /// answers [`DownTake::Press`](super::recognizer::DownTake::Press), the
    /// gate [`Self::dispatch`] applies before `add_pointer`), and its
    /// admission predicate, if any, accepts it. A drag, which claims a contact only past its slop, never
    /// counts, admission or not. The predicates are user code and run with no
    /// borrow held; one that panics unwinds out of this call.
    #[must_use]
    pub fn claims_press(&self, dispatch: PointerDispatch<'_>) -> bool {
        let PointerEvent::Down(down) = dispatch.local else {
            return false;
        };
        self.entries.iter().any(|entry| {
            entry.recognizer.upgrade().is_some_and(|recognizer| {
                admitted_take(entry, &*recognizer, dispatch, down.pointer.kind)
                    == Some(super::recognizer::DownTake::Press)
            })
        })
    }
    /// Whether no recognizers have been attached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Drop for RecognizerSet {
    fn drop(&mut self) {
        let mut first = FirstPanic::new();
        let incoming_failure = std::thread::panicking();
        for entry in &mut self.entries {
            super::callback_containment::retire_callback(entry.admit.take(), &mut first);
        }
        finish_containment(first, incoming_failure);
    }
}
