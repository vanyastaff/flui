use flui_foundation::RenderId;
use flui_foundation::geometry::{Matrix4, Offset, Rect, Size};
use flui_painting::Canvas;
use flui_painting::paint::{BlendMode, Clip, ImageFilter, Paint};
use flui_painting::styling::Color;

use super::{DamageMode, LayerDiffer};
use crate::{
    BackdropFilterLayer, ClipRectLayer, ContentToken, DamageRect, DamageRegion, FollowerLayer,
    Layer, LayerId, LayerLink, LayerNode, LayerTree, LeaderLayer, OffsetLayer, OpacityLayer,
    PerformanceOverlayLayer, PictureLayer, Scene, TextureLayer, TransformLayer,
};

const SURFACE: (u32, u32) = (400, 400);
const ROOT: u64 = 1;

fn id(raw: u64) -> RenderId {
    RenderId::new(raw as usize)
}

fn picture(rect: Rect<f64>) -> Layer {
    let mut canvas = Canvas::new();
    canvas.draw_rect(rect, &Paint::fill(Color::RED));
    Layer::from(PictureLayer::new(canvas.finish()))
}

fn full_fill() -> Layer {
    let mut canvas = Canvas::new();
    canvas.draw_color(Color::BLUE, BlendMode::SrcOver);
    Layer::from(PictureLayer::new(canvas.finish()))
}

/// A frame under construction: a stamped root, scaled by `dpr` as the paint
/// pass's root transform is, holding one small background picture.
struct Frame {
    tree: LayerTree,
}

impl Frame {
    fn new(dpr: f64, root_token: &ContentToken) -> Self {
        let root = LayerNode::new(Layer::from(TransformLayer::new(Matrix4::scaling(
            dpr, dpr, 1.0,
        ))))
        .with_boundary(id(ROOT), root_token.clone());
        let mut tree = LayerTree::new(root);
        let root_id = tree.root();
        tree.push_child(root_id, picture(Rect::from_xywh(0.0, 0.0, 10.0, 10.0)));
        Self { tree }
    }

    fn root(&self) -> LayerId {
        self.tree.root()
    }

    /// A boundary child: a stamped `OffsetLayer` at `at` with one picture of
    /// `size` at its origin, as the paint pass rebases a boundary.
    fn boundary(
        &mut self,
        parent: LayerId,
        raw: u64,
        token: &ContentToken,
        at: Offset<f64>,
        size: Size<f64>,
    ) -> LayerId {
        let node =
            LayerNode::new(Layer::from(OffsetLayer::new(at))).with_boundary(id(raw), token.clone());
        let layer = self.tree.push_child(parent, node);
        self.tree.push_child(
            layer,
            picture(Rect::from_xywh(0.0, 0.0, size.width, size.height)),
        );
        layer
    }

    fn push(&mut self, parent: LayerId, layer: impl Into<Layer>) -> LayerId {
        self.tree.push_child(parent, layer.into())
    }

    fn scene(self) -> Scene {
        Scene::new(self.tree)
    }
}

fn partial(region: DamageRegion) -> (u32, u32, u32, u32) {
    match region {
        DamageRegion::Partial(rect) => (rect.left(), rect.top(), rect.right(), rect.bottom()),
        other => panic!("expected a partial region, got {other:?}"),
    }
}

/// The damage rect a surface-pixel rect becomes.
fn covering(l: f64, t: f64, r: f64, b: f64) -> (u32, u32, u32, u32) {
    let rect = DamageRect::covering(Rect::from_ltrb(l, t, r, b), SURFACE).expect("on the surface");
    (rect.left(), rect.top(), rect.right(), rect.bottom())
}

fn one_boundary(root: &ContentToken, token: &ContentToken, at: Offset<f64>) -> Scene {
    let mut frame = Frame::new(1.0, root);
    let parent = frame.root();
    frame.boundary(parent, 2, token, at, Size::new(20.0, 20.0));
    frame.scene()
}

