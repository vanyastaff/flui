//! [`InteractiveNode`]: one element for the behaviours of a control.
//!
//! A control built from the public wrappers mounts an element and a render
//! object for each of them: `Semantics`, `GestureDetector` (three elements
//! of its own), `Actions`, `Focus` (three more) and `FocusRing`. The wrappers
//! stay the way an application composes behaviour around its own widgets; a
//! control of this crate, which always wants the same set, mounts this node
//! instead: one element with one [`RenderInteractive`] (ADR-0322 §1).
//!
//! The node restates none of the wrappers' logic. Its state owns the hosts
//! they run on ([`FocusHost`], [`RecognizerHost`]) and drives the parts of
//! its render object with the views that drive them alone ([`Listener`],
//! [`Semantics`]), so a control on the node behaves as the same control
//! composed from wrappers does: the same recognizers in the arena, the same
//! focus node, the same semantics.
//!
//! It is not an authoring surface. It has no styling and no layout, and
//! nothing outside this crate names it.

use std::any::TypeId;
use std::rc::Rc;

use flui_foundation::panic::FirstPanic;
use flui_interaction::focus::FocusNode;
use flui_objects::{FocusRingStyle, RenderInteractive, RenderListener, SubtreeAnchor};
use flui_platform::TargetPlatform;
use flui_rendering::RenderUpdateImpact;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::protocol::BoxProtocol;
use flui_view::environment::PlatformConvention;
use flui_view::prelude::*;
use flui_view::{
    Provided, RenderObjectContext, RenderView, RenderViewState, StatefulRenderView,
    impl_stateful_render_view,
};

use super::actions::{
    Action, ActionChain, EnclosingActions, ErasedAction, FocusRecord, Intent, layered_chain,
    shared_action,
};
use super::focus::render_box_rect_provider;
use super::focus_host::{FocusConfig, FocusHost};
use super::focus_ring::DefaultFocusRingStyle;
use super::gesture_detector::TapSlot;
use super::listener::release_targets;
use super::raw_gesture_detector::RecognizerHost;
use crate::support::EventCallback;
use crate::{Listener, Semantics};

/// The behaviours of a control around its child, in one element.
///
/// Every part is optional and an absent one costs nothing: no recognizer, no
/// focus node, no semantics configuration, no ring.
#[derive(Clone)]
pub(crate) struct InteractiveNode {
    behavior: HitTestBehavior,
    /// The one switch of ADR-0322 §2.
    enabled: bool,
    on_tap: Option<EventCallback>,
    focus: Option<FocusConfig>,
    focus_ring: bool,
    /// Shared, so a clone of the view copies no list.
    actions: Rc<Vec<(TypeId, ErasedAction)>>,
    semantics: Option<Semantics>,
    child: BoxedView,
}

impl std::fmt::Debug for InteractiveNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractiveNode")
            .field("behavior", &self.behavior)
            .field("enabled", &self.enabled)
            .field("on_tap", &self.on_tap.is_some())
            .field("focus", &self.focus.is_some())
            .field("actions", &self.actions.len())
            .field("semantics", &self.semantics.is_some())
            .finish_non_exhaustive()
    }
}

impl InteractiveNode {
    /// A node around `child` with no behaviour: hit where its child is, and
    /// transparent to everything else.
    pub(crate) fn new(child: BoxedView) -> Self {
        Self {
            behavior: HitTestBehavior::DeferToChild,
            enabled: true,
            on_tap: None,
            focus: None,
            focus_ring: false,
            actions: Rc::default(),
            semantics: None,
            child,
        }
    }

    /// Where the node is hit, as [`Listener::behavior`] says.
    #[must_use]
    pub(crate) fn behavior(mut self, behavior: HitTestBehavior) -> Self {
        self.behavior = behavior;
        self
    }

    /// Whether the control is enabled: the only switch of its parts
    /// (ADR-0322 §2). While `false` no recognizer joins an arena, the focus
    /// node cannot take focus and gives up the focus it holds, every bound
    /// action answers `is_enabled` `false` and stays declared, and the
    /// semantics say disabled and offer no action. The node is still hit
    /// where it was, so a disabled control covers what is beneath it.
    #[must_use]
    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The tap the pointer part answers: the control's, while enabled. The
    /// recognizer and the click the semantics offer both follow it, so a
    /// disabled control has neither, and a contact in flight when the switch
    /// turns ends in nothing.
    fn live_tap(&self) -> Option<EventCallback> {
        self.on_tap.clone().filter(|_| self.enabled)
    }

