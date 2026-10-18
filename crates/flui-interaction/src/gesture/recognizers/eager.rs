//! A reusable recognizer that claims an admitted contact immediately.
use super::{ArenaMembership, CancelOutcome, GestureRecognizer, PrimaryContact};
use crate::gesture::observer::GestureContest;
use crate::{
    events::{PointerEvent, PointerEventExt},
    gesture::arena::{GestureArena, GestureArenaMember},
    gesture::settings::GestureSettingsProvider,
    pointer::PointerDispatch,
};
use flui_platform_api::input_capabilities::PointerKindSet;
use std::rc::{Rc, Weak};

/// Wins the arena as soon as its primary contact is admitted.
#[derive(Debug)]
pub struct EagerGestureRecognizer {
    contact: PrimaryContact,
    settings: GestureSettingsProvider,
    allowed_devices: PointerKindSet,
}

/// Construction policy, frozen before the recognizer gains shared ownership.
#[must_use]
#[derive(Debug)]
pub struct EagerGestureRecognizerBuilder {
    arena: GestureArena,
    settings: GestureSettingsProvider,
    allowed_devices: PointerKindSet,
}

impl EagerGestureRecognizerBuilder {
    /// Set the settings snapshot captured by each admitted contact.
    pub fn settings(mut self, settings: impl Into<GestureSettingsProvider>) -> Self {
        self.settings = settings.into();
        self
    }

    /// Admit only contacts of these device kinds, before any arena rule runs
    /// (ADR-0208 rule 1): a pen that draws beside a finger that scrolls. The
    /// full set, the default, admits every device.
    pub fn allowed_devices(mut self, devices: PointerKindSet) -> Self {
        self.allowed_devices = devices;
        self
    }
    /// Build an owner-local recognizer.
    pub fn build(self) -> Rc<EagerGestureRecognizer> {
        Rc::new_cyclic(|this: &Weak<EagerGestureRecognizer>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            EagerGestureRecognizer {
                contact: PrimaryContact::new(ArenaMembership::new(self.arena, member)),
                settings: self.settings,
                allowed_devices: self.allowed_devices,
            }
        })
    }
}

impl EagerGestureRecognizer {
    /// Begin construction with default gesture settings.
    pub fn builder(arena: GestureArena) -> EagerGestureRecognizerBuilder {
        EagerGestureRecognizerBuilder {
            arena,
            settings: GestureSettingsProvider::default(),
            allowed_devices: PointerKindSet::all(),
        }
    }
}

impl GestureRecognizer for EagerGestureRecognizer {
    fn allowed_devices(&self) -> PointerKindSet {
        self.allowed_devices
    }
    fn add_pointer(&self, down: PointerDispatch<'_>) {
        let Some(pointer) = down.local.pointer_id() else {
            return;
        };
        let _span = tracing::info_span!(
            "eager.add_pointer",
            pointer = ?pointer,
            event = %crate::gesture::observability::GestureEvent::RecognizerAdded,
        );
        let settings = self
            .settings
            .snapshot_for(super::recognizer::admission_kind(down.local));
        if self.contact.begin(down, &settings).is_ok() {
            self.contact.accept();
        }
    }
    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        let _span = tracing::info_span!(
            "eager.handle_event",
            kind = %crate::gesture::observability::pointer_event_kind(dispatch.local),
            event = %crate::gesture::observability::GestureEvent::EventReceived,
        );
        if !dispatch
            .local
            .pointer_id()
            .is_some_and(|pointer| self.contact.tracks(pointer))
        {
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
    fn accept_gesture(&self, _: GestureContest) {}
    fn reject_gesture(&self, contest: GestureContest) {
        if self.contact.tracks_contest(contest) {
            self.contact.withdraw();
        }
    }
}
