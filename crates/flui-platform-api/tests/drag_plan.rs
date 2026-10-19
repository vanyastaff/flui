//! The drag start and escalation decision, and the identity representation (ADR-0211).

use std::num::NonZeroU64;
use std::sync::Arc;

use flui_foundation::{DragSessionId, IdMint, UiRuntimeId};
use flui_platform_api::data_transfer::{
    DataTransferOffer, DropFeedback, Mime, OfferRecord, OfferTable, TransferActions, TransferFormat,
};
use flui_platform_api::drag::{
    AllowedActions, DragError, DragMoment, DragReach, DragSessionKey, IDENTITY_ESSENCE,
    OsDragStart, OsOffer, OsSessionPlan, ProcessNonce, plan_os_session, validate_session,
};

const COPY: TransferActions = TransferActions::COPY;
const MOVE: TransferActions = TransferActions::MOVE;
const AT_LEAVE: OsDragStart = OsDragStart::OnLeavingWindow;
const AT_START: OsDragStart = OsDragStart::AtStart { eager_data: false };

fn key() -> DragSessionKey {
    let session = IdMint::<DragSessionId>::new().next().expect("fresh mint");
    DragSessionKey::new(
        ProcessNonce::from_raw(NonZeroU64::new(0xfeed_beef).expect("nonzero")),
        UiRuntimeId::new(3),
        session,
    )
}

fn plan(
    start: OsDragStart,
    moment: DragMoment,
    reach: DragReach,
    allowed: AllowedActions,
    sibling: bool,
) -> Result<Option<OsSessionPlan>, DragError> {
    plan_os_session(start, moment, reach, allowed, sibling, true)
}

fn offer_of(
    representations: Vec<flui_platform_api::data_transfer::RepresentationDescriptor>,
) -> DataTransferOffer {
    let mut table = OfferTable::new();
    let id = table.mint(OfferRecord::new(Arc::from(representations)));
    DataTransferOffer::new(
        id,
        Arc::clone(table.get(id).expect("live").representations()),
    )
}

#[test]
fn the_identity_survives_the_offer() {
    let key = key();
    let offer = offer_of(vec![key.representation()]);
    assert_eq!(DragSessionKey::from_offer(&offer), Some(key));
}

#[test]
fn a_foreign_offer_carries_no_identity() {
    assert_eq!(DragSessionKey::from_offer(&offer_of(Vec::new())), None);
}

#[test]
fn the_identity_format_is_known_by_its_media_type_alone() {
    assert!(key().representation().format.is_drag_identity());
    for (format, identity) in [
        (custom(IDENTITY_ESSENCE), true),
        (custom("application/x-flui-drag-session-other"), false),
        (custom("text/plain"), false),
        (TransferFormat::Text, false),
    ] {
        assert_eq!(format.is_drag_identity(), identity, "{format:?}");
    }
}

/// A target's cached answer resolves to the one action a drop takes: the
/// accepted actions the source permits, copy before move before link.
#[test]
fn a_drop_answer_resolves_to_one_permitted_action() {
    const LINK: TransferActions = TransferActions::LINK;
    for (accept, allowed, resolved) in [
        (Some(COPY | MOVE), COPY | MOVE, Some(COPY)),
        (Some(COPY | MOVE), MOVE | LINK, Some(MOVE)),
        (Some(COPY | MOVE | LINK), LINK, Some(LINK)),
        (Some(COPY), COPY, Some(COPY)),
        (Some(MOVE), COPY, None),
        (Some(TransferActions::empty()), COPY, None),
        (None, COPY, None),
    ] {
        assert_eq!(
            DropFeedback { accept }.resolve(allowed),
            resolved,
            "{accept:?} within {allowed:?}"
        );
    }
}

fn custom(mime: &str) -> TransferFormat {
    TransferFormat::Custom {
        mime: Mime::new(mime),
    }
}

