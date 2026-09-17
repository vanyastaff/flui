//! The render-tree adoption skip must be loud, not a silent orphan.
//!
//! `RenderBehavior::on_mount` adopts its fresh render object into the render
//! tree only when the element received a `parent_render_id`. A render element
//! mounted under an element-tree parent that carries an active `PipelineOwner`
//! but has no render ancestor in its chain (a component root mounted directly
//! through `mount_root_with_pipeline_owner`) falls into that skip: the render
//! object is inserted into the render tree with no parent link — present in
//! render space, invisible to layout/paint. These tests refuse that state at
//! mount (`debug_assert!` + `tracing::error`) while keeping the legal bare
//! mount — a render element mounted as the element-tree root, with no
//! element-tree parent at all — parentless by contract.

use flui_rendering::pipeline::{PipelineCell, PipelineOwner};

use super::*;

/// A component (render-less) root: answers `child_render_id()` with `None`,
/// so a render child inserted under it receives an owner but no render parent.
#[derive(Clone)]
struct ComponentRoot;

impl StatelessView for ComponentRoot {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        // Never drained by these tests: the misuse is driven through the
        // insert seam directly, not through a build_scope drain.
        UnitRenderHost.boxed()
    }
}

impl View for ComponentRoot {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateless(self)
    }
}

// Debug-only: the refusal is the `debug_assert!` arm, which release compiles
// out — there the mount succeeds and the tracing::error event is the only
// signal, pinned by the profile-portable sibling
// `the_release_arm_traces_the_orphaned_adoption_as_an_error`.
#[test]
#[cfg(debug_assertions)]
fn render_element_under_renderless_owner_parent_is_refused_not_silently_orphaned() {
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let mut tree = ElementTree::new();
    let mut build_owner = BuildOwner::new();

    let root_id = tree.mount_root_with_pipeline_owner(
        &ComponentRoot,
        Some(pipeline.clone()),
        &mut build_owner.element_owner_mut(),
    );

    let mount = tree.try_insert_with_provisional_order(
        &UnitRenderHost,
        root_id,
        0,
        &mut build_owner.element_owner_mut(),
        ProvisionalOrder::NONE,
    );

    match mount {
        Err(caught) => {
            let message = caught
                .payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| caught.payload.downcast_ref::<&'static str>().copied())
                .unwrap_or_default();
            assert!(
                message.contains("BUG:"),
                "the refusal must carry the BUG: invariant marker, got: {message}"
            );
            assert!(
                message.contains("orphan the render object"),
                "the refusal must say what the skip costs, got: {message}"
            );
        }
        Ok(child_id) => {
            // The pre-diagnostic behavior, made observable: the mount
            // succeeded and the render object exists in the render tree with
            // no parent link. Report the orphan state in the failure itself
            // so the silent skip is legible, not just absent.
            let render_id = tree
                .get(child_id)
                .and_then(|node| node.element().render_id())
                .expect("the render element created its render object");
            let parent_link = pipeline.with(|owner| owner.render_tree().parent(render_id));
            panic!(
                "the render element {child_id:?} mounted through a render-less \
                 owner-carrying parent and was silently ORPHANED: its render \
                 object {render_id:?} exists in the render tree with parent link \
                 {parent_link:?} — the silent adoption skip this suite refuses"
            );
        }
    }
}

#[test]
fn bare_mount_render_root_still_mounts_parentless() {
    // The legal shape the refusal must not touch: a render element mounted as
    // the element-tree root has no element-tree parent, so its render object
    // legitimately mounts without a render parent (HeadlessBinding /
    // WidgetsBinding wrap it in RootRenderView, which owns the root_id).
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let mut tree = ElementTree::new();
    let mut build_owner = BuildOwner::new();

    let root_id = tree.mount_root_with_pipeline_owner(
        &UnitRenderHost,
        Some(pipeline.clone()),
        &mut build_owner.element_owner_mut(),
    );

    let render_id = tree
        .get(root_id)
        .and_then(|node| node.element().render_id())
        .expect("a bare-mount render root creates its render object");
    let parent_link = pipeline.with(|owner| owner.render_tree().parent(render_id));
    assert!(
        parent_link.is_none(),
        "a bare-mount render root stays parentless in the render tree"
    );
}

#[test]
fn render_child_under_ownerless_parent_takes_the_warn_branch() {
    // No PipelineOwner anywhere in the chain: the render element's mount
    // takes the existing "without PipelineOwner" warn branch and creates no
    // render object at all — no orphan, and nothing for the new refusal to
    // say.
    let mut tree = ElementTree::new();
    let mut build_owner = BuildOwner::new();

    let root_id = tree.mount_root(&ComponentRoot, &mut build_owner.element_owner_mut());
    let child_id = tree.insert(
        &UnitRenderHost,
        root_id,
        0,
        &mut build_owner.element_owner_mut(),
    );

    let render_id = tree
        .get(child_id)
        .and_then(|node| node.element().render_id());
    assert!(
        render_id.is_none(),
        "an owner-less render element creates no render object"
    );
}
