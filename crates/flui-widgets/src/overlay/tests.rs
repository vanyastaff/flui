//! Unit tests for the overlay's private machinery that need no mounted tree:
//! the entry-list ownership rule, the onstage build plan and
//! [`OverlayScope`]'s notification predicate.
//!
//! The mounted-tree suite lives in `crates/flui-widgets/tests/overlay.rs`;
//! the headless harness is not compiled into this crate's unit-test build
//! (ADR-0083 §4).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_view::prelude::*;

use super::{OnstagePlan, OverlayEntry, onstage_plan};
use crate::SizedBox;

/// Counts how many times an entry's builder closure ran.
#[derive(Clone, Default)]
struct Calls(Arc<AtomicUsize>);

impl Calls {
    fn bump(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// An entry whose builder counts invocations and builds a leaf.
fn counting_entry(calls: &Calls) -> OverlayEntry {
    let calls = calls.clone();
    OverlayEntry::new(move |_ctx| {
        calls.bump();
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
}

/// The `skipCount` handed to the theater is the number of *covered but
/// maintained* entries, and they are always the leading children — the property
/// [`RenderTheater`](flui_objects::RenderTheater) relies on.
///
/// Asserted on [`onstage_plan`] directly: the element tree cannot observe
/// `skip_count`, and `harness_theater_*` in `flui-objects` covers what the render
/// object then does with it.
#[test]
fn overlay_build_plan_skips_covered_maintained_entries() {
    crate::contract_cases::run_cases(
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

    let plan = |entries: &[OverlayEntry]| onstage_plan(entries);

    assert_eq!(
        plan(&[plain(), plain()]),
        OnstagePlan {
            build: vec![0, 1],
            skip_count: 0
        },
        "nothing opaque: every entry onstage, a plain expanding stack"
    );
    assert_eq!(
        plan(&[plain(), opaque()]),
        OnstagePlan {
            build: vec![1],
            skip_count: 0
        },
        "the covered entry is dropped, not skipped — it never enters the tree"
    );
    assert_eq!(
        plan(&[maintained(), opaque()]),
        OnstagePlan {
            build: vec![0, 1],
            skip_count: 1
        },
        "a maintained covered entry is built and then skipped by the theater"
    );
    assert_eq!(
        plan(&[maintained(), plain(), opaque(), plain()]),
        OnstagePlan {
            build: vec![0, 2, 3],
            skip_count: 1
        },
        "only entries below the topmost opaque one are covered; the \
         non-maintained one among them is dropped"
    );
    assert_eq!(
        plan(&[maintained(), maintained_opaque(), opaque()]),
        OnstagePlan {
            build: vec![0, 1, 2],
            skip_count: 2
        },
        "an opaque entry that is itself covered is still skipped, not dropped"
    );
    assert_eq!(
        plan(&[]),
        OnstagePlan {
            build: vec![],
            skip_count: 0
        },
    );
}
