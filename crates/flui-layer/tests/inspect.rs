//! A layer snapshot reads each layer's own fields.

use std::sync::Arc;

use flui_foundation::geometry::{Offset, Point, RRect, RSuperellipse, Rect};
use flui_foundation::inspect::{
    DiffOptions, Level, Plain, SnapshotQuery, Variant, diff, render, snapshot,
};
use flui_layer::{
    AnnotatedRegionLayer, BackdropFilterLayer, BackdropGroupKey, ClipPathLayer, ClipRRectLayer,
    ClipRectLayer, ClipSuperellipseLayer, ColorFilterLayer, FollowerLayer, ImageFilterLayer, Layer,
    LayerLink, LayerTree, OffsetLayer, OpacityLayer, PictureLayer,
};
use flui_painting::paint::{BlendMode, Clip, ColorFilter, ImageFilter, Path};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

/// Builds a clip layer of one kind around `rect`.
type MakeClip = fn(Rect, Clip) -> Layer;

/// Every clip layer reports how it rasterises its edge, so two clips of one shape that differ
/// only in quality read apart.
///
/// Fails when a clip variant reports its shape alone.
#[test]
fn clip_layers_report_their_clip_behavior() {
    let rect = Rect::from_xywh(0.0, 0.0, 40.0, 20.0);
    let clips: [(&str, MakeClip); 4] = [
        ("ClipRect", |rect, clip| {
            ClipRectLayer::new(rect, clip).into()
        }),
        ("ClipRRect", |rect, clip| {
            ClipRRectLayer::new(RRect::from_rect_circular(rect, 4.0), clip).into()
        }),
        ("ClipPath", |rect, clip| {
            ClipPathLayer::new(Path::rectangle(rect), clip).into()
        }),
        ("ClipSuperellipse", |rect, clip| {
            ClipSuperellipseLayer::new(RSuperellipse::from_rect_circular(rect, 4.0), clip).into()
        }),
    ];
    for (kind, layer) in clips {
        for (clip, name) in [
            (Clip::HardEdge, "HardEdge"),
            (Clip::AntiAliasWithSaveLayer, "AntiAliasWithSaveLayer"),
        ] {
            let tree = LayerTree::new(layer(rect, clip));
            let root = snapshot(&tree, SnapshotQuery::default()).expect("a tree has a root");
            assert_eq!(
                root.get::<Variant<'_>>("clip_behavior"),
                Ok(Variant { ty: "Clip", name }),
                "{kind}"
            );
        }
    }
}

/// A non-rectangular clip reports its exact shape: two clips with one bounding rectangle and one
/// edge quality but different shapes snapshot apart.
///
/// Fails when a rounded-rectangle, path or superellipse clip reports only its bounds.
#[test]
fn clip_layers_of_one_bounds_and_different_shapes_read_apart() {
    let rect = Rect::from_xywh(0.0, 0.0, 40.0, 20.0);
    let pairs: [(&str, Layer, Layer); 3] = [
        (
            "ClipRRect",
            ClipRRectLayer::new(RRect::from_rect_circular(rect, 4.0), Clip::AntiAlias).into(),
            ClipRRectLayer::new(RRect::from_rect_circular(rect, 8.0), Clip::AntiAlias).into(),
        ),
        (
            "ClipPath",
            ClipPathLayer::new(Path::rectangle(rect), Clip::AntiAlias).into(),
            ClipPathLayer::new(diamond(), Clip::AntiAlias).into(),
        ),
        (
            "ClipSuperellipse",
            ClipSuperellipseLayer::new(
                RSuperellipse::from_rect_circular(rect, 4.0),
                Clip::AntiAlias,
            )
            .into(),
            ClipSuperellipseLayer::new(
                RSuperellipse::from_rect_circular(rect, 8.0),
                Clip::AntiAlias,
            )
            .into(),
        ),
    ];
    for (kind, first, second) in pairs {
        let first = snapshot(&LayerTree::new(first), SnapshotQuery::default()).expect("a root");
        let second = snapshot(&LayerTree::new(second), SnapshotQuery::default()).expect("a root");
        assert_eq!(first.rect, second.rect, "{kind}: one bounding rectangle");
        assert_ne!(first, second, "{kind}");
    }
}

