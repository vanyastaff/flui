//! Unit tests for `RenderState<P>` dirty propagation and storage.

use std::{
    collections::HashMap,
    mem::size_of,
    sync::{Arc, Mutex},
};

use flui_foundation::ElementId;
use flui_types::{Offset, geometry::px};

use super::*;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::storage::flags::RenderFlags;

// ========================================================================
// Mythos Step 14 -- static memory-footprint assertions
// ========================================================================
//
// These tests guard the data-oriented design budgets documented in
// `docs/designs/2026-05-20-mythos-flui-rendering-redesign.md` Section 9.
// If a future change blows up the per-node size, these tests fail
// loudly rather than the regression sneaking in unobserved.

#[test]
fn render_state_box_fits_budget() {
    // RenderState<BoxProtocol> = AtomicRenderFlags(4) + OnceCell<Size>
    // + OnceCell<BoxConstraints> + AtomicOffset(8) + PhantomData(0).
    // Documented estimate: 44-60 bytes. Cap at 128 to leave room for
    // future fields without forcing a re-budget on every commit.
    let actual = size_of::<RenderState<BoxProtocol>>();
    assert!(
        actual <= 128,
        "RenderState<BoxProtocol> grew beyond budget: {actual} bytes (cap 128)"
    );
}

#[test]
fn render_state_sliver_fits_budget() {
    let actual = size_of::<RenderState<SliverProtocol>>();
    assert!(
        actual <= 192,
        "RenderState<SliverProtocol> grew beyond budget: {actual} bytes (cap 192)"
    );
}

// Mock tree for testing dirty propagation
struct MockTree {
    states: HashMap<ElementId, BoxRenderState>,
    parents: HashMap<ElementId, ElementId>,
    needs_layout: Arc<Mutex<Vec<ElementId>>>,
    needs_paint: Arc<Mutex<Vec<ElementId>>>,
}

