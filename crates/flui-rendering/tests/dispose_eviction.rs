//! Removal is the dispose site — dirty entries die WITH the subtree.
//!
//! A `Drop` impl has no `&PipelineOwner`, so it cannot evict
//! dirty-queue entries; `PipelineOwner::remove_render_object` is the
//! one place removal and owner-side disposal stay in lockstep.
//! Without it, every removal left stale queue entries for the next
//! phase to warn about — and any future retained state would have
//! dangled outright.

use flui_objects::RenderColoredBox;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::testing::SceneHost;

use crate::common::BoxedRenderObject;

pub(crate) fn removing_a_subtree_evicts_its_dirty_entries() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let parent = owner.insert(Box::new(RenderColoredBox::red(10.0, 10.0)) as BoxedRenderObject);
    let child = owner
        .insert_child_render_object(parent, Box::new(RenderColoredBox::blue(10.0, 10.0)))
        .expect("child insert");
    owner.set_root_id(Some(parent));

    // insert already queued both; pile on explicit marks too.
    owner.mark_needs_layout(child);
    owner.mark_needs_paint(child);
    assert!(owner.has_dirty_nodes());

    let removed = owner.remove_render_object(parent);
    assert_eq!(removed, 2, "parent + child must both be removed");
    assert!(
        !owner.has_dirty_nodes(),
        "every dirty entry of the removed subtree must be evicted — a \
         freed slot's queue entry would otherwise reach the next phase",
    );
    assert!(
        owner.root_id().is_none(),
        "removing the root clears root_id",
    );
    assert!(
        owner.render_tree().get(parent).is_none(),
        "stale parent id must not resolve (generation bumped)",
    );
}

#[derive(Clone, Debug)]
struct RetirementProbe {
    label: &'static str,
    fail: bool,
    log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
}

impl Drop for RetirementProbe {
    fn drop(&mut self) {
        self.log.lock().expect("probe log").push(self.label);
        if self.fail {
            std::panic::panic_any(self.label);
        }
    }
}

impl flui_foundation::inspect::Inspect for RetirementProbe {
    fn kind(&self) -> &'static str {
        "RetirementProbe"
    }

    fn inspect(&self, _: &mut flui_foundation::inspect::Fields<'_>) {}
}
impl flui_rendering::traits::RenderBox for RetirementProbe {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        _cx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_rendering::RenderResult<flui_foundation::geometry::Size> {
        Ok(flui_foundation::geometry::Size::new(10.0, 10.0))
    }
}
impl flui_rendering::parent_data::ParentData for RetirementProbe {}

fn retirement_child(mode: &str) {
    if let Some(mode) = mode.strip_prefix("visual-") {
        visual_notifier_retirement_child(mode);
        return;
    }
    use flui_rendering::{pipeline::PipelineCell, storage::RenderTree};
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut tree = RenderTree::new();
    let vacant = (mode == "sparse")
        .then(|| tree.insert_box(Box::new(RenderColoredBox::red(1.0, 1.0)) as BoxedRenderObject));
    for (object, data, index) in [("object-a", "data-a", 0), ("object-b", "data-b", 1)] {
        let object_fails = matches!(mode, "object" | "pipeline") && index == 0
            || mode == "second-object" && index == 1;
        let data_fails = mode == "data" && index == 0 || mode == "second-data" && index == 1;
        let id = tree.insert_box(Box::new(RetirementProbe {
            label: object,
            fail: object_fails,
            log: log.clone(),
        }));
        tree.get_mut(id)
            .expect("inserted node")
            .as_box_mut()
            .expect("box node")
            .state_mut()
            .set_parent_data(Box::new(RetirementProbe {
                label: data,
                fail: data_fails,
                log: log.clone(),
            }));
    }
    if let Some(vacant) = vacant {
        drop(tree.remove_shallow(vacant).expect("remove initial slot"));
    }
    // A destructor that panics leaves the rest of the tree to drop during the
    // unwind, as std drops a `Vec` (ADR-0281): every value is destroyed.
    let all = vec!["object-a", "data-a", "object-b", "data-b"];
    let expected = match mode {
        "object" | "pipeline" => (Some("object-a"), all),
        "data" => (Some("data-a"), all),
        "second-object" => (Some("object-b"), all),
        "second-data" => (Some("data-b"), all),
        "healthy" | "shared" | "sparse" => (None, all),
        _ => panic!("unknown retirement mode {mode}"),
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if matches!(mode, "pipeline" | "shared") {
            let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
            *owner.render_tree_mut() = tree;
            let cell = PipelineCell::new(owner);
            if mode == "shared" {
                let alias = cell.clone();
                drop(cell);
                assert!(
                    log.lock().expect("probe log").is_empty(),
                    "alias keeps physical owner"
                );
                assert_eq!(alias.with(|owner| owner.render_tree().len()), 2);
                drop(alias);
            } else {
                drop(cell);
            }
        } else {
            drop(tree);
        }
    }));
    match (outcome, expected.0) {
        (Ok(()), None) => {}
        (Err(payload), Some(text)) => {
            assert_eq!(
                payload.downcast_ref::<&'static str>(),
                Some(&text),
                "first failure remains authoritative"
            );
        }
        _ => panic!("unexpected retirement outcome for {mode}"),
    }
    assert_eq!(
        *log.lock().expect("probe log"),
        expected.1,
        "healthy order and untouched tail custody"
    );
    // A subsequent real owner still accepts nodes and computes layout.
    let mut next = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root = next.insert(Box::new(RenderColoredBox::red(10.0, 10.0)) as BoxedRenderObject);
    next.set_root_id(Some(root));
    next.set_root_constraints(Some(flui_rendering::constraints::BoxConstraints::tight(
        flui_foundation::geometry::Size::new(10.0, 10.0),
    )));
    let mut next = next.into_layout();
    next.run_layout().expect("next owner layouts");
    assert_eq!(
        next.render_tree()
            .get(root)
            .expect("live root")
            .as_box()
            .expect("box root")
            .state()
            .geometry(),
        Some(flui_foundation::geometry::Size::new(10.0, 10.0))
    );
}

