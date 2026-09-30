//! [`RawButton`] — a pressable area whose callback writes signals through the
//! [`EventCx`] it receives (ADR-0086).

use std::rc::Rc;

use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;

use crate::support::event_callback;
use crate::{GestureDetector, Semantics};

/// A press callback, already adapted to report its outcome.
type PressCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

/// A theme-free button: its child, a tap target over it, and the semantics of
/// a button.
///
/// The press callback receives `&mut EventCx<'_>`, the write capability of
/// ADR-0086, so it writes a signal directly:
///
/// ```rust,ignore
/// RawButton::new(Text::new("Increment")).on_press(move |cx| count.update(cx, |n| *n += 1))
/// ```
///
/// The callback may return `()` or the `Result` of a write; a refused write
/// is logged on the `flui::signals` target, not dropped silently.
///
/// Pressing it is a primary-button tap anywhere over the child (the tap
/// target is opaque), or the platform's activate action on its semantics
/// node (VoiceOver's VO-Space, `AXPress`, UI Automation's `Invoke`). Without
/// [`on_press`](Self::on_press) the button is disabled: its node says so and
/// offers no action, and a tap does nothing.
///
/// The button holds no state: it forwards the callback to
/// [`GestureDetector::on_tap`], whose own [`WriterSource`] opens the
/// [`EventCx`] for each press.
///
/// It is a design-agnostic press primitive with no Material styling; see
/// `ARCHITECTURE.md` "Mapping decisions".
/// Keyboard activation and pressed or hovered state are not implemented yet.
#[derive(Clone, StatelessView)]
pub struct RawButton {
    on_press: Option<PressCallback>,
    child: Child,
}

impl std::fmt::Debug for RawButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawButton")
            .field("enabled", &self.on_press.is_some())
            .field("child", &self.child)
            .finish()
    }
}

impl RawButton {
    /// A button showing `child`, disabled until it has an
    /// [`on_press`](Self::on_press) callback.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            on_press: None,
            child: Child::some(child.into_view()),
        }
    }

    /// Call `callback` when the button is pressed, with the dispatch's
    /// `&mut EventCx<'_>`.
    #[must_use]
    pub fn on_press<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>) -> R + 'static,
        R: EventOutcome,
    {
        self.on_press = Some(event_callback(callback));
        self
    }
}

impl StatelessView for RawButton {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut detector = GestureDetector::new().behavior(HitTestBehavior::Opaque);
        // `GestureDetector` refreshes its tap slot on every build, so a new
        // closure takes effect on the next press.
        if let Some(handler) = self.on_press.clone() {
            detector = detector.on_tap(move |cx| handler(cx));
        }
        if let Some(child) = self.child.clone().into_inner() {
            detector = detector.child(child);
        }
        Semantics::new()
            .container(true)
            .button(true)
            .enabled(self.on_press.is_some())
            .child(detector)
    }
}
