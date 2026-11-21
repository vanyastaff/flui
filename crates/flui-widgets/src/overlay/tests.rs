//! Unit tests for the overlay's private machinery that need no mounted tree:
//! the entry-list ownership rule, the onstage build plan and
//! [`OverlayScope`]'s notification predicate.
//!
//! The mounted-tree suite lives in `crates/flui-widgets/tests/overlay.rs`;
//! the headless harness is not compiled into this crate's unit-test build
//! (ADR-0083 §4).

use flui_view::prelude::*;

use super::{OnstagePlan, OverlayEntry, onstage_plan};
use crate::SizedBox;

/// The `skipCount` handed to the theater is the number of *covered but
/// maintained* entries, and they are always the leading children — the property
/// [`RenderTheater`](flui_objects::RenderTheater) relies on.
///
/// Asserted on [`onstage_plan`] directly: the element tree cannot observe
/// `skip_count`, and `harness_theater_*` in `flui-objects` covers what the render
/// object then does with it.
#[test]
fn overlay_build_plan_skips_covered_maintained_entries() {
    crate::support::test_cases::run_cases(
        "overlay_build_plan_skips_covered_maintained_entries",
        &[
            (
                "covered_maintained_entry_build_plan",
                covered_maintained_entry_build_plan as fn(),
            ),
            (
                "overlay_entry_identity_exhaustion_preserves_existing_entries",
                super::entry::overlay_entry_identity_exhaustion_preserves_existing_entries,
            ),
        ],
    );
}

fn covered_maintained_entry_build_plan() {
    let plain = || OverlayEntry::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed());
    let opaque = || plain().with_opaque(true);
    let maintained = || plain().with_maintain_state(true);
    let maintained_opaque = || plain().with_opaque(true).with_maintain_state(true);

    let modal = || plain().with_modal(true);
    let maintained_modal = || plain().with_maintain_state(true).with_modal(true);

    // (build, skip_count, semantics_blocked)
    let plan = |entries: &[OverlayEntry]| {
        let OnstagePlan {
            build,
            skip_count,
            semantics_blocked,
        } = onstage_plan(entries);
        (build, skip_count, semantics_blocked)
    };

    assert_eq!(
        plan(&[plain(), plain()]),
        (vec![0, 1], 0, 0),
        "nothing opaque: every entry onstage, a plain expanding stack"
    );
    assert_eq!(
        plan(&[plain(), opaque()]),
        (vec![1], 0, 0),
        "the covered entry is dropped, not skipped — it never enters the tree"
    );
    assert_eq!(
        plan(&[maintained(), opaque()]),
        (vec![0, 1], 1, 0),
        "a maintained covered entry is built and then skipped by the theater"
    );
    assert_eq!(
        plan(&[maintained(), plain(), opaque(), plain()]),
        (vec![0, 2, 3], 1, 0),
        "only entries below the topmost opaque one are covered; the \
         non-maintained one among them is dropped"
    );
    assert_eq!(
        plan(&[maintained(), maintained_opaque(), opaque()]),
        (vec![0, 1, 2], 2, 0),
        "an opaque entry that is itself covered is still skipped, not dropped"
    );
    assert_eq!(plan(&[]), (vec![], 0, 0));

    assert_eq!(
        plan(&[plain(), plain(), modal()]),
        (vec![0, 1, 2], 0, 2),
        "a modal entry hides every onstage entry below it from semantics"
    );
    assert_eq!(
        plan(&[plain(), modal(), plain()]),
        (vec![0, 1, 2], 0, 1),
        "an entry above the modal one stays reachable"
    );
    assert_eq!(
        plan(&[plain(), modal(), modal()]),
        (vec![0, 1, 2], 0, 2),
        "the topmost modal entry decides; a dialog over a dialog hides both below"
    );
    assert_eq!(
        plan(&[maintained_modal(), opaque(), plain()]),
        (vec![0, 1, 2], 1, 1),
        "a covered modal entry hides nothing beyond what is already offstage"
    );
}

/// An entry's builder may own a value whose `Drop` calls back into the
/// overlay: the overlay holds no borrow of its cells when that `Drop` runs, so
/// every call it makes lands instead of meeting a `BorrowMutError`.
#[test]
fn an_entry_builder_drop_may_reenter_its_overlay() {
    use super::{InsertPosition, OverlayHandle};

    struct ReenterOnDrop {
        overlay: OverlayHandle,
        late: OverlayEntry,
    }
    impl Drop for ReenterOnDrop {
        fn drop(&mut self) {
            self.overlay.insert(&self.late, &InsertPosition::Top);
            self.late.set_opaque(true);
            self.late.mark_needs_build();
            self.overlay.rearrange(std::slice::from_ref(&self.late));
            self.late.remove();
            self.overlay.insert(&self.late, &InsertPosition::Top);
        }
    }

    let overlay = OverlayHandle::new();
    let late = OverlayEntry::new(|_ctx| SizedBox::new(1.0, 1.0).into_view().boxed());
    let reenter = ReenterOnDrop {
        overlay: overlay.clone(),
        late: late.clone(),
    };
    let removed = OverlayEntry::new(move |_ctx| {
        std::hint::black_box(&reenter);
        SizedBox::new(1.0, 1.0).into_view().boxed()
    });
    let kept = OverlayEntry::new(|_ctx| SizedBox::new(1.0, 1.0).into_view().boxed());
    overlay.insert(&removed, &InsertPosition::Top);
    overlay.insert(&kept, &InsertPosition::Top);
    overlay.rearrange(&[kept.clone(), removed.clone()]);
    removed.remove();
    assert_eq!(overlay.ids_bottom_to_top(), vec![kept.id()]);

    // The last handle: the builder, and the value it owns, drop here.
    drop(removed);
    assert_eq!(
        overlay.ids_bottom_to_top(),
        vec![kept.id(), late.id()],
        "every call the dropping builder made landed"
    );
    assert!(late.is_attached() && late.opaque());
}
