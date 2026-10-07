//! Weak, ordered recognizer attachment shared by pointer listeners.
use super::{callback_containment::finish_containment, recognizer::GestureRecognizer};
use crate::{
    events::PointerEvent,
    retain::Retain,
    routing::{PointerDispatch, RoutePanic},
};
use std::rc::{Rc, Weak};

type Admission = Rc<dyn Fn(PointerDispatch<'_>) -> bool>;
#[derive(Clone)]
struct Attached {
    recognizer: Weak<dyn GestureRecognizer>,
    admit: Option<Admission>,
}

/// Ordered weak recognizer attachments; widget state retains strong ownership.
#[derive(Clone, Default)]
pub struct RecognizerSet {
    entries: Vec<Attached>,
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
        admit: Option<Admission>,
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
        let mut first = None;
        for entry in &self.entries {
            let Some(recognizer) = entry.recognizer.upgrade() else {
                continue;
            };
            let candidate = RoutePanic::capture(|| {
                if matches!(dispatch.local, PointerEvent::Down(_)) {
                    if entry.admit.as_ref().is_none_or(|admit| admit(dispatch)) {
                        recognizer.add_pointer(dispatch);
                    }
                } else {
                    recognizer.handle_event(dispatch);
                }
            });
            RoutePanic::preserve_first(&mut first, candidate, "recognizer attachment");
            if first.is_some() {
                recognizer.retain();
            } else {
                let candidate = RoutePanic::capture(|| drop(recognizer));
                RoutePanic::preserve_first(
                    &mut first,
                    candidate,
                    "recognizer attachment retirement",
                );
            }
        }
        finish_containment(first, incoming_failure);
    }
    /// Whether no recognizers have been attached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Drop for RecognizerSet {
    fn drop(&mut self) {
        let mut first = None;
        let incoming_failure = std::thread::panicking();
        for entry in &mut self.entries {
            super::callback_containment::retire_callback(entry.admit.take(), &mut first);
        }
        finish_containment(first, incoming_failure);
    }
}
