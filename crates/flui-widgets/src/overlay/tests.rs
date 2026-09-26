//! Unit tests for the overlay's private machinery that need no mounted tree:
//! the entry-list ownership rule, the onstage build plan and
//! [`OverlayScope`]'s notification predicate.
//!
//! The mounted-tree suite lives in `crates/flui-widgets/tests/overlay.rs`;
//! the headless harness is not compiled into this crate's unit-test build
//! (ADR-0083 §4).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_view::InheritedView;
use flui_view::prelude::*;

use super::{InsertPosition, OnstagePlan, OverlayEntry, OverlayHandle, OverlayScope, onstage_plan};
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

/// An entry is in one overlay, once. Inserting it into a second overlay, or
/// twice into one, is refused and logged, never a ghost copy: the entry stays
/// where it was, and `remove` still takes it out of there.
///
/// Red-check: make `admissible` return `candidates.to_vec()`; `b` gains the
/// entry, `a` keeps a copy `remove` can no longer reach, and `a` holds it twice.
#[test]
fn an_entry_already_in_an_overlay_is_refused_elsewhere_and_twice() {
    let calls = Calls::default();
    let entry = counting_entry(&calls);
    let (a, b) = (OverlayHandle::new(), OverlayHandle::new());

    a.insert(&entry, &InsertPosition::Top);
    b.insert(&entry, &InsertPosition::Top);
    b.rearrange(std::slice::from_ref(&entry));
    a.insert(&entry, &InsertPosition::Top);
    a.insert_all(&[entry.clone(), entry.clone()], &InsertPosition::Top);

    assert_eq!(
        a.ids_bottom_to_top(),
        vec![entry.id()],
        "a holds it exactly once"
    );
    assert!(b.ids_bottom_to_top().is_empty(), "b refused it");

    entry.remove();
    assert!(
        a.ids_bottom_to_top().is_empty(),
        "remove reaches the one real owner"
    );
    b.insert(&entry, &InsertPosition::Top);
    assert_eq!(
        b.ids_bottom_to_top(),
        vec![entry.id()],
        "once removed, it may go elsewhere"
    );
}

/// The `skipCount` handed to the theater is the number of *covered but
/// maintained* entries, and they are always the leading children — the property
/// [`RenderTheater`](flui_objects::RenderTheater) relies on.
///
/// Asserted on [`onstage_plan`] directly: the element tree cannot observe
/// `skip_count`, and `harness_theater_*` in `flui-objects` covers what the render
/// object then does with it.
#[test]
fn overlay_build_plan_matches_flutters_onstage_loop() {
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

/// `OverlayScope::update_should_notify` is handle-**identity**, not
/// structural equality — the same handle (even a fresh clone of it) must not
/// notify, a different handle must.
///
/// This is a direct call, not a mounted-tree test: an `OverlayEntryView`
/// element is reconciled in place across ordinary rebuilds of the *same*
/// mounted entry, and its `overlay` field never changes across that entry's
/// lifetime, so there is no reachable production path that ever hands the
/// same mount point two different overlay identities to compare. The
/// `InheritedView` contract is still real and still worth pinning directly —
/// exactly the precedent `GestureArenaScope`'s own
/// `update_should_notify_is_always_false` test and `view/inherited.rs`'s
/// `test_inherited_element_update_should_notify` set.
///
/// Mutation-RUN: hardcode `update_should_notify` to always return `false` —
/// the second assertion below fails (`scope_b` must notify against `scope_a1`
/// but the stub reports it must not).
#[test]
fn overlay_scope_update_should_notify_is_true_only_on_handle_identity_change() {
    let handle_a = OverlayHandle::new();
    let handle_b = OverlayHandle::new();
    let scope_a1 = OverlayScope::new(handle_a.clone(), SizedBox::shrink());
    let scope_a2 = OverlayScope::new(handle_a.clone(), SizedBox::shrink());
    let scope_b = OverlayScope::new(handle_b, SizedBox::shrink());

    assert!(
        !scope_a2.update_should_notify(&scope_a1),
        "the same overlay handle identity must not notify dependents, even \
         across two separately constructed OverlayScope values"
    );
    assert!(
        scope_b.update_should_notify(&scope_a1),
        "a different overlay handle identity must notify dependents"
    );
}
