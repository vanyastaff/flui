//! A render element mounted under an element-tree parent that carries an
//! active `PipelineOwner` but has no render ancestor in its chain must be
//! refused at mount, not silently adopted parentless.
//!
//! The shape this refuses: a component (render-less) root mounted directly
//! through `mount_root_with_pipeline_owner` propagates its `PipelineOwner` to
//! children but answers `child_render_id()` with `None` (it has no render
//! ancestor to pass through), so a render child mounted under it reached
//! `RenderBehavior::on_mount` with an owner but no render parent and its
//! render object was inserted into the render tree with no parent link.
//! Production roots are wrapped in `RootRenderView`, which owns a render id —
//! the shape is reachable only through the low-level attach primitive, which
//! is exactly where the refusal belongs.
//!
//! The legal bare-mount root (render element mounted as the element-tree
//! root, no element-tree parent) is pinned by
//! `flui-testing/tests/mount_bootstrap.rs` and the in-crate
//! `orphaned_render_mount_tests` and is untouched.

use flui_objects::RenderSizedBox;
use flui_rendering::RenderUpdateImpact;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_view::{
    BuildContext, BuildOwner, ElementTree, IntoView, RenderView, StatelessView, View, ViewExt,
};

// ============================================================================
// Fixtures
// ============================================================================

/// Minimal render leaf: creates a real render object on mount.
#[derive(Clone)]
struct OrphanProbeLeaf;

impl RenderView for OrphanProbeLeaf {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}

impl View for OrphanProbeLeaf {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

/// A component (render-less) root: propagates its `PipelineOwner` but answers
/// `child_render_id()` with `None`, so a render child under it mounts with an
/// owner and no render parent — the shape the mount refusal exists for.
#[derive(Clone)]
struct OrphanProbeRoot;

impl StatelessView for OrphanProbeRoot {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        OrphanProbeLeaf.boxed()
    }
}

impl View for OrphanProbeRoot {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

fn misuse_tree() -> (
    PipelineCell,
    ElementTree,
    BuildOwner,
    flui_foundation::ElementId,
) {
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let mut tree = ElementTree::new();
    let mut build_owner = BuildOwner::new();
    let root_id = tree.mount_root_with_pipeline_owner(
        &OrphanProbeRoot,
        Some(pipeline.clone()),
        &mut build_owner.element_owner_mut(),
    );
    (pipeline, tree, build_owner, root_id)
}

// ============================================================================
// The misuse, driven through the public seams
// ============================================================================

// Debug-only: the refusal is the `debug_assert!` arm, which release compiles
// out — there the mount succeeds and the tracing::error event is the only
// signal, pinned by the profile-portable sibling below
// (`the_release_arm_traces_the_orphaned_adoption_as_an_error`).
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "orphan the render object")]
fn a_render_child_mounted_under_a_renderless_owner_carrying_parent_is_refused() {
    // Direct insert: the mount's containment window converts the refusal
    // panic into a `ChildHookPanic`, and the infallible public `insert`
    // resumes the unwind from its payload — so the debug refusal surfaces
    // here as this panic.
    let (_pipeline, mut tree, mut build_owner, root_id) = misuse_tree();
    tree.insert(
        &OrphanProbeLeaf,
        root_id,
        0,
        &mut build_owner.element_owner_mut(),
    );
}

#[test]
fn the_release_arm_traces_the_orphaned_adoption_as_an_error() {
    // The `tracing::error` arm runs in BOTH profiles — debug reaches it just
    // before the `debug_assert!` fires; release never compiles the assert in,
    // so this event is the only signal there. The resumed panic is contained
    // here with `catch_unwind` so the same test asserts the arm in release
    // too, where `insert` simply returns Ok.
    let (_pipeline, mut tree, mut build_owner, root_id) = misuse_tree();

    let (mount, log) = flui_testing::log_capture::capture(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.insert(
                &OrphanProbeLeaf,
                root_id,
                0,
                &mut build_owner.element_owner_mut(),
            )
        }))
    });

    assert_eq!(
        log.at_level(tracing::Level::ERROR)
            .filter(|record| record.contains("orphan the render object"))
            .count(),
        1,
        "the release arm must trace exactly one orphan-refusal error; captured:\n{log}"
    );

    #[cfg(debug_assertions)]
    {
        let payload = match &mount {
            Err(payload) => payload,
            Ok(child_id) => {
                panic!("the debug refusal must panic; instead mounted {child_id:?}")
            }
        };
        let payload_text = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&'static str>().copied())
            .unwrap_or_default();
        assert!(
            payload_text.contains("orphan the render object"),
            "the refusal must say what the skip costs, got: {payload_text}"
        );
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = mount.expect("the release arm is diagnostics-only: the mount succeeds");
    }
}

#[test]
fn an_ownerless_render_child_mounts_without_refusal() {
    // No PipelineOwner anywhere: the render element takes the existing
    // "without PipelineOwner" warn branch — no render object, no refusal.
    let mut tree = ElementTree::new();
    let mut build_owner = BuildOwner::new();
    let root_id = tree.mount_root(&OrphanProbeRoot, &mut build_owner.element_owner_mut());

    let (child_id, log) = flui_testing::log_capture::capture(|| {
        tree.insert(
            &OrphanProbeLeaf,
            root_id,
            0,
            &mut build_owner.element_owner_mut(),
        )
    });

    let refused = log
        .at_level(tracing::Level::ERROR)
        .any(|record| record.contains("orphan the render object"));
    assert!(
        !refused,
        "an owner-less mount must stay on the warn branch, not the refusal; captured:\n{log}"
    );
    let render_id = tree
        .get(child_id)
        .and_then(|node| node.element().render_id());
    assert!(
        render_id.is_none(),
        "an owner-less render element creates no render object"
    );
}