#[test]
fn first_frame_is_full() {
    let mut differ = LayerDiffer::default();
    let root = ContentToken::mint();
    let child = ContentToken::mint();
    assert_eq!(
        differ.diff(
            &one_boundary(&root, &child, Offset::new(50.0, 50.0)),
            SURFACE
        ),
        DamageRegion::Full
    );
    assert_eq!(differ.retained_boundaries(), 2);
}

#[test]
fn identical_tokens_are_unchanged() {
    let mut differ = LayerDiffer::default();
    let root = ContentToken::mint();
    let child = ContentToken::mint();
    let at = Offset::new(50.0, 50.0);
    differ.diff(&one_boundary(&root, &child, at), SURFACE);
    // A freshly built tree with freshly recorded pictures: only the tokens
    // say the content is the same.
    assert_eq!(
        differ.diff(&one_boundary(&root, &child, at), SURFACE),
        DamageRegion::Unchanged
    );
}

/// The root carries the device pixel ratio; a region computed in logical
/// pixels would be half the size and in the wrong place.
#[test]
fn a_new_token_damages_that_boundary_in_physical_pixels() {
    let build = |child: &ContentToken, root: &ContentToken| {
        let mut frame = Frame::new(2.0, root);
        let parent = frame.root();
        frame.boundary(
            parent,
            2,
            child,
            Offset::new(50.0, 60.0),
            Size::new(20.0, 10.0),
        );
        frame.scene()
    };
    let mut differ = LayerDiffer::default();
    let root = ContentToken::mint();
    differ.diff(&build(&ContentToken::mint(), &root), SURFACE);
    let region = differ.diff(&build(&ContentToken::mint(), &root), SURFACE);
    assert_eq!(partial(region), covering(100.0, 120.0, 140.0, 140.0));
}

/// A differ that damaged only the new bounds would leave the old position
/// showing the boundary's last frame.
#[test]
fn a_moved_boundary_damages_old_and_new() {
    let mut differ = LayerDiffer::default();
    let root = ContentToken::mint();
    let child = ContentToken::mint();
    differ.diff(
        &one_boundary(&root, &child, Offset::new(10.0, 20.0)),
        SURFACE,
    );
    let region = differ.diff(
        &one_boundary(&root, &child, Offset::new(100.0, 20.0)),
        SURFACE,
    );
    assert_eq!(partial(region), covering(10.0, 20.0, 120.0, 40.0));
}

#[test]
fn removed_and_added_boundaries() {
    let mut differ = LayerDiffer::default();
    let root = ContentToken::mint();
    let child = ContentToken::mint();
    differ.diff(
        &one_boundary(&root, &child, Offset::new(10.0, 20.0)),
        SURFACE,
    );

    // Removed: the old region.
    let empty = Frame::new(1.0, &root).scene();
    assert_eq!(
        partial(differ.diff(&empty, SURFACE)),
        covering(10.0, 20.0, 30.0, 40.0)
    );

    // Added: the new region.
    assert_eq!(
        partial(differ.diff(
            &one_boundary(&root, &child, Offset::new(200.0, 200.0)),
            SURFACE
        )),
        covering(200.0, 200.0, 220.0, 220.0)
    );
}

/// An effect above a boundary changes the pixels its content lands on while
/// the content itself, and its token, stay the same.
#[test]
fn an_ancestor_opacity_change_damages_nested_boundaries() {
    let build = |alpha: f64, outer: &ContentToken, inner: &ContentToken, root: &ContentToken| {
        let mut frame = Frame::new(1.0, root);
        let parent = frame.root();
        let opacity = frame.push(parent, OpacityLayer::new(alpha));
        let outer_id = frame.boundary(
            opacity,
            2,
            outer,
            Offset::new(0.0, 100.0),
            Size::new(10.0, 10.0),
        );
        frame.boundary(
            outer_id,
            3,
            inner,
            Offset::new(200.0, 0.0),
            Size::new(30.0, 30.0),
        );
        frame.scene()
    };
    let (root, outer, inner) = (
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    );
    let mut differ = LayerDiffer::default();
    differ.diff(&build(1.0, &outer, &inner, &root), SURFACE);
    let region = differ.diff(&build(0.5, &outer, &inner, &root), SURFACE);
    // The outer boundary's own picture and the nested one's, both under the
    // changed opacity; the root's own picture at the origin is not.
    assert_eq!(partial(region), covering(0.0, 100.0, 230.0, 130.0));
    assert_eq!(
        differ.diff(&build(0.5, &outer, &inner, &root), SURFACE),
        DamageRegion::Unchanged
    );
}

