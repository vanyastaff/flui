//! Hold on Down (ADR-0208 rule 4): a press on a moving scrollable stops it.
//!
//! The recognizer joins its contact's arena as a hold. The arena rejects the
//! contact's members that do not move, a tap or a long press, and refuses
//! such members joining later, so the press that stopped the motion never
//! becomes a tap; a drag or a scroll still competes and takes the contact
//! from the hold. Nothing is taken out of hit testing: which members lose is
//! the arena's rule, not the widget's.
//!
//! Attach it only while the scrollable moves, for instance through a
//! listener's admission predicate, and stop the motion in
//! [`on_hold`](HoldGestureRecognizerBuilder::on_hold).

use std::rc::{Rc, Weak};

use flui_foundation::geometry::Offset;
use flui_platform_api::input_capabilities::PointerKindSet;

use super::{
    ArenaMembership, CancelOutcome, GestureRecognizer, PrimaryContact,
    callback_containment::{invoke_callback, retire_callbacks},
    recognizer::{admission_kind, is_primary_down},
};
use crate::{
    events::{PointerEvent, PointerEventExt, PointerKind},
    gesture::arena::{GestureArena, GestureArenaMember},
    gesture::settings::GestureSettingsProvider,
    ids::PointerId,
    pointer::PointerDispatch,
};

/// How a hold ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HoldEnd {
    /// The contact lifted while the hold had it.
    Lifted,
    /// A moving member, a drag or a scroll, took the contact.
    Yielded,
    /// The contact or the recognizer was cancelled.
    Cancelled,
}

/// The contact that started a hold.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct HoldDetails {
    /// Root-space Down position.
    pub global_position: Offset<f64>,
    /// Receiving node's Down position.
    pub local_position: Offset<f64>,
    /// Device kind of the contact.
    pub kind: PointerKind,
}

type HoldCallback = Rc<dyn Fn(HoldDetails)>;
type ReleaseCallback = Rc<dyn Fn(HoldEnd)>;

#[derive(Default)]
#[expect(
    clippy::struct_field_names,
    reason = "callback fields name their public notifications"
)]
struct HoldCallbacks {
    on_hold: Option<HoldCallback>,
    on_release: Option<ReleaseCallback>,
}

impl Drop for HoldCallbacks {
    fn drop(&mut self) {
        retire_callbacks!(self; on_hold, on_release);
    }
}

/// Holds a pressed contact on Down; see the [module](self) documentation.
pub struct HoldGestureRecognizer {
    contact: PrimaryContact,
    settings: GestureSettingsProvider,
    allowed_devices: PointerKindSet,
    callbacks: HoldCallbacks,
}

impl std::fmt::Debug for HoldGestureRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HoldGestureRecognizer")
            .field("contact", &self.contact)
            .finish_non_exhaustive()
    }
}

/// Construction policy, frozen before the recognizer gains shared ownership.
#[must_use]
pub struct HoldGestureRecognizerBuilder {
    arena: GestureArena,
    allowed_devices: PointerKindSet,
    callbacks: HoldCallbacks,
}

impl std::fmt::Debug for HoldGestureRecognizerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HoldGestureRecognizerBuilder")
            .finish_non_exhaustive()
    }
}

impl HoldGestureRecognizerBuilder {
    /// Admit only contacts of these device kinds (ADR-0208 rule 1).
    pub fn allowed_devices(mut self, devices: PointerKindSet) -> Self {
        self.allowed_devices = devices;
        self
    }

    /// Called once the hold joined its contact's arena, after the members it
    /// beats were rejected: stop the motion here.
    pub fn on_hold(mut self, callback: impl Fn(HoldDetails) + 'static) -> Self {
        self.callbacks.on_hold = Some(Rc::new(callback));
        self
    }

    /// Called once per hold when it ends: resume or settle the motion here
    /// unless a drag of the same scrollable took the contact.
    pub fn on_release(mut self, callback: impl Fn(HoldEnd) + 'static) -> Self {
        self.callbacks.on_release = Some(Rc::new(callback));
        self
    }

    /// Build an owner-local recognizer.
    pub fn build(self) -> Rc<HoldGestureRecognizer> {
        Rc::new_cyclic(|this: &Weak<HoldGestureRecognizer>| {
            let member: Weak<dyn GestureArenaMember> = this.clone();
            HoldGestureRecognizer {
                contact: PrimaryContact::new(
                    ArenaMembership::new(self.arena, member).holding_contact(),
                ),
                settings: GestureSettingsProvider::default(),
                allowed_devices: self.allowed_devices,
                callbacks: self.callbacks,
            }
        })
    }
}

impl HoldGestureRecognizer {
    /// Begin construction.
    pub fn builder(arena: GestureArena) -> HoldGestureRecognizerBuilder {
        HoldGestureRecognizerBuilder {
            arena,
            allowed_devices: PointerKindSet::all(),
            callbacks: HoldCallbacks::default(),
        }
    }

    fn release(&self, end: HoldEnd) {
        invoke_callback(self.callbacks.on_release.clone(), || {}, |callback| {
            callback(end);
        });
    }
}

impl GestureRecognizer for HoldGestureRecognizer {
    fn allowed_devices(&self) -> PointerKindSet {
        self.allowed_devices
    }

    fn add_pointer(&self, down: PointerDispatch<'_>) {
        if !is_primary_down(down.local) {
            return;
        }
        let (Some(local_position), Some(global_position)) =
            (down.local.position(), down.global.position())
        else {
            return;
        };
        let kind = admission_kind(down.local);
        let settings = self.settings.snapshot_for(kind);
        if self.contact.begin(down, &settings).is_err() {
            return;
        }
        invoke_callback(self.callbacks.on_hold.clone(), || {}, |callback| {
            callback(HoldDetails {
                global_position,
                local_position,
                kind,
            });
        });
    }

    fn handle_event(&self, dispatch: PointerDispatch<'_>) {
        if !dispatch
            .local
            .pointer_id()
            .is_some_and(|pointer| self.contact.tracks(pointer))
        {
            return;
        }
        match dispatch.local {
            PointerEvent::Up(_) => {
                if self.contact.finish().is_some() {
                    self.release(HoldEnd::Lifted);
                }
            }
            PointerEvent::Cancel(_) => {
                let _ = self.cancel();
            }
            _ => {}
        }
    }

    fn cancel(&self) -> CancelOutcome {
        if self.contact.cancel().is_some() {
            self.release(HoldEnd::Cancelled);
            CancelOutcome::Cancelled
        } else {
            CancelOutcome::Idle
        }
    }
}

impl GestureArenaMember for HoldGestureRecognizer {
    // A hold that wins keeps the contact until it lifts.
    fn accept_gesture(&self, _: PointerId) {}

    fn reject_gesture(&self, pointer: PointerId) {
        if self.contact.tracks(pointer) && self.contact.withdraw().is_some() {
            self.release(HoldEnd::Yielded);
        }
    }
}
