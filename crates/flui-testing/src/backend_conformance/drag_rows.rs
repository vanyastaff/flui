//! The drag-and-drop rows (ADR-0211): when a backend can start an OS drag session, and what it
//! claims about it.
//!
//! The decision itself is the pure function `plan_os_session` every runtime shares; these rows
//! evaluate it over what the backend declares, so a backend that claims a drag source it lacks,
//! or starts its sessions at another moment than it declares, fails.

use std::sync::Arc;

use flui_platform_api::data_transfer::TransferActions;
use flui_platform_api::drag::{
    AllowedActions, DragError, DragMoment, DragReach, OsDragStart, OsOffer, OsSessionPlan,
    PlatformDragSource, plan_os_session,
};
use flui_platform_api::input_capabilities::Availability;

use super::rows::Outcome;
use super::{BackendUnderTest, RowOutcome};

const OUTSIDE: AllowedActions = AllowedActions::new(TransferActions::MOVE, TransferActions::COPY);
const WITHIN: AllowedActions = AllowedActions::within(TransferActions::MOVE);

fn source<B: BackendUnderTest>(backend: &mut B) -> Result<Arc<dyn PlatformDragSource>, Outcome> {
    backend.drag_source().ok_or(Ok(RowOutcome::NotApplicable(
        "the backend starts no OS drag session",
    )))
}

fn at_start_timing<B: BackendUnderTest>(backend: &mut B) -> Result<OsDragStart, Outcome> {
    let source = source(backend)?;
    if matches!(backend.drag_start(), OsDragStart::AtStart { .. }) {
        Ok(source.start_timing())
    } else {
        Err(Ok(RowOutcome::NotApplicable(
            "the backend starts its session when the pointer leaves the window",
        )))
    }
}

/// A backend with a drag source claims outgoing drags, one without claims none, and a drag that
/// needs the OS on the latter fails with `DragError::Unavailable` instead of starting.
pub(super) fn outgoing_drag_is_delivered_as_claimed<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let claimed = backend.capabilities().outgoing_drag();
    let source = backend.drag_source();
    match (claimed, source) {
        (Availability::Unavailable, Some(_)) => {
            Err("starts OS drag sessions yet claims outgoing drags unavailable".to_owned())
        }
        (Availability::Native | Availability::Emulated, None) => Err(format!(
            "claims outgoing drags {claimed:?} but offers no drag source"
        )),
        (Availability::Unavailable, None) => {
            let refused = plan_os_session(
                backend.drag_start(),
                moment_of(backend.drag_start()),
                DragReach::System,
                OUTSIDE,
                false,
                false,
            );
            if refused == Err(DragError::Unavailable) {
                Ok(RowOutcome::Held)
            } else {
                Err(format!(
                    "a drag that needs the OS did not fail as unavailable: {refused:?}"
                ))
            }
        }
        (_, Some(_)) => Ok(RowOutcome::Held),
    }
}

/// The drag source starts its sessions when the backend declares it does.
pub(super) fn a_drag_source_starts_when_the_backend_declares<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let source = match source(backend) {
        Ok(source) => source,
        Err(outcome) => return outcome,
    };
    let (declared, actual) = (backend.drag_start(), source.start_timing());
    if declared == actual {
        Ok(RowOutcome::Held)
    } else {
        Err(format!(
            "declares {declared:?} but its source starts at {actual:?}"
        ))
    }
}

/// A backend that starts at lift never escalates mid-drag, and starts a full session at the
/// start when the drag reaches the system (UIKit's `UIDragSession`, the browser's `dragstart`).
pub(super) fn an_at_start_backend_never_escalates_mid_drag<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let timing = match at_start_timing(backend) {
        Ok(timing) => timing,
        Err(outcome) => return outcome,
    };
    let mid = plan_os_session(
        timing,
        DragMoment::LeavesWindow,
        DragReach::System,
        OUTSIDE,
        true,
        true,
    );
    if mid != Ok(None) {
        return Err(format!("escalated mid-drag: {mid:?}"));
    }
    let start = plan_os_session(
        timing,
        DragMoment::Start,
        DragReach::System,
        OUTSIDE,
        false,
        true,
    );
    match start {
        Ok(Some(OsSessionPlan {
            offer: OsOffer::Full,
            ..
        })) => Ok(RowOutcome::Held),
        other => Err(format!(
            "did not start a full session at the start: {other:?}"
        )),
    }
}

/// The start decision counts a sibling presentation of the source's runtime: with no outside
/// actions the session offers only the private identity and advertises `within_app`.
pub(super) fn start_decision_includes_sibling_presentation_with_identity_only_offer<
    B: BackendUnderTest,
>(
    backend: &mut B,
) -> Outcome {
    let timing = match at_start_timing(backend) {
        Ok(timing) => timing,
        Err(outcome) => return outcome,
    };
    let with_sibling = plan_os_session(
        timing,
        DragMoment::Start,
        DragReach::Runtime,
        WITHIN,
        true,
        true,
    );
    let expected = OsSessionPlan {
        offer: OsOffer::IdentityOnly,
        advertised: TransferActions::MOVE,
    };
    if with_sibling != Ok(Some(expected)) {
        return Err(format!("with a sibling presentation: {with_sibling:?}"));
    }
    let alone = plan_os_session(
        timing,
        DragMoment::Start,
        DragReach::Runtime,
        WITHIN,
        false,
        true,
    );
    if alone != Ok(None) {
        return Err(format!("with no sibling presentation: {alone:?}"));
    }
    Ok(RowOutcome::Held)
}

/// A backend that starts when the pointer leaves the window starts nothing before, and a full
/// session when it does leave with outside actions.
pub(super) fn a_leaving_backend_escalates_when_the_pointer_leaves_the_window<
    B: BackendUnderTest,
>(
    backend: &mut B,
) -> Outcome {
    let source = match source(backend) {
        Ok(source) => source,
        Err(outcome) => return outcome,
    };
    if !matches!(backend.drag_start(), OsDragStart::OnLeavingWindow) {
        return Ok(RowOutcome::NotApplicable(
            "the backend starts its session at the start of the drag",
        ));
    }
    let timing = source.start_timing();
    let early = plan_os_session(
        timing,
        DragMoment::Start,
        DragReach::System,
        OUTSIDE,
        true,
        true,
    );
    if early != Ok(None) {
        return Err(format!("started before the pointer left: {early:?}"));
    }
    let leaving = plan_os_session(
        timing,
        DragMoment::LeavesWindow,
        DragReach::System,
        OUTSIDE,
        false,
        true,
    );
    match leaving {
        Ok(Some(OsSessionPlan {
            offer: OsOffer::Full,
            ..
        })) => Ok(RowOutcome::Held),
        other => Err(format!(
            "did not start a full session on leaving: {other:?}"
        )),
    }
}

/// The moment a backend of `start` decides at.
fn moment_of(start: OsDragStart) -> DragMoment {
    match start {
        OsDragStart::OnLeavingWindow => DragMoment::LeavesWindow,
        OsDragStart::AtStart { .. } => DragMoment::Start,
    }
}
