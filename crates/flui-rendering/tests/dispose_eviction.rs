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

impl flui_foundation::Diagnosticable for RetirementProbe {}
impl flui_rendering::traits::RenderBox for RetirementProbe {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        _cx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_foundation::geometry::Size {
        flui_foundation::geometry::Size::new(10.0, 10.0)
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
    let mut detached_id = None;
    let vacant = (mode == "sparse")
        .then(|| tree.insert_box(Box::new(RenderColoredBox::red(1.0, 1.0)) as BoxedRenderObject));
    for (object, data, index) in [("object-a", "data-a", 0), ("object-b", "data-b", 1)] {
        if mode == "detached-incoming" && index == 1 {
            break;
        }
        let object_fails = matches!(
            mode,
            "object" | "both" | "pipeline" | "incoming" | "detached-incoming"
        ) && index == 0
            || mode == "second-object" && index == 1
            || matches!(mode, "both" | "data-next") && index == 1;
        let data_fails = matches!(
            mode,
            "data" | "both" | "data-next" | "incoming" | "detached-incoming"
        ) && index == 0
            || mode == "second-data" && index == 1;
        let id = tree.insert_box(Box::new(RetirementProbe {
            label: object,
            fail: object_fails,
            log: log.clone(),
        }));
        detached_id = Some(id);
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
    let expected = match mode {
        "object" | "both" | "pipeline" => (Some("object-a"), vec!["object-a"]),
        "data" | "data-next" => (Some("data-a"), vec!["object-a", "data-a"]),
        "second-object" => (Some("object-b"), vec!["object-a", "data-a", "object-b"]),
        "second-data" => (
            Some("data-b"),
            vec!["object-a", "data-a", "object-b", "data-b"],
        ),
        "incoming" | "detached-incoming" => (Some("outer failure"), vec![]),
        "healthy" | "shared" | "sparse" => (None, vec!["object-a", "data-a", "object-b", "data-b"]),
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
        } else if mode == "detached-incoming" {
            struct UnwindNode {
                _node: flui_rendering::storage::RenderNode,
            }
            let node = tree
                .remove_shallow(detached_id.expect("inserted detached node"))
                .expect("extract detached node");
            assert!(tree.is_empty());
            drop(tree);
            let _guard = UnwindNode { _node: node };
            std::panic::panic_any("outer failure");
        } else if mode == "incoming" {
            struct UnwindTree {
                _tree: RenderTree,
            }
            let _guard = UnwindTree { _tree: tree };
            std::panic::panic_any("outer failure");
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
        fail: matches!(
            mode,
            "visual" | "visual-created" | "visual-disposed" | "incoming"
        ),
        log: Arc::clone(&log),
    };
    notifier.set_need_visual_update(move || {
        let _capture = &visual;
    });
    let created = RetirementProbe {
        label: "created",
        fail: matches!(
            mode,
            "created" | "visual-created" | "created-disposed" | "incoming"
        ),
        log: Arc::clone(&log),
    };
    notifier.set_semantics_owner_created(move || {
        let _capture = &created;
    });
    let disposed = RetirementProbe {
        label: "disposed",
        fail: matches!(
            mode,
            "disposed" | "visual-disposed" | "created-disposed" | "incoming"
        ),
        log: Arc::clone(&log),
    };
    notifier.set_semantics_owner_disposed(move || {
        let _capture = &disposed;
    });
    let expected = match mode {
        "healthy" | "shared" => (None, vec!["visual", "created", "disposed"]),
        "visual" | "visual-created" | "visual-disposed" => (Some("visual"), vec!["visual"]),
        "created" | "created-disposed" => (Some("created"), vec!["visual", "created"]),
        "disposed" => (Some("disposed"), vec!["visual", "created", "disposed"]),
        "incoming" => (Some("incoming notifier failure"), vec![]),
        _ => panic!("unknown visual notifier retirement mode {mode}"),
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if mode == "incoming" {
            let _notifier = notifier;
            std::panic::panic_any("incoming notifier failure");
        } else if mode == "shared" {
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
            flui_foundation::panic::retain_opaque_payload(payload);
        }
        _ => panic!("unexpected visual notifier outcome for {mode}"),
    }
    assert_eq!(
        *log.lock().expect("probe log"),
        expected.1,
        "retirement order and retained tail"
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

/// Public producers run in bounded children: restored collection drop glue
/// would abort before an outer catch could report the first failure.
pub(crate) fn render_tree_retirement_preserves_independent_envelopes() {
    const CHILD: &str = "FLUI_RENDER_TREE_RETIREMENT_CHILD";
    if let Ok(mode) = std::env::var(CHILD) {
        retirement_child(&mode);
        return;
    }
    let mut failures = Vec::new();
    for mode in [
        "healthy",
        "object",
        "data",
        "both",
        "data-next",
        "second-object",
        "second-data",
        "incoming",
        "detached-incoming",
        "pipeline",
        "shared",
        "sparse",
        "visual-healthy",
        "visual-visual",
        "visual-created",
        "visual-disposed",
        "visual-visual-created",
        "visual-visual-disposed",
        "visual-created-disposed",
        "visual-incoming",
        "visual-shared",
        "visual-reentry",
    ] {
        use std::io::Read;
        let mut child =
            std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args([
                    "--exact",
                    "contract_matrices::lifecycle_matrix",
                    "--nocapture",
                ])
                .env(CHILD, mode)
                .env("RUST_BACKTRACE", "0")
                .env("RUST_LIB_BACKTRACE", "0")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("retirement child");
        let mut stdout = child.stdout.take().expect("stdout pipe");
        let mut stderr = child.stderr.take().expect("stderr pipe");
        let stdout = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).expect("stdout read");
            bytes
        });
        let stderr = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).expect("stderr read");
            bytes
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let (status, timed_out) = loop {
            if let Some(status) = child.try_wait().expect("child status") {
                break (status, false);
            }
            if std::time::Instant::now() >= deadline {
                child.kill().expect("kill timed out child");
                break (child.wait().expect("reap timed out child"), true);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let stdout = stdout.join().expect("stdout reader");
        let stderr = stderr.join().expect("stderr reader");
        let output = String::from_utf8_lossy(&stdout);
        if timed_out || !status.success() || !output.contains("1 passed; 0 failed") {
            failures.push(format!(
                "{mode}: {status}, timeout={timed_out}\n{}\n{}",
                output,
                String::from_utf8_lossy(&stderr)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "retirement children failed:\n{}",
        failures.join("\n")
    );
}