#[test]
fn clip_bounds_the_region() {
    let build = |token: &ContentToken, root: &ContentToken| {
        let mut frame = Frame::new(1.0, root);
        let parent = frame.root();
        let clip = frame.push(
            parent,
            ClipRectLayer::new(Rect::from_ltrb(0.0, 0.0, 60.0, 60.0), Clip::HardEdge),
        );
        frame.boundary(
            clip,
            2,
            token,
            Offset::new(50.0, 50.0),
            Size::new(100.0, 100.0),
        );
        frame.scene()
    };
    let root = ContentToken::mint();
    let mut differ = LayerDiffer::default();
    differ.diff(&build(&ContentToken::mint(), &root), SURFACE);
    let region = differ.diff(&build(&ContentToken::mint(), &root), SURFACE);
    assert_eq!(partial(region), covering(50.0, 50.0, 60.0, 60.0));
}

/// A full-canvas fill in a boundary reaches everything its clip allows, not
/// the (absent) layout bounds of the picture.
#[test]
fn unbounded_picture_takes_the_clip() {
    let build = |token: &ContentToken, root: &ContentToken| {
        let mut frame = Frame::new(1.0, root);
        let parent = frame.root();
        let clip = frame.push(
            parent,
            ClipRectLayer::new(Rect::from_ltrb(100.0, 100.0, 180.0, 150.0), Clip::HardEdge),
        );
        let boundary = frame.tree.push_child(
            clip,
            LayerNode::new(Layer::from(OffsetLayer::new(Offset::new(120.0, 120.0))))
                .with_boundary(id(2), token.clone()),
        );
        frame.push(boundary, full_fill());
        frame.scene()
    };
    let root = ContentToken::mint();
    let mut differ = LayerDiffer::default();
    differ.diff(&build(&ContentToken::mint(), &root), SURFACE);
    let region = differ.diff(&build(&ContentToken::mint(), &root), SURFACE);
    assert_eq!(partial(region), covering(100.0, 100.0, 180.0, 150.0));
}

#[test]
fn unstamped_root_is_full() {
    let scene = || {
        let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
        let root = tree.root();
        tree.push_child(root, picture(Rect::from_xywh(0.0, 0.0, 10.0, 10.0)));
        Scene::new(tree)
    };
    let mut differ = LayerDiffer::default();
    assert_eq!(differ.diff(&scene(), SURFACE), DamageRegion::Full);
    assert_eq!(differ.diff(&scene(), SURFACE), DamageRegion::Full);
    assert_eq!(differ.retained_boundaries(), 0);
}

#[test]
fn root_id_change_is_full() {
    let token = ContentToken::mint();
    let scene = |root_id: u64| {
        let tree = LayerTree::new(
            LayerNode::new(Layer::from(OffsetLayer::zero()))
                .with_boundary(id(root_id), token.clone()),
        );
        Scene::new(tree)
    };
    let mut differ = LayerDiffer::default();
    differ.diff(&scene(1), SURFACE);
    assert_eq!(differ.diff(&scene(1), SURFACE), DamageRegion::Unchanged);
    assert_eq!(differ.diff(&scene(9), SURFACE), DamageRegion::Full);
    // A root placement change (the device pixel ratio) is Full too.
    let root = ContentToken::mint();
    differ.diff(&Frame::new(1.0, &root).scene(), SURFACE);
    assert_eq!(
        differ.diff(&Frame::new(2.0, &root).scene(), SURFACE),
        DamageRegion::Full
    );
}

