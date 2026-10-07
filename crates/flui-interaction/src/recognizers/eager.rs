//! A reusable recognizer that claims an admitted contact immediately.
use super::{ArenaMembership, CancelOutcome, GestureRecognizer, PrimaryContact};
use crate::{
    arena::{GestureArena, GestureArenaMember},
    events::{PointerEvent, PointerEventExt},
    ids::PointerId,
    routing::PointerDispatch,
    settings::GestureSettings,
};
use std::rc::{Rc, Weak};

/// Wins the arena as soon as its primary contact is admitted.
#[derive(Debug)]
pub struct EagerGestureRecognizer {
    contact: PrimaryContact,
    settings: GestureSettings,
}

/// Construction policy, frozen before the recognizer gains shared ownership.
#[must_use]
#[derive(Debug)]
pub struct EagerGestureRecognizerBuilder {
    arena: GestureArena,
    settings: GestureSettings,
}

impl EagerGestureRecognizerBuilder {
    /// Set the settings snapshot captured by each admitted contact.
    pub fn settings(mut self, settings: GestureSettings) -> Self {
        self.settings = settings;
        self
    }
    /// Build an owner-local recognizer.
    pub fn build(self) -> Rc<EagerGestureRecognizer> {
        Rc::new_cyclic(|this: &Weak<EagerGestureRecognizer>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            EagerGestureRecognizer {
                contact: PrimaryContact::new(ArenaMembership::new(self.arena, member)),
                settings: self.settings,
            }
        })
    }
}

impl EagerGestureRecognizer {
    /// Begin construction with default gesture settings.
    pub fn builder(arena: GestureArena) -> EagerGestureRecognizerBuilder {
        EagerGestureRecognizerBuilder {
            arena,
            settings: GestureSettings::default(),
        }
    }
}

impl GestureRecognizer for EagerGestureRecognizer {
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        let pointer = down.local.pointer_id();
        let _span = tracing::info_span!(
            "eager.add_pointer",
            pointer = ?pointer,
            event = %crate::observability::GestureEvent::RecognizerAdded,
        );
        if self.contact.begin(down, &self.settings).is_ok() {
            self.contact.accept();
        }
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let _span = tracing::info_span!(
            "eager.handle_event",
            kind = %crate::observability::pointer_event_kind(dispatch.local),
            event = %crate::observability::GestureEvent::EventReceived,
        );
        if !self.contact.tracks(dispatch.local.pointer_id()) {
            return;
        }
        match dispatch.local {
            PointerEvent::Up(_) => {
                self.contact.finish();
            }
            PointerEvent::Cancel(_) => {
                self.cancel();
            }
            _ => {}
        }
    }
    fn cancel(&self) -> CancelOutcome {
        if self.contact.cancel().is_some() {
            CancelOutcome::Cancelled
        } else {
            CancelOutcome::Idle
        }
    }
}

impl GestureArenaMember for EagerGestureRecognizer {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, pointer: PointerId) {
        if self.contact.tracks(pointer) {
            self.contact.withdraw();
        }
    }
}