fn visual_notifier_retirement_child(mode: &str) {
    use flui_rendering::pipeline::VisualUpdateNotifier;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    if mode == "reentry" {
        struct ReplaceOnDrop {
            slot: Arc<Mutex<Option<VisualUpdateNotifier>>>,
            calls: Arc<AtomicUsize>,
        }
        impl Drop for ReplaceOnDrop {
            fn drop(&mut self) {
                let mut replacement = VisualUpdateNotifier::new();
                let calls = Arc::clone(&self.calls);
                replacement.set_need_visual_update(move || {
                    calls.fetch_add(1, Ordering::SeqCst);
                });
                let mut slot = self.slot.lock().expect("external notifier slot");
                assert!(slot.is_none(), "outer owner released before retirement");
                *slot = Some(replacement);
            }
        }
        let slot = Arc::new(Mutex::new(None));
        let calls = Arc::new(AtomicUsize::new(0));
        let capture = ReplaceOnDrop {
            slot: Arc::clone(&slot),
            calls: Arc::clone(&calls),
        };
        let mut notifier = VisualUpdateNotifier::new();
        notifier.set_need_visual_update(move || {
            let _capture = &capture;
        });
        *slot.lock().expect("external notifier slot") = Some(notifier);
        let outgoing = slot.lock().expect("external notifier slot").take();
        drop(outgoing);
        let replacement = slot
            .lock()
            .expect("external notifier slot")
            .take()
            .expect("capture installed replacement");
        replacement.fire_need_visual_update();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(replacement);
        return;
    }

    let log = Arc::new(Mutex::new(Vec::new()));
    let mut notifier = VisualUpdateNotifier::new();
    let visual = RetirementProbe {
        label: "visual",
        fail: mode == "visual",
        log: Arc::clone(&log),
    };
    notifier.set_need_visual_update(move || {
        let _capture = &visual;
    });
    let created = RetirementProbe {
        label: "created",
        fail: mode == "created",
        log: Arc::clone(&log),
    };
    notifier.set_semantics_owner_created(move || {
        let _capture = &created;
    });
    let disposed = RetirementProbe {
        label: "disposed",
        fail: mode == "disposed",
        log: Arc::clone(&log),
    };
    notifier.set_semantics_owner_disposed(move || {
        let _capture = &disposed;
    });
    let expected = match mode {
        "healthy" | "shared" => (None, vec!["visual", "created", "disposed"]),
        "visual" => (Some("visual"), vec!["visual", "created", "disposed"]),
        "created" => (Some("created"), vec!["visual", "created", "disposed"]),
        "disposed" => (Some("disposed"), vec!["visual", "created", "disposed"]),
        _ => panic!("unknown visual notifier retirement mode {mode}"),
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if mode == "shared" {
            let shared = Arc::new(notifier);
            let alias = Arc::clone(&shared);
            drop(shared);
            assert!(
                log.lock().expect("probe log").is_empty(),
                "nonlast owner retires nothing"
            );
            drop(alias);
        } else {
            drop(notifier);
        }
    }));
    match (outcome, expected.0) {
        (Ok(()), None) => {}
        (Err(payload), Some(text)) => {
            assert_eq!(payload.downcast_ref::<&'static str>(), Some(&text));
            flui_foundation::panic::discard_payload(payload);
        }
        _ => panic!("unexpected visual notifier outcome for {mode}"),
    }
    assert_eq!(
        *log.lock().expect("probe log"),
        expected.1,
        "retirement order: every capture is destroyed"
    );

    // The actual public event producer works again after the contained failure.
    let calls = Arc::new(AtomicUsize::new(0));
    let mut next = VisualUpdateNotifier::new();
    let next_calls = Arc::clone(&calls);
    next.set_need_visual_update(move || {
        next_calls.fetch_add(1, Ordering::SeqCst);
    });
    next.fire_need_visual_update();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(next);
}