#[test]
fn escalation_decision_table() {
    let outside = AllowedActions::new(MOVE, COPY);
    let within = AllowedActions::within(MOVE);
    // Leaving the window with outside actions offers everything and advertises both scopes.
    assert_eq!(
        plan(
            AT_LEAVE,
            DragMoment::LeavesWindow,
            DragReach::System,
            outside,
            false
        ),
        Ok(Some(OsSessionPlan {
            offer: OsOffer::Full,
            advertised: COPY | MOVE
        }))
    );
    // Inside the window nothing escalates on an at-leave backend.
    assert_eq!(
        plan(
            AT_LEAVE,
            DragMoment::Start,
            DragReach::System,
            outside,
            true
        ),
        Ok(None)
    );
    // No outside actions and no sibling: leaving is a move with no target.
    assert_eq!(
        plan(
            AT_LEAVE,
            DragMoment::LeavesWindow,
            DragReach::Runtime,
            within,
            false
        ),
        Ok(None)
    );
    // A sibling presentation makes an identity-only session advertising within_app.
    assert_eq!(
        plan(
            AT_LEAVE,
            DragMoment::LeavesWindow,
            DragReach::Runtime,
            within,
            true
        ),
        Ok(Some(OsSessionPlan {
            offer: OsOffer::IdentityOnly,
            advertised: MOVE
        }))
    );
    // A window-reach session never becomes an OS session.
    assert_eq!(
        plan(
            AT_LEAVE,
            DragMoment::LeavesWindow,
            DragReach::Window,
            outside,
            true
        ),
        Ok(None)
    );
    assert_eq!(
        plan(
            AT_START,
            DragMoment::Start,
            DragReach::Window,
            outside,
            true
        ),
        Ok(None)
    );
    // At-start backends decide at the start and never escalate mid-drag.
    assert_eq!(
        plan(
            AT_START,
            DragMoment::Start,
            DragReach::Runtime,
            within,
            true
        ),
        Ok(Some(OsSessionPlan {
            offer: OsOffer::IdentityOnly,
            advertised: MOVE
        }))
    );
    assert_eq!(
        plan(
            AT_START,
            DragMoment::LeavesWindow,
            DragReach::System,
            outside,
            true
        ),
        Ok(None)
    );
    assert_eq!(
        plan(
            AT_START,
            DragMoment::Start,
            DragReach::Runtime,
            within,
            false
        ),
        Ok(None)
    );
}

#[test]
fn a_backend_without_a_drag_source_refuses_what_needs_the_os() {
    let outside = AllowedActions::new(MOVE, COPY);
    assert_eq!(
        plan_os_session(
            AT_LEAVE,
            DragMoment::LeavesWindow,
            DragReach::System,
            outside,
            false,
            false
        ),
        Err(DragError::Unavailable)
    );
    assert_eq!(
        plan_os_session(
            AT_LEAVE,
            DragMoment::Start,
            DragReach::System,
            outside,
            false,
            false
        ),
        Ok(None)
    );
}

#[test]
fn session_validation_refusals() {
    let outside = AllowedActions::new(MOVE, COPY);
    assert_eq!(
        validate_session(AllowedActions::default(), DragReach::Runtime, false),
        Err(DragError::NoAllowedActions)
    );
    assert_eq!(
        validate_session(AllowedActions::within(MOVE), DragReach::System, true),
        Err(DragError::NoOutsideActions)
    );
    assert_eq!(
        validate_session(outside, DragReach::System, false),
        Err(DragError::NoSerializedRepresentation)
    );
    // A narrower reach does not excuse outside actions nothing could redeem.
    assert_eq!(
        validate_session(outside, DragReach::Window, false),
        Err(DragError::NoSerializedRepresentation)
    );
    assert_eq!(validate_session(outside, DragReach::System, true), Ok(()));
    assert_eq!(
        validate_session(AllowedActions::within(MOVE), DragReach::Window, false),
        Ok(())
    );
}
