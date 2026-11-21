//! The actions that answer [`ZoomIntent`] and [`PanIntent`] inside a viewer, and
//! the mount that places the viewer's gesture alternatives around its content.

use std::rc::Rc;

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{BoxedView, IntoView, View, ViewState};

use super::types::{PanIntent, ZoomIntent};
use super::viewer_core::ViewerCore;
use crate::{Action, ActionOutcome, Focus, GestureAlternatives};

/// Answers [`ZoomIntent`] for everything inside the viewer.
pub(super) struct ZoomAction(pub(super) Rc<ViewerCore>);

impl Action<ZoomIntent> for ZoomAction {
    fn invoke(&self, _ctx: &mut EventContext<'_>, intent: &ZoomIntent) -> ActionOutcome {
        if self.0.zoom_step(*intent) {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

/// Answers [`PanIntent`] for everything inside the viewer.
pub(super) struct PanAction(pub(super) Rc<ViewerCore>);

impl Action<PanIntent> for PanAction {
    fn invoke(&self, _ctx: &mut EventContext<'_>, intent: &PanIntent) -> ActionOutcome {
        if self.0.pan_step(*intent) {
            ActionOutcome::Performed
        } else {
            ActionOutcome::NotPerformed
        }
    }
}

/// Mounts the alternatives around the viewer's content, and resolves their
/// first command from below the viewer's own actions for the double tap.
#[derive(Clone)]
pub(super) struct AlternativesMount {
    pub(super) alternatives: GestureAlternatives,
    pub(super) core: Rc<ViewerCore>,
    pub(super) content: BoxedView,
}

impl std::fmt::Debug for AlternativesMount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlternativesMount")
            .field("alternatives", &self.alternatives)
            .finish_non_exhaustive()
    }
}

impl View for AlternativesMount {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

/// Keeps nothing between builds; its state holds the view-to-slot write out
/// of `build`.
pub(super) struct AlternativesMountState;

impl StatefulView for AlternativesMount {
    type State = AlternativesMountState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        AlternativesMountState
    }
}

impl ViewState<AlternativesMount> for AlternativesMountState {
    fn apply_view(&mut self, view: &AlternativesMount, ctx: &dyn BuildContext) {
        let outgoing = view
            .core
            .command
            .replace(Some(view.alternatives.handle(ctx)));
        drop(outgoing);
    }

    fn build(&self, view: &AlternativesMount, _ctx: &dyn BuildContext) -> impl IntoView {
        view.alternatives.clone().child(RowFocusProbe {
            core: Rc::clone(&view.core),
            child: view.content.clone(),
        })
    }
}

/// Records the focus node of the alternatives' row the viewer sits in.
#[derive(Clone)]
pub(super) struct RowFocusProbe {
    pub(super) core: Rc<ViewerCore>,
    pub(super) child: BoxedView,
}

impl std::fmt::Debug for RowFocusProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowFocusProbe").finish_non_exhaustive()
    }
}

impl View for RowFocusProbe {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

/// Keeps nothing between builds; its state holds the focus-node write out of
/// `build`.
pub(super) struct RowFocusProbeState;

impl StatefulView for RowFocusProbe {
    type State = RowFocusProbeState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        RowFocusProbeState
    }
}

impl ViewState<RowFocusProbe> for RowFocusProbeState {
    fn apply_view(&mut self, view: &RowFocusProbe, ctx: &dyn BuildContext) {
        let outgoing = view.core.row_focus.replace(Focus::maybe_of(ctx));
        drop(outgoing);
    }

    fn build(&self, view: &RowFocusProbe, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}