    /// The focus part's configuration: the control's own, conjoined with
    /// the switch.
    fn focus_config(&self) -> Option<FocusConfig> {
        self.focus.as_ref().map(|config| FocusConfig {
            can_request_focus: Some(config.can_request_focus.unwrap_or(true) && self.enabled),
            ..config.clone()
        })
    }

    /// The bindings the node declares: its own while enabled, and the same
    /// types switched off while disabled, so an intent stops at the control
    /// and does not activate an enclosing one.
    fn declared_actions(&self) -> Rc<Vec<(TypeId, ErasedAction)>> {
        if self.enabled {
            return Rc::clone(&self.actions);
        }
        Rc::new(
            self.actions
                .iter()
                .map(|(intent, action)| (*intent, action.switched_off()))
                .collect::<Vec<(TypeId, ErasedAction)>>(),
        )
    }

    /// Call `callback` on a primary tap, recognized as
    /// [`GestureDetector::on_tap`](crate::GestureDetector::on_tap)
    /// recognizes it.
    #[must_use]
    pub(crate) fn on_tap(mut self, callback: EventCallback) -> Self {
        self.on_tap = Some(callback);
        self
    }

    /// Host a focus node configured by `config`, as [`Focus`](crate::Focus)
    /// does: the node publishes it to the subtree, and its semantics say
    /// whether it can take focus and whether it has it.
    ///
    /// Whether the node hosts a focus node is decided when it mounts. A
    /// control that stops taking focus says so in the configuration
    /// (`can_request_focus`), and keeps the node.
    #[must_use]
    pub(crate) fn focus(mut self, config: FocusConfig) -> Self {
        self.focus = Some(config);
        self
    }

    /// Draw the focus ring over the child while the hosted focus node has
    /// visible focus, in the inherited [`DefaultFocusRingStyle`], as a
    /// [`FocusRing`](crate::FocusRing) under the `Focus` does.
    #[must_use]
    pub(crate) fn focus_ring(mut self) -> Self {
        self.focus_ring = true;
        self
    }

    /// Bind intent type `T` to `action` for the hosted focus node and the
    /// subtree, as an [`Actions`](crate::Actions) around a `Focus` does.
    ///
    /// A control binds one action to several intent types, so the node
    /// takes it shared: each binding names the same action, and none holds
    /// a copy.
    #[must_use]
    pub(crate) fn action<T: Intent, A: Action<T> + 'static>(mut self, action: &Rc<A>) -> Self {
        Rc::make_mut(&mut self.actions).push(shared_action::<T, A>(action));
        self
    }

    /// Describe the node to assistive technology as `semantics` says, as a
    /// [`Semantics`] around the control does. Its child is not read: the
    /// node's child is the one annotated.
    #[must_use]
    pub(crate) fn semantics(mut self, semantics: Semantics) -> Self {
        self.semantics = Some(semantics);
        self
    }
}

/// The hosts of a mounted [`InteractiveNode`].
pub(crate) struct InteractiveNodeState {
    tap: TapSlot,
    recognizers: RecognizerHost,
    focus: Option<FocusHost>,
    /// The node's own bindings, as the latest view gave them.
    own_actions: Rc<Vec<(TypeId, ErasedAction)>>,
    /// The chain above the node, read in lifecycle hooks.
    ambient_actions: Option<ActionChain>,
    /// The node's bindings layered over the ambient chain. `None` while the
    /// node binds nothing: the ambient chain is then the one in force, and
    /// the node publishes none.
    chain: Option<ActionChain>,
    /// The platform convention the node is built under, recorded with the
    /// chain on the focus node. `None` before `init_state`.
    platform: Option<TargetPlatform>,
    ring_style: FocusRingStyle,
    /// Names the render object, for the focus node to measure.
    anchor: SubtreeAnchor,
}

impl InteractiveNodeState {
    fn layer_actions(&mut self) {
        self.chain = (!self.own_actions.is_empty())
            .then(|| layered_chain(self.ambient_actions.clone(), Rc::clone(&self.own_actions)));
    }

    /// What the hosted focus node records: the chain in force at the node
    /// and the platform convention (ADR-0079).
    fn focus_record(&self) -> Option<FocusRecord> {
        let chain = self
            .chain
            .clone()
            .or_else(|| self.ambient_actions.clone())?;
        Some(FocusRecord::new(chain, self.platform?))
    }

