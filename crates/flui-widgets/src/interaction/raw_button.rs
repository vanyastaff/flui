//! [`RawButton`] — a pressable area whose callback writes signals through the
//! [`EventCx`] it receives (ADR-0086).

use std::rc::Rc;

use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;

use super::actions::{
    Action, ActionOutcome, Actions, ActivateIntent, ActivationPressIntent, ButtonActivateIntent,
};
use super::focus::Focus;
use super::focus_ring::FocusRing;
use crate::support::event_callback;
use crate::{GestureDetector, Semantics};

/// A press callback, already adapted to report its outcome.
type PressCallback = Rc<dyn Fn(&mut EventCx<'_>)>;

/// A theme-free button: its child, a tap target over it, keyboard focus and
/// activation, and the semantics of a button.
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
/// target is opaque); the keyboard's activation while it has focus — Enter,
/// or Space on its release ([`ActivateIntent`], [`ButtonActivateIntent`]);
/// or the platform's activate action on its semantics node (VoiceOver's
/// VO-Space, `AXPress`, UI Automation's `Invoke`). While focus is visible a
/// [`FocusRing`] marks it, in the inherited
/// [`DefaultFocusRingStyle`](super::focus_ring::DefaultFocusRingStyle).
///
/// Without [`on_press`](Self::on_press) the button is disabled: it takes no
/// focus, its node says so and offers no action, a tap does nothing, and an
/// activation key reaching it keeps bubbling.
///
/// It is a design-agnostic press primitive with no Material styling and no
/// pressed or hovered look; see `ARCHITECTURE.md` "Mapping decisions".
#[derive(Clone, StatelessView)]
pub struct RawButton {
    on_press: Option<PressCallback>,
    child: BoxedView,
}

impl std::fmt::Debug for RawButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawButton")
            .field("enabled", &self.on_press.is_some())
            .finish_non_exhaustive()
    }
}

impl RawButton {
    /// A button showing `child`, disabled until it has an
    /// [`on_press`](Self::on_press) callback.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self {
            on_press: None,
            child: child.into_view().boxed(),
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

/// The button's answer to the activation intents: the press callback, or —
/// disabled — a declared but disabled mapping, so the intent stops here
/// instead of activating an enclosing control.
#[derive(Clone)]
struct Press(Option<PressCallback>);

impl Press {
    fn run(&self, cx: &mut EventCx<'_>) -> ActionOutcome {
        match &self.0 {
            Some(on_press) => {
                on_press(cx);
                ActionOutcome::Performed
            }
            None => ActionOutcome::NotPerformed,
        }
    }
}

impl Action<ActivateIntent> for Press {
    fn is_enabled(&self, _intent: &ActivateIntent) -> bool {
        self.0.is_some()
    }

    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ActivateIntent) -> ActionOutcome {
        self.run(cx)
    }
}

impl Action<ButtonActivateIntent> for Press {
    fn is_enabled(&self, _intent: &ButtonActivateIntent) -> bool {
        self.0.is_some()
    }

    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ButtonActivateIntent) -> ActionOutcome {
        self.run(cx)
    }
}

/// Space's key-down on an enabled button: consumed, so the press and the
/// release that activates belong to the button. It changes nothing — the
/// button has no pressed look.
impl Action<ActivationPressIntent> for Press {
    fn is_enabled(&self, _intent: &ActivationPressIntent) -> bool {
        self.0.is_some()
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &ActivationPressIntent) -> ActionOutcome {
        ActionOutcome::Performed
    }
}

impl StatelessView for RawButton {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let enabled = self.on_press.is_some();
        let ring = FocusRing::new(self.child.clone());
        // The `Actions` sit above the `Focus`, so the focused node records
        // them and the root bindings resolve the activation here (ADR-0079).
        let press = Press(self.on_press.clone());
        let actions = Actions::new(
            Focus::new(ring)
                .can_request_focus(enabled)
                .debug_label("RawButton"),
        )
        .action::<ActivateIntent>(press.clone())
        .action::<ButtonActivateIntent>(press.clone())
        .action::<ActivationPressIntent>(press);
        let mut detector = GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .child(actions);
        // `GestureDetector` refreshes its tap slot on every build, so a new
        // closure takes effect on the next press.
        if let Some(handler) = self.on_press.clone() {
            detector = detector.on_tap(move |cx| handler(cx));
        }
        // One node for assistive technology: the button, which the focus
        // annotation below joins.
        Semantics::new()
            .container(true)
            .role(crate::SemanticsRole::Button)
            .enabled(enabled)
            .child(detector)
    }
}