#[test]
fn size_change_is_full() {
    let root = ContentToken::mint();
    let child = ContentToken::mint();
    let at = Offset::new(10.0, 10.0);
    let mut differ = LayerDiffer::default();
    differ.diff(&one_boundary(&root, &child, at), SURFACE);
    assert_eq!(
        differ.diff(&one_boundary(&root, &child, at), (401, 400)),
        DamageRegion::Full
    );
}

#[test]
fn textures_and_overlays_are_damaged_every_frame() {
    let build = |root: &ContentToken| {
        let mut frame = Frame::new(1.0, root);
        let parent = frame.root();
        frame.push(
            parent,
            TextureLayer::new(
                flui_painting::paint::TextureId::new(7),
                Rect::from_xywh(100.0, 100.0, 50.0, 50.0),
            ),
        );
        frame.push(
            parent,
            PerformanceOverlayLayer::all_stats(Rect::from_xywh(300.0, 0.0, 100.0, 20.0)),
        );
        frame.scene()
    };
    let root = ContentToken::mint();
    let mut differ = LayerDiffer::default();
    differ.diff(&build(&root), SURFACE);
    for _ in 0..2 {
        assert_eq!(
            partial(differ.diff(&build(&root), SURFACE)),
            covering(100.0, 0.0, 400.0, 150.0),
            "no token vouches for a texture's or an overlay's pixels"
        );
    }
}

/// Two overlapping sibling boundaries that swap paint order keep their
/// tokens and transforms; the swap still damages the one that moved, which
/// covers their overlap, so the one now on top is painted over the other. A boundary inserted before them shifts their
/// positions without changing their relative order and damages only itself.
#[test]
fn a_paint_order_swap_of_overlapping_siblings_damages_their_overlap() {
    let tokens = [
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    ];
    let build = |order: &[u64]| {
        let mut frame = Frame::new(1.0, &tokens[0]);
        let parent = frame.root();
        for &raw in order {
            let at = match raw {
                2 => Offset::new(100.0, 100.0),
                3 => Offset::new(120.0, 120.0),
                _ => Offset::new(300.0, 300.0),
            };
            frame.boundary(
                parent,
                raw,
                &tokens[raw as usize - 1],
                at,
                Size::new(40.0, 40.0),
            );
        }
        frame.scene()
    };
    let mut differ = LayerDiffer::default();
    differ.diff(&build(&[2, 3]), SURFACE);
    let (l, t, r, b) = partial(differ.diff(&build(&[3, 2]), SURFACE));
    let (overlap_l, overlap_t, overlap_r, overlap_b) = covering(120.0, 120.0, 140.0, 140.0);
    assert!(
        l <= overlap_l && t <= overlap_t && r >= overlap_r && b >= overlap_b,
        "the overlap the swap changes is damaged: {:?}",
        (l, t, r, b)
    );
    assert_eq!(
        partial(differ.diff(&build(&[4, 3, 2]), SURFACE)),
        covering(300.0, 300.0, 340.0, 340.0),
        "an insertion ahead of them damages only the new boundary"
    );
}

/// A picture that draws an external texture (`Canvas::draw_texture`) keeps
/// its boundary's token while the texture's producer replaces the content
/// behind the same id: the texture's rect is damaged on every frame, the
/// rest of the boundary is not.
#[test]
fn a_pictures_texture_draw_is_damaged_every_frame() {
    let (root, card) = (ContentToken::mint(), ContentToken::mint());
    let build = || {
        let mut frame = Frame::new(1.0, &root);
        let parent = frame.root();
        let node = LayerNode::new(Layer::from(OffsetLayer::new(Offset::new(100.0, 100.0))))
            .with_boundary(id(2), card.clone());
        let boundary = frame.tree.push_child(parent, node);
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 200.0, 200.0),
            &Paint::fill(Color::RED),
        );
        canvas.draw_texture(
            flui_painting::paint::TextureId::new(7),
            Rect::from_xywh(10.0, 20.0, 30.0, 40.0),
            None,
            flui_painting::paint::FilterQuality::None,
            1.0,
        );
        frame.push(boundary, PictureLayer::new(canvas.finish()));
        frame.scene()
    };
    let mut differ = LayerDiffer::default();
    differ.diff(&build(), SURFACE);
    for _ in 0..2 {
        assert_eq!(
            partial(differ.diff(&build(), SURFACE)),
            covering(110.0, 120.0, 140.0, 160.0),
            "the texture's rect, and only it, is damaged under an unchanged token"
        );
    }
}