    fn read_dependencies(&mut self, cx: &dyn LifecycleContext) {
        self.ambient_actions = EnclosingActions::depend_on(cx);
        self.platform = Some(PlatformConvention::of(cx));
        self.ring_style = DefaultFocusRingStyle::of(cx);
        self.layer_actions();
    }

    fn focus_node(&self) -> Option<&Rc<FocusNode>> {
        self.focus.as_ref().map(FocusHost::node)
    }

    /// The listener that drives the pointer part: the recognizers, fed
    /// contacts only, as a `GestureDetector` feeds them.
    fn listener(&self, view: &InteractiveNode) -> Listener {
        Listener::new()
            .behavior(view.behavior)
            .contact_recognizers()
            .recognizers(self.recognizers.recognizer_set(None))
    }

    /// The semantics that drive the semantics part, assembled in one order
    /// (ADR-0322 §6): the view's properties, then the activation its tap
    /// recognizer gives, then the focus state a `Focus` annotates its
    /// subtree with. A node that cannot take focus says nothing about focus,
    /// and a node with none of the three has no semantics part.
    fn semantics(&self, view: &InteractiveNode) -> Option<Semantics> {
        // The description never carries the switch itself (ADR-0322 §2).
        let mut semantics = view
            .semantics
            .clone()
            .map(|described| described.enabled(view.enabled));
        if self.tap.is_set() {
            semantics = Some(semantics.unwrap_or_default().on_click(self.tap.click()));
        }
        let focus = self
            .focus
            .as_ref()
            .filter(|focus| focus.node().can_request_focus());
        if let Some(focus) = focus {
            let annotated = semantics
                .unwrap_or_default()
                .focusable(true)
                .focused(focus.node().has_primary_focus());
            semantics = Some(match focus.assistive_focus_request() {
                Some(request) => annotated.on_focus(move |_cx| request()),
                None => annotated,
            });
        }
        semantics
    }
}

impl StatefulRenderView for InteractiveNode {
    type Protocol = BoxProtocol;
    type RenderObject = RenderInteractive;
    type State = InteractiveNodeState;

    fn create_state(&self) -> Self::State {
        let tap = TapSlot::new(self.live_tap());
        InteractiveNodeState {
            recognizers: RecognizerHost::new(tap.factory().as_slice()),
            tap,
            focus: self.focus_config().as_ref().map(FocusHost::new),
            own_actions: self.declared_actions(),
            ambient_actions: None,
            chain: None,
            platform: None,
            ring_style: FocusRingStyle::BUILT_IN,
            anchor: SubtreeAnchor::new(),
        }
    }

    /// The object registers nothing here: its targets deliver to what the
    /// state acquires in `init_state`, so the first build registers them.
    fn create_render_object(&self, _cx: &RenderObjectContext<'_>) -> Self::RenderObject {
        RenderInteractive::new(RenderListener::new(None, self.behavior))
    }

    fn did_unmount_render_object(
        &self,
        cx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        release_targets(cx, render_object.pointer_mut());
        if let Some(part) = render_object.semantics_mut() {
            crate::semantics::release_part(cx, part);
        }
    }
}

impl RenderViewState<InteractiveNode> for InteractiveNodeState {
    fn init_state(&mut self, cx: &dyn LifecycleContext) {
        self.read_dependencies(cx);
        self.recognizers.init(cx);
        let record = self.focus_record();
        if let Some(focus) = &mut self.focus {
            let anchor = self.anchor.clone();
            focus.init(
                cx,
                render_box_rect_provider(move || anchor.get(), cx),
                record,
            );
        }
    }

    fn did_change_dependencies(&mut self, cx: &dyn LifecycleContext) {
        self.read_dependencies(cx);
        self.recognizers.dependencies_changed(cx);
        let record = self.focus_record();
        if let Some(focus) = &mut self.focus {
            focus.dependencies_changed(cx, record);
        }
    }

