//! [`RawButton`] — a pressable area whose callback writes signals through the
//! [`EventCx`] it receives (ADR-0086).

use std::rc::Rc;

use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;

use super::actions::{
    Action, ActionOutcome, ActivateIntent, ActivationPressIntent, ButtonActivateIntent,
};
use super::focus_host::FocusConfig;
use super::node::{Description, InteractiveNode};
use crate::support::event_callback;
use crate::widget_state::WidgetStatesController;

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
/// is logged on the `flui.signals` target, not dropped silently.
///
/// Pressing it is a primary-button tap anywhere over the child (the tap
/// target is opaque); the keyboard's activation while it has focus — Enter,
/// or Space on its release ([`ActivateIntent`], [`ButtonActivateIntent`]);
/// or the platform's activate action on its semantics node (VoiceOver's
/// VO-Space, `AXPress`, UI Automation's `Invoke`). While focus is visible a
/// [`FocusRing`](super::FocusRing) marks it, in the inherited
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
    identifier: Option<std::sync::Arc<str>>,
    states: Option<WidgetStatesController>,
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
            identifier: None,
            states: None,
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

    /// Name the button's node for tests and automation (ADR-0240); see
    /// [`Semantics::identifier`](crate::Semantics::identifier). Never announced.
    #[must_use]
    pub fn identifier(mut self, identifier: impl Into<std::sync::Arc<str>>) -> Self {
        self.identifier = Some(identifier.into());
        self
    }

    /// Write the button's interaction states to `controller`: `Hovered`
    /// while a pointer is inside it, `Pressed`
    /// while a contact is down on it, `Focused` while it holds focus,
    /// `FocusVisible` while that focus is shown, and `Disabled` without an
    /// [`on_press`](Self::on_press). A design system draws its pressed and
    /// focused looks from the controller; the button clears what it wrote
    /// when it unmounts or is given another one.
    #[must_use]
    pub fn states_controller(mut self, controller: WidgetStatesController) -> Self {
        self.states = Some(controller);
        self
    }
}

/// The button's answer to the activation intents: the press callback. A
/// disabled button's node keeps the bindings declared and switched off, so
/// the intent stops at the button instead of activating an enclosing
/// control.
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
    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ActivateIntent) -> ActionOutcome {
        self.run(cx)
    }
}

impl Action<ButtonActivateIntent> for Press {
    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ButtonActivateIntent) -> ActionOutcome {
        self.run(cx)
    }
}

/// Space's key-down on an enabled button: consumed, so the press and the
/// release that activates belong to the button. It changes nothing — the
/// button has no pressed look.
impl Action<ActivationPressIntent> for Press {
    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &ActivationPressIntent) -> ActionOutcome {
        ActionOutcome::Performed
    }
}

impl StatelessView for RawButton {
    /// One `InteractiveNode` carries the tap target, the focus node, the
    /// activation bindings, the semantics and the focus ring.
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let enabled = self.on_press.is_some();
        let press = Rc::new(Press(self.on_press.clone()));
        // One node for assistive technology: the button.
        let description = Description {
            role: crate::SemanticsRole::Button,
            identifier: self.identifier.clone(),
        };
        // The bindings are the node's own, so its focus node records them
        // and the root bindings resolve the activation here (ADR-0079).
        let mut node = InteractiveNode::new(self.child.clone())
            .behavior(HitTestBehavior::Opaque)
            .enabled(enabled)
            .focus(FocusConfig {
                debug_label: Some("RawButton"),
                ..FocusConfig::new()
            })
            .focus_ring()
            .action::<ActivateIntent, Press>(&press)
            .action::<ButtonActivateIntent, Press>(&press)
            .action::<ActivationPressIntent, Press>(&press);
        // The node replaces its callbacks on every build, so a new closure
        // takes effect on the next press.
        if let Some(handler) = self.on_press.clone() {
            node = node.on_tap(handler);
        }
        if let Some(states) = self.states.clone() {
            node = node.states(states);
        }
        node.semantics(description)
    }
}