impl MockTree {
    fn new() -> Self {
        Self {
            states: HashMap::new(),
            parents: HashMap::new(),
            needs_layout: Arc::new(Mutex::new(Vec::new())),
            needs_paint: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn add_element(&mut self, id: ElementId, parent: Option<ElementId>) {
        // Create state with clean flags (no dirty flags) for testing propagation
        let state = BoxRenderState::with_flags(RenderFlags::empty());
        self.states.insert(id, state);
        if let Some(parent_id) = parent {
            self.parents.insert(id, parent_id);
        }
    }

    fn set_relayout_boundary(&mut self, id: ElementId, is_boundary: bool) {
        if let Some(state) = self.states.get(&id) {
            state.set_relayout_boundary(is_boundary);
        }
    }

    /// Marks an element as needing layout, properly handling propagation
    fn mark_element_needs_layout(&mut self, id: ElementId) {
        self.mark_element_needs_layout_inner(id);
    }

    fn mark_element_needs_layout_inner(&mut self, id: ElementId) {
        // Get parent first to avoid borrow issues
        let parent_id = self.parents.get(&id).copied();

        // Get state from tree (not clone) to preserve relayout boundary info
        let (already_dirty, is_boundary) = if let Some(state) = self.states.get(&id) {
            let already = state.flags().needs_layout();
            let boundary = state.is_relayout_boundary();
            if !already {
                state.flags().mark_needs_layout();
            }
            (already, boundary)
        } else {
            return;
        };

        if already_dirty {
            return;
        }

        // Check boundary and propagate
        if is_boundary {
            self.needs_layout.lock().unwrap().push(id);
        } else {
            // Propagate to parent
            if let Some(parent_id) = parent_id {
                self.mark_element_needs_layout_inner(parent_id);
            }
        }
    }
}

impl RenderDirtyPropagation for MockTree {
    fn parent(&self, id: ElementId) -> Option<ElementId> {
        self.parents.get(&id).copied()
    }

    fn get_render_state<P: Protocol>(&self, id: ElementId) -> Option<&RenderState<P>> {
        // Type erasure hack for tests - we know it's BoxProtocol
        self.states
            .get(&id)
            .map(|s| unsafe { std::mem::transmute::<&BoxRenderState, &RenderState<P>>(s) })
    }

    fn register_needs_layout(&mut self, id: ElementId) {
        self.needs_layout.lock().unwrap().push(id);
    }

    fn register_needs_paint(&mut self, id: ElementId) {
        self.needs_paint.lock().unwrap().push(id);
    }

    fn register_needs_compositing_bits_update(&mut self, _id: ElementId) {
        // Not used in current tests
    }

    fn is_repaint_boundary(&self, id: ElementId) -> bool {
        self.states
            .get(&id)
            .map(|s| s.is_repaint_boundary())
            .unwrap_or(false)
    }

    fn was_repaint_boundary(&self, _id: ElementId) -> bool {
        // For tests, assume no previous state
        false
    }
}

#[test]
fn test_mark_needs_layout_propagates_to_parent() {
    let mut tree = MockTree::new();
    let child_id = ElementId::new(1);
    let parent_id = ElementId::new(2);

    tree.add_element(child_id, Some(parent_id));
    tree.add_element(parent_id, None);

    // Mark child dirty - clone state to avoid borrow conflict
    let child_state = tree.states.get(&child_id).unwrap().clone();
    child_state.mark_needs_layout(child_id, &mut tree);

    // Check parent is also dirty
    let parent_state = tree.states.get(&parent_id).unwrap();
    assert!(parent_state.needs_layout());
}

#[test]
fn test_mark_needs_layout_stops_at_relayout_boundary() {
    let mut tree = MockTree::new();
    let child_id = ElementId::new(1);
    let boundary_id = ElementId::new(2);
    let grandparent_id = ElementId::new(3);

    tree.add_element(child_id, Some(boundary_id));
    tree.add_element(boundary_id, Some(grandparent_id));
    tree.add_element(grandparent_id, None);

    // Make middle element a relayout boundary
    tree.set_relayout_boundary(boundary_id, true);

    // Verify boundary is set correctly
    assert!(
        tree.states
            .get(&boundary_id)
            .unwrap()
            .is_relayout_boundary(),
        "boundary_id should be a relayout boundary"
    );

    // Mark child dirty using helper to avoid borrow conflict
    tree.mark_element_needs_layout(child_id);

    // Check child is dirty
    assert!(
        tree.states.get(&child_id).unwrap().needs_layout(),
        "child should need layout"
    );

    // Check boundary is dirty
    assert!(
        tree.states.get(&boundary_id).unwrap().needs_layout(),
        "boundary should need layout"
    );

    // Check boundary is still marked as relayout boundary
    assert!(
        tree.states
            .get(&boundary_id)
            .unwrap()
            .is_relayout_boundary(),
        "boundary should still be relayout boundary after marking dirty"
    );

    // Boundary is registered with pipeline owner
    let needs_layout = tree.needs_layout.lock().unwrap();
    assert_eq!(
        needs_layout.len(),
        1,
        "expected 1 registration, got {:?}",
        *needs_layout
    );
    assert_eq!(needs_layout[0], boundary_id);
    drop(needs_layout);

    // Grandparent is NOT dirty (propagation stopped at boundary)
    let grandparent_state = tree.states.get(&grandparent_id).unwrap();
    assert!(!grandparent_state.needs_layout());
}

#[test]
fn test_mark_needs_layout_early_return() {
    let mut tree = MockTree::new();
    let id = ElementId::new(1);
    tree.add_element(id, None);

    // Clone state to avoid borrow conflict
    let state = tree.states.get(&id).unwrap().clone();

    // First call marks dirty
    state.mark_needs_layout(id, &mut tree);
    assert!(state.needs_layout());

    // Clear the registered list
    tree.needs_layout.lock().unwrap().clear();

    // Second call should early return (no registration)
    state.mark_needs_layout(id, &mut tree);
    assert_eq!(tree.needs_layout.lock().unwrap().len(), 0);
}

#[test]
fn test_mark_parent_needs_layout_ignores_boundary() {
    let mut tree = MockTree::new();
    let child_id = ElementId::new(1);
    let parent_id = ElementId::new(2);

    tree.add_element(child_id, Some(parent_id));
    tree.add_element(parent_id, None);

    // Make child a relayout boundary
    tree.set_relayout_boundary(child_id, true);

    // Mark parent needs layout (should propagate despite boundary) - clone to avoid
    // borrow conflict
    let child_state = tree.states.get(&child_id).unwrap().clone();
    child_state.mark_parent_needs_layout(child_id, &mut tree);

    // Parent should be dirty
    let parent_state = tree.states.get(&parent_id).unwrap();
    assert!(parent_state.needs_layout());
}

#[test]
fn test_geometry_write_once() {
    let state = BoxRenderState::new();
    let size1 = flui_types::Size::new(px(100.0), px(50.0));
    let size2 = flui_types::Size::new(px(200.0), px(100.0));

    // First set succeeds
    state.set_geometry(size1);
    assert_eq!(state.geometry(), Some(size1));

    // Second set panics
    let result = std::panic::catch_unwind(|| {
        state.set_geometry(size2);
    });
    assert!(result.is_err());
}

#[test]
fn test_atomic_offset() {
    let state = BoxRenderState::new();
    let offset = Offset::new(px(10.0), px(20.0));

    state.set_offset(offset);
    assert_eq!(state.offset(), offset);

    // Can update multiple times
    let offset2 = Offset::new(px(30.0), px(40.0));
    state.set_offset(offset2);
    assert_eq!(state.offset(), offset2);
}

#[test]
fn test_boundary_flags() {
    let state = BoxRenderState::new();

    assert!(!state.is_relayout_boundary());
    assert!(!state.is_repaint_boundary());

    state.set_relayout_boundary(true);
    assert!(state.is_relayout_boundary());

    state.set_repaint_boundary(true);
    assert!(state.is_repaint_boundary());

    state.set_relayout_boundary(false);
    assert!(!state.is_relayout_boundary());
    assert!(state.is_repaint_boundary());
}
