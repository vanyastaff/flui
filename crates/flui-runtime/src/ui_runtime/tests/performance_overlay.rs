use flui_foundation::RenderId;
use flui_layer::{Chunk, ContentVersion, Entry, Layer, SceneNodeId, SceneVersion};
use flui_painting::FontCollection;

use super::*;

const SIZE: flui_foundation::geometry::Size = flui_foundation::geometry::Size::new(400.0, 300.0);

fn ui_runtime_at(device_pixel_ratio: f64) -> UiRuntime {
    let ui_runtime = UiRuntime::try_new(
        test_window(),
        device_pixel_ratio,
        crate::runtime_services::RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            &FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("a ui_runtime over a headless window");
    ui_runtime
        .enter(|ui_runtime| {
            ui_runtime.attach_root_widget(&flui_widgets::Column::new((
                flui_widgets::Text::new("first"),
                flui_widgets::Text::new("second"),
            )))
        })
        .expect("attach succeeds");
    ui_runtime
}

fn draw(ui_runtime: &UiRuntime) -> Option<flui_layer::Scene> {
    ui_runtime.draw_frame(BoxConstraints::tight(SIZE))
}

/// Marks the pipeline's root as needing paint, so the next frame paints
/// without any other render object changing.
fn repaint_root(ui_runtime: &UiRuntime) {
    ui_runtime
        .presentations
        .primary()
        .pipeline()
        .with_mut(|owner| {
            let root = owner.root_id().expect("an attached tree has a root");
            owner.mark_needs_paint(root);
        });
}

/// The nodes the scene's root slots, in paint order.
fn slotted(version: &SceneVersion) -> Vec<SceneNodeId> {
    version
        .node(version.root())
        .expect("a version holds its root")
        .content()
        .slots()
        .collect()
}

/// The overlay's node: slotted after the frame node.
fn overlay_node(version: &SceneVersion) -> Option<SceneNodeId> {
    slotted(version).get(1).copied()
}

/// The chunk that holds the overlay's recorded readout.
pub(super) fn overlay_readout(version: &SceneVersion) -> Option<&Chunk> {
    let node = version.node(overlay_node(version)?)?;
    node.content()
        .entries()
        .iter()
        .find_map(|entry| match entry {
            Entry::Chunk { chunk, .. } => Some(&**chunk),
            Entry::Effect { .. } | Entry::Slot { .. } => None,
        })
}

/// Marks `render` as needing paint, so the next frame paints it and nothing
/// else changes.
fn mark_needs_paint(ui_runtime: &UiRuntime, render: RenderId) {
    ui_runtime
        .presentations
        .primary()
        .pipeline()
        .with_mut(|owner| owner.mark_needs_paint(render));
}

/// Every chunk the version paints outside the overlay's node, in paint
/// order, with the node it sits in and its owner: what the raster side keeps
/// a recording for by its version.
fn chunks_outside_the_overlay(
    version: &SceneVersion,
) -> Vec<(SceneNodeId, RenderId, ContentVersion)> {
    let overlay = overlay_node(version);
    let mut nodes = vec![version.root()];
    let mut chunks = Vec::new();
    while let Some(id) = nodes.pop() {
        if Some(id) == overlay {
            continue;
        }
        let node = version
            .node(id)
            .expect("a slot names a node of its version");
        for entry in node.content().entries() {
            match entry {
                Entry::Chunk { chunk, .. } => chunks.push((id, chunk.owner(), chunk.version())),
                Entry::Slot { node, .. } => nodes.push(*node),
                Entry::Effect { .. } => {}
            }
        }
    }
    chunks
}

/// The overlay's node is a clip to its bounds over one chunk of the readout,
/// under the device-pixel-ratio scale when there is one, and the readout
/// inks inside the bounds. Fails if the readout is published as a layer that
/// draws, without its clip, or past its bounds.
fn the_overlay_is_one_chunk_under_a_clip_to_its_bounds() {
    for device_pixel_ratio in [1.0, 2.0] {
        let ui_runtime = ui_runtime_at(device_pixel_ratio);
        ui_runtime.set_performance_overlay(true);
        let scene = draw(&ui_runtime).expect("the first frame paints");
        let version = scene.version();
        let node = version
            .node(overlay_node(version).expect("the overlay is slotted after the frame"))
            .expect("the overlay's node is in its version");
        let content = node.content();
        let layers: Vec<&Layer> = content.effect_layers().map(|(_, layer)| layer).collect();
        let clip = match layers.as_slice() {
            [Layer::ClipRect(clip)] if device_pixel_ratio == 1.0 => clip,
            [Layer::Transform(scale), Layer::ClipRect(clip)] if device_pixel_ratio == 2.0 => {
                assert_eq!(
                    *scale.transform(),
                    flui_foundation::geometry::Matrix4::scaling(2.0, 2.0, 1.0),
                    "the readout is scaled to device pixels"
                );
                clip
            }
            other => panic!("dpr {device_pixel_ratio}: the overlay's effects are {other:?}"),
        };
        assert_eq!(clip.clip_rect(), crate::performance_overlay::BOUNDS);
        assert!(clip.clips(), "the clip cuts the readout");

        let (clip_entry, _) = content
            .effect_layers()
            .last()
            .expect("the overlay has its clip");
        let chunks: Vec<_> = content
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Chunk { chunk, parent } => Some((chunk, *parent)),
                Entry::Effect { .. } | Entry::Slot { .. } => None,
            })
            .collect();
        let [(chunk, parent)] = chunks.as_slice() else {
            panic!("dpr {device_pixel_ratio}: the overlay holds one chunk: {chunks:?}");
        };
        assert_eq!(*parent, Some(clip_entry), "the chunk sits under the clip");
        let inked = chunk
            .commands()
            .bounds()
            .expect("the readout records a background");
        assert!(
            crate::performance_overlay::BOUNDS.contains_rect(&inked),
            "dpr {device_pixel_ratio}: the readout inks {inked:?}, inside the bounds"
        );
    }
}