/// A diamond touching every edge of the 40×20 rectangle, so its bounds are that rectangle.
fn diamond() -> Path {
    let mut path = Path::new();
    path.move_to(Point::new(20.0, 0.0));
    path.line_to(Point::new(40.0, 10.0));
    path.line_to(Point::new(20.0, 20.0));
    path.line_to(Point::new(0.0, 10.0));
    path.close();
    path
}

/// A layer whose effect or drawing changes snapshots apart from the one before, so `diff` reports it.
///
/// Fails when a variant reports nothing about its effect, or a picture only its command count,
/// and two such layers read alike.
#[test]
fn layers_that_differ_only_in_their_effect_diff_apart() {
    let link = LayerLink::new();
    let bounds = Rect::from_xywh(0.0, 0.0, 40.0, 20.0);
    let pairs: [(&str, Layer, Layer); 7] = [
        (
            "Opacity",
            OpacityLayer::with_blend(0.5, Offset::ZERO, BlendMode::SrcOver).into(),
            OpacityLayer::with_blend(0.5, Offset::ZERO, BlendMode::Src).into(),
        ),
        ("Picture", filled(Color::RED), filled(Color::BLUE)),
        (
            "AnnotatedRegion",
            AnnotatedRegionLayer::new(bounds, Arc::new("old")).into(),
            AnnotatedRegionLayer::new(bounds, Arc::new("new")).into(),
        ),
        (
            "AnnotatedRegion type",
            AnnotatedRegionLayer::new(bounds, Arc::new(Vec::<u8>::new())).into(),
            AnnotatedRegionLayer::new(bounds, Arc::new(Vec::<u16>::new())).into(),
        ),
        (
            "ColorFilter",
            ColorFilterLayer::new(ColorFilter::grayscale()).into(),
            ColorFilterLayer::new(ColorFilter::mode(Color::BLUE, BlendMode::SrcOver)).into(),
        ),
        (
            "ImageFilter",
            ImageFilterLayer::new(ImageFilter::blur(2.0)).into(),
            ImageFilterLayer::new(ImageFilter::blur(8.0)).into(),
        ),
        (
            "Follower",
            FollowerLayer::new(link).into(),
            FollowerLayer::new(link)
                .with_show_when_unlinked(false)
                .into(),
        ),
    ];
    for (kind, before, after) in pairs {
        let before =
            snapshot(&LayerTree::new(before), SnapshotQuery::default()).expect("a tree has a root");
        let after =
            snapshot(&LayerTree::new(after), SnapshotQuery::default()).expect("a tree has a root");
        assert!(
            !diff(&before, &after, DiffOptions::default()).is_empty(),
            "{kind}: {before} and {after} read alike"
        );
    }
}

/// A chain far deeper than the thread's stack would hold a frame per level snapshots, renders,
/// diffs, clones, compares, debug-prints and drops.
///
/// Fails, by overflowing the stack, when any of those walks recurses per level.
#[test]
fn a_deep_layer_chain_snapshots_without_recursion() {
    const DEPTH: usize = 30_000;
    let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
    let mut tip = tree.root();
    for _ in 1..DEPTH {
        tip = tree.push_child(tip, Layer::from(OffsetLayer::zero()));
    }
    let root = snapshot(&tree, SnapshotQuery::default()).expect("a tree has a root");
    assert_eq!(root.iter().count(), DEPTH);
    let mut lines = Lines(0);
    render(&root, Level::Summary, &Plain, &mut lines).expect("counting never fails");
    assert_eq!(lines.0, DEPTH);
    assert_eq!(format!("{root:?}").matches("SnapshotNode {").count(), DEPTH);
    let copy = root.clone();
    assert_eq!(copy, root);
    assert_eq!(diff(&root, &copy, DiffOptions::default()), Vec::new());
}