/// A shadow's blur reaches as far on an axis a boundary's transform
/// compresses as on the one it stretches, because the renderer blurs with
/// one sigma from the largest scale: a removed shadow under `scale(4, 0.25)`
/// damages its blur's full reach vertically too.
#[test]
fn a_shadow_under_a_non_uniform_scale_damages_its_blur_on_both_axes() {
    let root = ContentToken::mint();
    let with_shadow = |shadow: bool, card: &ContentToken| {
        let mut frame = Frame::new(1.0, &root);
        let parent = frame.root();
        let node = LayerNode::new(Layer::from(TransformLayer::new(
            Matrix4::translation(100.0, 200.0, 0.0) * Matrix4::scaling(4.0, 0.25, 1.0),
        )))
        .with_boundary(id(2), card.clone());
        let boundary = frame.tree.push_child(parent, node);
        if shadow {
            let mut canvas = Canvas::new();
            canvas.draw_shadow(
                &flui_painting::paint::Path::rectangle(Rect::from_xywh(0.0, 0.0, 10.0, 40.0)),
                Color::BLACK,
                2.0,
            );
            frame.push(boundary, PictureLayer::new(canvas.finish()));
        }
        frame.scene()
    };
    let mut differ = LayerDiffer::default();
    differ.diff(&with_shadow(true, &ContentToken::mint()), SURFACE);
    // Device rect (100, 200)-(140, 210); reach 3.5 x elevation 2 x scale 4.
    assert_eq!(
        partial(differ.diff(&with_shadow(false, &ContentToken::mint()), SURFACE)),
        covering(72.0, 172.0, 168.0, 238.0)
    );
}

/// A leader that moves carries its follower's content with it, while the
/// boundary holding the follower keeps its token.
#[test]
fn a_leader_move_damages_its_follower() {
    let link = LayerLink::new();
    let build = |leader_at: Offset<f64>, tokens: &[ContentToken; 3]| {
        let mut frame = Frame::new(1.0, &tokens[0]);
        let parent = frame.root();
        let leader_boundary = frame.boundary(parent, 2, &tokens[1], leader_at, Size::new(5.0, 5.0));
        frame.push(leader_boundary, LeaderLayer::new(link, Size::new(5.0, 5.0)));
        let follower_boundary =
            frame.boundary(parent, 3, &tokens[2], Offset::ZERO, Size::new(1.0, 1.0));
        let follower = frame.push(follower_boundary, FollowerLayer::new(link));
        frame.push(follower, picture(Rect::from_xywh(0.0, 0.0, 30.0, 30.0)));
        frame.scene()
    };
    let tokens = [
        ContentToken::mint(),
        ContentToken::mint(),
        ContentToken::mint(),
    ];
    let mut differ = LayerDiffer::default();
    differ.diff(&build(Offset::new(100.0, 100.0), &tokens), SURFACE);
    let moved = [tokens[0].clone(), ContentToken::mint(), tokens[2].clone()];
    let region = differ.diff(&build(Offset::new(200.0, 100.0), &moved), SURFACE);
    // Both follower positions (100..130 and 200..230), and the leader's
    // repainted boundary at both positions.
    assert_eq!(partial(region), covering(100.0, 100.0, 230.0, 130.0));
}

fn backdrop_scene(tokens: &[ContentToken; 2], changed_at: Offset<f64>) -> Scene {
    let mut frame = Frame::new(1.0, &tokens[0]);
    let parent = frame.root();
    frame.boundary(parent, 2, &tokens[1], changed_at, Size::new(20.0, 20.0));
    frame.push(
        parent,
        BackdropFilterLayer::new(
            ImageFilter::blur(2.0),
            BlendMode::SrcOver,
            Rect::from_xywh(200.0, 200.0, 100.0, 100.0),
        ),
    );
    frame.scene()
}

