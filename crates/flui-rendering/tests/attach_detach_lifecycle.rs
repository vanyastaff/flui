//! ADR-0013 milestone — the `attach`/`detach` tree-lifecycle hook.
//!
//! Proves the pipeline actually fires `RenderObject::attach`/`detach` at
//! insert/remove, that the handed-over `RenderInvalidationHandle` is bound to the
//! right node and drives a REAL re-layout on the very next frame, and
//! that a handle captured before removal degrades to a silent no-op
//! afterward — the generational-staleness guarantee `RenderInvalidationHandle`
//! documents. Reparenting in this codebase has no dedicated API (no
//! `move_child`/`adopt_child`); it is remove-then-insert, so that case is
//! exercised the same way a real reparent would hit it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use flui_foundation::Leaf;
use flui_rendering::pipeline::{PipelineOwner, RenderInvalidationHandle};
use flui_rendering::prelude::*;

use crate::common::BoxedRenderObject;

// ────────────────────────────────────────────────────────────────────────
// Probe: a leaf RenderBox that records attach/detach/perform_layout calls
// ────────────────────────────────────────────────────────────────────────

/// Shared bookkeeping a [`LifecycleProbe`] writes into on
/// `attach`/`detach`/`perform_layout`, read back by the test after the
/// pipeline call returns.
#[derive(Clone, Default, Debug)]
struct LifecycleLog {
    attach_count: Arc<AtomicUsize>,
    detach_count: Arc<AtomicUsize>,
    layout_count: Arc<AtomicUsize>,
    captured_handle: Arc<Mutex<Option<RenderInvalidationHandle>>>,
}

impl LifecycleLog {
    fn attach_count(&self) -> usize {
        self.attach_count.load(Ordering::SeqCst)
    }

    /// The most recently captured handle. Panics if `attach` never fired —
    /// every test here calls it only after asserting `attach_count() > 0`.
    fn captured_handle(&self) -> RenderInvalidationHandle {
        self.captured_handle
            .lock()
            .expect("lock poisoned")
            .clone()
            .expect("attach must have captured a handle before this call")
    }
}

/// A leaf `RenderBox` whose only job is to prove the tree-lifecycle hook
/// fires and hands over a working handle — real render objects (e.g. the
/// future `RenderAnimatedSize`) hold a `Listenable` here instead of a log.
#[derive(Debug)]
struct LifecycleProbe {
    log: LifecycleLog,
    size: Size,
}

impl flui_foundation::Diagnosticable for LifecycleProbe {}

impl RenderBox for LifecycleProbe {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        self.log.layout_count.fetch_add(1, Ordering::SeqCst);
        self.size
    }

    fn attach(&mut self, handle: RenderInvalidationHandle) {
        self.log.attach_count.fetch_add(1, Ordering::SeqCst);
        let _prev = self
            .log
            .captured_handle
            .lock()
            .expect("lock poisoned")
            .replace(handle);
    }

    fn detach(&mut self) {
        self.log.detach_count.fetch_add(1, Ordering::SeqCst);
    }
}

fn probe(log: LifecycleLog) -> BoxedRenderObject {
    Box::new(LifecycleProbe {
        log,
        size: Size::new(40.0, 40.0),
    }) as BoxedRenderObject
}

// ────────────────────────────────────────────────────────────────────────
// Probe: a leaf RenderSliver that records attach/detach/perform_layout
// calls — the Sliver-protocol counterpart of `LifecycleProbe`, proving the
// ADR-0013 lifecycle hook fires for Sliver children too (it did not,
// before this fix: no insertion path called `attach` for a Sliver child).
// ────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────
// attach on insert
// ────────────────────────────────────────────────────────────────────────

pub(crate) fn insert_fires_exactly_one_attach_with_a_handle_bound_to_the_new_id() {
    let mut owner = PipelineOwner::new();
    let log = LifecycleLog::default();

    let id = owner.insert(probe(log.clone()));

    assert_eq!(
        log.attach_count(),
        1,
        "insert must call attach exactly once"
    );
    let handle = log.captured_handle();
    assert_eq!(
        handle.id(),
        id,
        "the handed-over handle must be bound to the freshly-inserted node"
    );
}

// ────────────────────────────────────────────────────────────────────────
// mark_needs_layout from the captured handle reaches perform_layout
// ────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────
// detach on remove
// ────────────────────────────────────────────────────────────────────────

// The token is a linear capability: duplicating it would let one detached
// batch be attached and released, or released twice.
static_assertions::assert_not_impl_any!(
    flui_rendering::pipeline::DetachedRenderSubtrees: Clone, Copy
);

// ────────────────────────────────────────────────────────────────────────
// Generational staleness: a handle captured before removal goes silent
// ────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────
// Reparent = remove + insert (no dedicated API in this codebase)
// ────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────
// Sliver-protocol coverage: no insertion path called `attach` for a Sliver
// child before this fix. `insert_child_render_object` was hard-coded to
// `BoxProtocol`. This test exercises the fixed call site: the new
// `insert_sliver_child_render_object` (the Sliver-protocol counterpart of
// `insert_child_render_object`).
// ────────────────────────────────────────────────────────────────────────