/// Counts the lines written to it and keeps nothing.
struct Lines(usize);

impl core::fmt::Write for Lines {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        self.0 += text.matches('\n').count();
        Ok(())
    }
}

/// A picture of one rectangle filled with `color`.
fn filled(color: Color) -> Layer {
    let mut canvas = Canvas::new();
    canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 40.0, 20.0), &Paint::fill(color));
    PictureLayer::new(canvas.finish()).into()
}

/// Two backdrop filters that share a group snapshot apart from two in groups of their own,
/// since a group samples one backdrop for all of its filters.
///
/// Fails when the group reports a label that every key shares.
#[test]
fn backdrop_filters_that_stop_sharing_a_group_diff_apart() {
    let pane = |key| {
        let bounds = Rect::from_xywh(0.0, 0.0, 40.0, 20.0);
        Layer::from(
            BackdropFilterLayer::new(ImageFilter::blur(4.0), BlendMode::SrcOver, bounds)
                .with_group(Some(key)),
        )
    };
    let panes = |first: BackdropGroupKey, second: BackdropGroupKey| {
        let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
        let root = tree.root();
        tree.push_child(root, pane(first));
        tree.push_child(root, pane(second));
        snapshot(&tree, SnapshotQuery::default()).expect("a tree has a root")
    };
    let shared = BackdropGroupKey::new();
    let grouped = panes(shared.clone(), shared.clone());
    let apart = panes(shared, BackdropGroupKey::new());
    assert_ne!(diff(&grouped, &apart, DiffOptions::default()), Vec::new());
}

/// A snapshot names backdrop groups and annotation payloads by the order it meets them, so
/// two scenes that share the same way snapshot alike whichever objects they were built from,
/// and the text shows which layers share.
///
/// Fails when a group or payload reads by its address, which differs between the two scenes
/// and can repeat once an object is freed.
#[test]
fn shared_groups_and_payloads_read_by_their_order_in_the_scene() {
    let bounds = Rect::from_xywh(0.0, 0.0, 40.0, 20.0);
    let scene = |groups: [&BackdropGroupKey; 3], payload: &Arc<&'static str>| {
        let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
        let root = tree.root();
        for group in groups {
            tree.push_child(
                root,
                Layer::from(
                    BackdropFilterLayer::new(ImageFilter::blur(4.0), BlendMode::SrcOver, bounds)
                        .with_group(Some(group.clone())),
                ),
            );
        }
        for _ in 0..2 {
            tree.push_child(
                root,
                Layer::from(AnnotatedRegionLayer::new(bounds, Arc::clone(payload))),
            );
        }
        snapshot(&tree, SnapshotQuery::default()).expect("a tree has a root")
    };
    let (first, second) = (BackdropGroupKey::new(), BackdropGroupKey::new());
    let before = scene([&first, &first, &second], &Arc::new("toolbar"));
    let (third, fourth) = (BackdropGroupKey::new(), BackdropGroupKey::new());
    let after = scene([&third, &third, &fourth], &Arc::new("toolbar"));

    assert_eq!(before, after);
    let text = before.to_string();
    let groups: Vec<_> = before
        .children
        .iter()
        .filter_map(|node| node.field("group"))
        .map(|field| field.value.to_string())
        .collect();
    assert_eq!(
        groups,
        ["BackdropGroup#1", "BackdropGroup#1", "BackdropGroup#2"],
        "{text}"
    );
    let payloads: Vec<_> = before
        .children
        .iter()
        .filter_map(|node| node.field("instance"))
        .map(|field| field.value.to_string())
        .collect();
    assert_eq!(payloads, ["&str#1", "&str#1"], "{text}");
}