const RETIREMENT_CASE: &str = "FLUI_RENDER_TREE_RETIREMENT_CASE";

/// Child-process entry for one row of
/// [`render_tree_retirement_preserves_independent_envelopes`]; a no-op
/// unless the parent names the row.
#[test]
#[ignore = "runs only as a child of render_tree_retirement_preserves_independent_envelopes"]
fn render_tree_retirement_case() {
    if let Ok(mode) = std::env::var(RETIREMENT_CASE) {
        retirement_child(&mode);
        crate::common::isolated_case_passed();
    }
}

/// Each row runs in a bounded child process, so a panicking probe never
/// shares a process with another row. One failing destructor leaves the rest
/// to drop during the unwind, as std does (ADR-0281).
pub(crate) fn render_tree_retirement_preserves_independent_envelopes() {
    let failures: Vec<String> = [
        "healthy",
        "object",
        "data",
        "second-object",
        "second-data",
        "pipeline",
        "shared",
        "sparse",
        "visual-healthy",
        "visual-visual",
        "visual-created",
        "visual-disposed",
        "visual-shared",
        "visual-reentry",
    ]
    .into_iter()
    .filter_map(|mode| {
        crate::common::run_isolated(
            "dispose_eviction::render_tree_retirement_case",
            RETIREMENT_CASE,
            mode,
        )
        .err()
    })
    .collect();
    assert!(
        failures.is_empty(),
        "retirement children failed:\n{}",
        failures.join("\n")
    );
}

/// A leaf whose destructor panics with `label`, for the removal rows below.
fn panics_when_dropped(label: &'static str) -> BoxedRenderObject {
    Box::new(RetirementProbe {
        label,
        fail: true,
        log: std::sync::Arc::default(),
    })
}

/// A slot whose generation is exhausted leaves circulation: removing its last
/// id commits like any other removal, the id never resolves again, and no
/// later insert, `clear` included, hands the slot out under an id it could
/// alias.
pub(crate) fn an_exhausted_render_slot_retires_and_its_last_id_never_resolves_again() {
    use flui_rendering::storage::RenderTree;

    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let first = owner.set_root_render_object(Box::new(RenderColoredBox::red(10.0, 10.0)));
    assert_eq!(owner.remove_render_object(first), 1);
    assert!(
        owner.render_tree_mut().age_next_free_slot_for_test(),
        "the removal freed a slot"
    );
    let last = owner.set_root_render_object(Box::new(RenderColoredBox::blue(10.0, 10.0)));
    assert_eq!(last.index(), first.index(), "the freed slot is taken again");

    let removal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.remove_render_object(last)
    }));
    assert!(
        matches!(removal, Ok(1)),
        "removing a slot's last generation commits like any removal"
    );
    assert_eq!(owner.root_id(), None, "the removed root is cleared");
    assert!(owner.render_tree().get(last).is_none());

    let next = owner.insert(Box::new(RenderColoredBox::red(10.0, 10.0)) as BoxedRenderObject);
    assert_ne!(
        next.index(),
        last.index(),
        "a retired slot is never handed out again"
    );
    assert!(
        owner.render_tree().get(last).is_none(),
        "the last id stays stale after the next insert"
    );
    assert_eq!(owner.remove_render_object(last), 0);

    let mut tree = RenderTree::new();
    let first = tree.insert_box(Box::new(RenderColoredBox::red(1.0, 1.0)));
    drop(
        tree.remove_shallow(first)
            .expect("the first node is removed"),
    );
    assert!(
        tree.age_next_free_slot_for_test(),
        "the removal freed a slot"
    );
    let last = tree.insert_box(Box::new(RenderColoredBox::red(1.0, 1.0)));
    let removal = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tree.remove_shallow(last).is_some()
    }));
    assert!(matches!(removal, Ok(true)), "the last id is removed");
    let cleared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tree.clear()));
    assert!(
        cleared.is_ok(),
        "clear keeps a retired slot without panicking"
    );
    let again = tree.insert_box(Box::new(RenderColoredBox::red(1.0, 1.0)));
    assert_ne!(again.index(), last.index(), "a retired slot survives clear");
}