#[test]
fn damage_meeting_a_backdrop_includes_the_backdrop() {
    let root = ContentToken::mint();
    let at = Offset::new(190.0, 190.0);
    let mut differ = LayerDiffer::default();
    differ.diff(
        &backdrop_scene(&[root.clone(), ContentToken::mint()], at),
        SURFACE,
    );
    let region = differ.diff(&backdrop_scene(&[root, ContentToken::mint()], at), SURFACE);
    assert_eq!(partial(region), covering(190.0, 190.0, 300.0, 300.0));
}

#[test]
fn damage_disjoint_from_a_backdrop_does_not_expand() {
    let root = ContentToken::mint();
    // Further from the backdrop than its blur reads (3 sigma = 6 px).
    let at = Offset::new(10.0, 10.0);
    let mut differ = LayerDiffer::default();
    differ.diff(
        &backdrop_scene(&[root.clone(), ContentToken::mint()], at),
        SURFACE,
    );
    let region = differ.diff(&backdrop_scene(&[root, ContentToken::mint()], at), SURFACE);
    assert_eq!(partial(region), covering(10.0, 10.0, 30.0, 30.0));
}

#[test]
fn damage_over_threshold_is_full() {
    let root = ContentToken::mint();
    let build = |token: &ContentToken| {
        let mut frame = Frame::new(1.0, &root);
        let parent = frame.root();
        frame.boundary(parent, 2, token, Offset::ZERO, Size::new(300.0, 300.0));
        frame.scene()
    };
    let mut differ = LayerDiffer::new(DamageMode::On { full_above: 0.5 });
    differ.diff(&build(&ContentToken::mint()), SURFACE);
    assert_eq!(
        differ.diff(&build(&ContentToken::mint()), SURFACE),
        DamageRegion::Full,
        "301x301 of 400x400 is over half the surface"
    );
    let mut lenient = LayerDiffer::new(DamageMode::On { full_above: 0.9 });
    lenient.diff(&build(&ContentToken::mint()), SURFACE);
    assert!(matches!(
        lenient.diff(&build(&ContentToken::mint()), SURFACE),
        DamageRegion::Partial(_)
    ));
}

#[test]
fn off_retains_nothing_and_is_always_full() {
    let root = ContentToken::mint();
    let child = ContentToken::mint();
    let at = Offset::new(10.0, 10.0);
    let mut differ = LayerDiffer::new(DamageMode::Off);
    for _ in 0..3 {
        assert_eq!(
            differ.diff(&one_boundary(&root, &child, at), SURFACE),
            DamageRegion::Full
        );
        assert_eq!(differ.retained_boundaries(), 0);
    }

    // Switching off releases what an on differ held; forgetting makes the
    // next diff full.
    let mut differ = LayerDiffer::default();
    differ.diff(&one_boundary(&root, &child, at), SURFACE);
    assert_eq!(differ.retained_boundaries(), 2);
    differ.forget();
    assert_eq!(
        differ.diff(&one_boundary(&root, &child, at), SURFACE),
        DamageRegion::Full
    );
    differ.set_mode(DamageMode::Off);
    assert_eq!(differ.retained_boundaries(), 0);
}

#[test]
fn a_boundary_stamped_twice_is_full() {
    let root = ContentToken::mint();
    let child = ContentToken::mint();
    let build = || {
        let mut frame = Frame::new(1.0, &root);
        let parent = frame.root();
        frame.boundary(parent, 2, &child, Offset::ZERO, Size::new(5.0, 5.0));
        frame.boundary(
            parent,
            2,
            &child,
            Offset::new(50.0, 0.0),
            Size::new(5.0, 5.0),
        );
        frame.scene()
    };
    let mut differ = LayerDiffer::default();
    differ.diff(&build(), SURFACE);
    assert_eq!(differ.diff(&build(), SURFACE), DamageRegion::Full);
}