    /// The recognizers follow the new view first; a callback of a retired
    /// one that panics does not leave the focus node on the old
    /// configuration.
    fn did_update_view(&mut self, _old: &InteractiveNode, new: &InteractiveNode) {
        let mut failure = FirstPanic::new();
        failure.contain(|| {
            self.tap.set(new.live_tap());
            self.recognizers.update(self.tap.factory().as_slice());
        });
        // The replaced bindings drop here; an action is user code.
        failure.contain(|| self.own_actions = new.declared_actions());
        self.layer_actions();
        let record = self.focus_record();
        if let (Some(focus), Some(config)) = (&mut self.focus, &new.focus_config()) {
            failure.contain(|| {
                focus.update(config);
                focus.record(record);
            });
        }
        failure.resume();
    }

    fn update_render_object(
        &self,
        view: &InteractiveNode,
        cx: &RenderObjectContext<'_>,
        render_object: &mut RenderInteractive,
    ) -> RenderUpdateImpact {
        let mut impact =
            RenderView::update_render_object(&self.listener(view), cx, render_object.pointer_mut());
        match (self.semantics(view), render_object.semantics_mut()) {
            (Some(semantics), Some(part)) => {
                impact |= RenderView::update_render_object(&semantics, cx, part);
            }
            (Some(semantics), None) => {
                let part = RenderView::create_render_object(&semantics, cx);
                impact |= render_object.set_semantics(Some(part)).1;
            }
            (None, Some(_)) => {
                let (previous, removed) = render_object.set_semantics(None);
                if let Some(mut previous) = previous {
                    crate::semantics::release_part(cx, &mut previous);
                }
                impact |= removed;
            }
            (None, None) => {}
        }
        let ring = view.focus_ring
            && self
                .focus_node()
                .is_some_and(|node| node.has_visible_focus());
        render_object.set_anchor(self.focus.is_some().then(|| self.anchor.clone()));
        impact |= render_object.set_focus_ring(ring.then_some(self.ring_style));
        impact
    }

    fn provide(&self, _view: &InteractiveNode, _cx: &dyn BuildContext, values: &mut Provided<'_>) {
        if let Some(focus) = &self.focus {
            focus.provide(values);
        }
        if let Some(chain) = &self.chain {
            values.value(EnclosingActions::new(chain.clone()));
        }
    }

    fn child(&self, view: &InteractiveNode, _cx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }

    /// A recognizer's cancellation runs its callbacks. One that panics does
    /// not keep the focus node attached: both hosts are disposed, and the
    /// first failure is raised after the second.
    fn dispose(&mut self) {
        let mut failure = FirstPanic::new();
        failure.contain(|| self.recognizers.dispose());
        // A request from assistive technology that arrives late finds no
        // callback; the outgoing closure is user code.
        failure.contain(|| self.tap.set(None));
        if let Some(focus) = &mut self.focus {
            failure.contain(|| focus.dispose());
        }
        failure.resume();
    }
}

impl_stateful_render_view!(InteractiveNode);

/// The node's switch is reachable from a mounted tree only through
/// `RawButton`, whose disabled form has no press callback to begin with.
/// The pointer and action rows of the switch, for a control that keeps its
/// callback while disabled, are checked here on the view.
#[cfg(test)]
mod switch_tests {
    use std::any::TypeId;
    use std::rc::Rc;

    use flui_view::{EventCx, ViewExt};

    use super::InteractiveNode;
    use crate::support::event_callback;
    use crate::{Action, ActionOutcome, ActivateIntent, SizedBox};

    struct Willing;

    impl Action<ActivateIntent> for Willing {
        fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &ActivateIntent) -> ActionOutcome {
            ActionOutcome::Performed
        }
    }

    fn node(enabled: bool) -> InteractiveNode {
        InteractiveNode::new(SizedBox::new(10.0, 10.0).boxed())
            .on_tap(event_callback(|_cx| {}))
            .action::<ActivateIntent, Willing>(&Rc::new(Willing))
            .enabled(enabled)
    }

    #[test]
    fn a_disabled_node_answers_no_tap_though_it_holds_the_callback() {
        assert!(node(true).live_tap().is_some());
        assert!(node(false).live_tap().is_none());
    }

    #[test]
    fn a_disabled_node_declares_its_bindings_switched_off() {
        for (enabled, runs) in [(true, true), (false, false)] {
            let declared = node(enabled).declared_actions();
            let [(intent, action)] = declared.as_slice() else {
                panic!("one binding stays declared, enabled or not");
            };
            assert_eq!(*intent, TypeId::of::<ActivateIntent>());
            assert_eq!(action.is_enabled(&ActivateIntent), runs);
        }
    }
}