/// A frame that repaints one text with the overlay on leaves the version of
/// every chunk but the repainted text's, and the root's content, as they
/// were: the recordings the raster side keeps for them by version stay valid,
/// whatever the overlay records. Fails if publishing the overlay re-records
/// or republishes the frame's other chunks or the root.
fn an_overlay_frame_changes_no_other_chunk() {
    let ui_runtime = ui_runtime_at(1.0);
    ui_runtime.set_performance_overlay(true);
    let first = draw(&ui_runtime).expect("the first frame paints");
    let before = chunks_outside_the_overlay(first.version());
    let [kept, (_, repainted, _)] = before.as_slice() else {
        panic!("the frame paints one chunk per text: {before:?}");
    };
    mark_needs_paint(&ui_runtime, *repainted);
    let second = draw(&ui_runtime).expect("a repainted text paints");
    let after = chunks_outside_the_overlay(second.version());

    assert_eq!(
        overlay_node(first.version()),
        overlay_node(second.version()),
        "one overlay node"
    );
    assert!(
        overlay_readout(second.version()).is_some(),
        "the overlay stays shown"
    );
    let [now_kept, (_, owner, version)] = after.as_slice() else {
        panic!("the frame paints one chunk per text: {after:?}");
    };
    assert_eq!(now_kept, kept, "the other text keeps its chunk's version");
    assert_eq!(owner, repainted);
    assert_ne!(
        Some(version),
        before.last().map(|(_, _, version)| version),
        "the repainted text records anew"
    );
    let root = |version: &SceneVersion| {
        std::sync::Arc::clone(
            version
                .node(version.root())
                .expect("a version holds its root")
                .content(),
        )
    };
    assert!(
        std::sync::Arc::ptr_eq(&root(first.version()), &root(second.version())),
        "the root's content is not published again"
    );
}

/// With the overlay off the root slots the frame node alone, and a frame
/// with nothing to paint publishes nothing, the overlay on or off; turning
/// it off retires its node. Fails if the overlay publishes while off, keeps
/// its node, or asks an idle frame to publish.
fn the_overlay_off_publishes_nothing() {
    let ui_runtime = ui_runtime_at(1.0);
    let scene = draw(&ui_runtime).expect("the first frame paints");
    assert_eq!(slotted(scene.version()).len(), 1, "the frame node alone");
    assert!(
        draw(&ui_runtime).is_none(),
        "an idle frame publishes nothing"
    );

    ui_runtime.set_performance_overlay(true);
    repaint_root(&ui_runtime);
    let shown = draw(&ui_runtime).expect("a repainted root paints");
    let overlay = overlay_node(shown.version()).expect("the overlay is shown");
    assert!(
        draw(&ui_runtime).is_none(),
        "an idle frame publishes nothing with the overlay on"
    );

    ui_runtime.set_performance_overlay(false);
    repaint_root(&ui_runtime);
    let hidden = draw(&ui_runtime).expect("a repainted root paints");
    assert_eq!(slotted(hidden.version()).len(), 1, "the frame node alone");
    assert!(
        hidden.version().node(overlay).is_none(),
        "the overlay's node is retired"
    );
    assert!(
        draw(&ui_runtime).is_none(),
        "an idle frame publishes nothing"
    );
}

#[test]
fn performance_overlay_matrix() {
    crate::table_test::run_table(
        "performance_overlay_matrix",
        &[
            (
                "the_overlay_is_one_chunk_under_a_clip_to_its_bounds",
                the_overlay_is_one_chunk_under_a_clip_to_its_bounds,
            ),
            (
                "an_overlay_frame_changes_no_other_chunk",
                an_overlay_frame_changes_no_other_chunk,
            ),
            (
                "the_overlay_off_publishes_nothing",
                the_overlay_off_publishes_nothing,
            ),
        ],
    );
}