/// `RenderTree::clear` commits before any destructor runs: when one panics,
/// the tree is already empty and no id minted before the clear resolves to a
/// node inserted after it.
pub(crate) fn a_clear_whose_destructor_panics_leaves_every_old_id_stale() {
    use flui_objects::RenderPadding;
    use flui_rendering::storage::RenderTree;

    let mut tree = RenderTree::new();
    let root = tree.insert_box(Box::new(RenderPadding::all(1.0)));
    tree.set_root(Some(root));
    let child = tree
        .insert_box_child(root, panics_when_dropped("child destructor"))
        .expect("the child attaches");

    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tree.clear()))
        .expect_err("the destructor's panic reaches the caller");
    assert_eq!(
        flui_foundation::panic::payload_text(payload.as_ref()),
        Some("child destructor")
    );
    flui_foundation::panic::discard_payload(payload);

    assert_eq!(tree.root(), None, "the root is reset before the drop");
    assert!(tree.is_empty(), "every node left the tree before the drop");
    let fresh = tree.insert_box(Box::new(RenderColoredBox::red(1.0, 1.0)));
    assert_ne!(fresh, root, "a fresh node never takes an old id");
    assert_ne!(fresh, child, "a fresh node never takes an old id");
    assert!(tree.get(root).is_none(), "the old root id stays stale");
    assert!(tree.get(child).is_none(), "the old child id stays stale");
}

/// `remove_render_object` commits the tree and the owner before a removed
/// render object's destructor runs: when one panics, the whole subtree is
/// gone, the removed root is cleared and the surviving parent is invalidated.
pub(crate) fn a_destructor_that_panics_during_removal_still_commits_the_owner() {
    use flui_foundation::geometry::Size;
    use flui_objects::RenderPadding;
    use flui_rendering::constraints::BoxConstraints;

    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root = owner.set_root_render_object(Box::new(RenderPadding::all(1.0)));
    let child = owner
        .insert_child_render_object(root, panics_when_dropped("child destructor"))
        .expect("the child attaches");
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.remove_render_object(root)
    }))
    .expect_err("the destructor's panic reaches the caller");
    assert_eq!(
        flui_foundation::panic::payload_text(payload.as_ref()),
        Some("child destructor")
    );
    flui_foundation::panic::discard_payload(payload);
    assert_eq!(owner.root_id(), None, "the removed root is cleared");
    assert!(
        owner.render_tree().get(root).is_none(),
        "the root left the tree with its child"
    );
    assert!(owner.render_tree().get(child).is_none());
    assert!(owner.render_tree().is_empty());

    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let parent = owner.set_root_render_object(Box::new(RenderPadding::all(1.0)));
    let child = owner
        .insert_child_render_object(parent, panics_when_dropped("child destructor"))
        .expect("the child attaches");
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(40.0, 40.0))));
    let mut scene = SceneHost::new();
    let (mut owner, frame) = scene.run_frame(owner);
    frame.expect("the first frame runs");
    assert!(
        !owner.has_dirty_nodes(),
        "the first frame left nothing dirty"
    );

    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.remove_render_object(child)
    }))
    .expect_err("the destructor's panic reaches the caller");
    flui_foundation::panic::discard_payload(payload);
    let parent_node = owner.render_tree().get(parent).expect("the parent stays");
    assert!(
        parent_node.children().is_empty(),
        "the child left its parent"
    );
    assert!(
        parent_node.needs_layout(),
        "the parent lays out again without its child"
    );
    assert!(
        owner
            .nodes_needing_layout()
            .iter()
            .any(|entry| entry.id == parent),
        "the parent's relayout is queued"
    );
}
