use std::f64::consts::FRAC_PI_2;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::LayerId;
use flui_foundation::geometry::{Matrix4, Offset, Rect, Size};
use flui_layer::{
    ClipRectLayer, FollowerLayer, Layer, LayerLink, LayerTree, LeaderClip, LeaderLayer,
    OffsetLayer, TransformLayer, resolve_follower_transform,
};
use flui_painting::paint::{Alignment, Clip};

fn offset(dx: f64, dy: f64) -> Layer {
    Layer::from(OffsetLayer::new(Offset::new(dx, dy)))
}

fn transform(matrix: Matrix4) -> Layer {
    Layer::from(TransformLayer::new(matrix))
}

fn clip(rect: Rect<f64>) -> Layer {
    Layer::from(ClipRectLayer::new(rect, Clip::HardEdge))
}

fn leader(link: LayerLink, dx: f64, dy: f64) -> Layer {
    Layer::from(LeaderLayer::with_offset(
        link,
        Size::new(20.0, 10.0),
        Offset::new(dx, dy),
    ))
}

fn follower(link: LayerLink) -> FollowerLayer {
    FollowerLayer::new(link).with_size(Size::new(10.0, 10.0))
}

/// Where the resolved transform puts the follower-local point `(x, y)`, in
/// the coordinates of the follower's parent.
fn place(tree: &LayerTree, id: LayerId, x: f64, y: f64) -> (f64, f64) {
    let transform =
        resolve_follower_transform(tree, id).expect("the follower resolves to a transform");
    let (px, py) = transform.transform_point(x, y);
    // Rounded to a micro-pixel so rotations compare exactly.
    let round = |value: f64| (value * 1e6).round() / 1e6;
    (round(px), round(py))
}

/// Leader and follower under two different `Offset` branches: both chains
/// are walked to the root, not assumed to share an immediate parent.
pub(crate) fn linked_across_offset_branches_sums_both_chains() {
    let link = LayerLink::new();
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    let branch_a = tree.push_child(root, offset(100.0, 0.0));
    tree.push_child(branch_a, leader(link, 5.0, 5.0));
    let branch_b = tree.push_child(root, offset(0.0, 200.0));
    let id = tree.push_child(branch_b, Layer::from(follower(link)));

    // Leader absolute (105,5) minus the follower's pushed position (0,200).
    assert_eq!(place(&tree, id, 0.0, 0.0), (105.0, -195.0));
}

/// A scale above the leader moves the anchor and scales the follower's
/// content; a translation-only chain would put it at (10, 30).
pub(crate) fn a_scaled_leader_scales_the_follower() {
    let link = LayerLink::new();
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    let scaled = tree.push_child(root, transform(Matrix4::scaling(2.0, 2.0, 1.0)));
    tree.push_child(scaled, leader(link, 10.0, 20.0));
    let id = tree.push_child(
        root,
        Layer::from(follower(link).with_leader_anchor(Alignment::BOTTOM_LEFT)),
    );

    // The leader's bottom-left is (10, 30) under the scale, (20, 60) above it.
    assert_eq!(place(&tree, id, 0.0, 0.0), (20.0, 60.0));
    assert_eq!(place(&tree, id, 1.0, 0.0), (22.0, 60.0));
}

/// A rotation above the leader rotates the follower with it.
pub(crate) fn a_rotated_leader_rotates_the_follower() {
    let link = LayerLink::new();
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    let rotated = tree.push_child(root, transform(Matrix4::rotation_z(FRAC_PI_2)));
    tree.push_child(rotated, leader(link, 10.0, 0.0));
    let id = tree.push_child(root, Layer::from(follower(link)));

    assert_eq!(place(&tree, id, 0.0, 0.0), (0.0, 10.0));
    assert_eq!(place(&tree, id, 1.0, 0.0), (0.0, 11.0));
}

/// A scale above the follower is undone, so its content lands on the leader
/// at the leader's own scale.
pub(crate) fn a_follower_under_a_scaled_parent_undoes_its_scale() {
    let link = LayerLink::new();
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    tree.push_child(root, leader(link, 40.0, 40.0));
    let scaled = tree.push_child(root, transform(Matrix4::scaling(2.0, 2.0, 1.0)));
    let id = tree.push_child(scaled, Layer::from(follower(link)));

    assert_eq!(place(&tree, id, 0.0, 0.0), (20.0, 20.0));
    assert_eq!(place(&tree, id, 2.0, 0.0), (21.0, 20.0));
}

/// A leader painted inside another follower (a submenu anchored in a menu)
/// is placed through that follower's own resolved transform.
pub(crate) fn a_leader_inside_another_follower_resolves_through_it() {
    let menu_link = LayerLink::new();
    let submenu_link = LayerLink::new();
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    tree.push_child(root, leader(menu_link, 100.0, 0.0));
    let menu = tree.push_child(root, Layer::from(follower(menu_link)));
    tree.push_child(menu, leader(submenu_link, 5.0, 5.0));
    let submenu = tree.push_child(root, Layer::from(follower(submenu_link)));

    assert_eq!(place(&tree, submenu, 0.0, 0.0), (105.0, 5.0));
}

/// A zero scale on either chain collapses the follower: it resolves to
/// nothing rather than to a singular or non-finite transform.
pub(crate) fn a_singular_transform_on_a_chain_resolves_to_nothing() {
    for on_leader_chain in [true, false] {
        let link = LayerLink::new();
        let mut tree = LayerTree::new(offset(0.0, 0.0));
        let root = tree.root();
        let zero = Matrix4::scaling(0.0, 0.0, 1.0);
        let leader_parent = if on_leader_chain {
            tree.push_child(root, transform(zero))
        } else {
            root
        };
        tree.push_child(leader_parent, leader(link, 10.0, 10.0));
        let follower_parent = if on_leader_chain {
            root
        } else {
            tree.push_child(root, transform(zero))
        };
        let id = tree.push_child(follower_parent, Layer::from(follower(link)));

        assert_eq!(
            resolve_follower_transform(&tree, id),
            None,
            "on the leader chain: {on_leader_chain}"
        );
    }
}

/// A non-finite matrix on a chain is never published.
pub(crate) fn a_non_finite_transform_on_a_chain_resolves_to_nothing() {
    let link = LayerLink::new();
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    let broken = tree.push_child(root, transform(Matrix4::translation(f64::NAN, 0.0, 0.0)));
    tree.push_child(broken, leader(link, 10.0, 10.0));
    let id = tree.push_child(root, Layer::from(follower(link)));

    assert_eq!(resolve_follower_transform(&tree, id), None);
}

/// Counts the warnings emitted while it is the thread's default subscriber.
struct WarningCount(AtomicUsize);

impl tracing::Subscriber for WarningCount {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::WARN {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Resolves `id` with a warning counter installed for this thread.
fn resolve_counting_warnings(tree: &LayerTree, id: LayerId) -> (Option<Matrix4>, usize) {
    let counter = std::sync::Arc::new(WarningCount(AtomicUsize::new(0)));
    let resolved =
        tracing::subscriber::with_default(counter.clone(), || resolve_follower_transform(tree, id));
    (resolved, counter.0.load(Ordering::Relaxed))
}

/// A chain of followers each anchored in the previous one: a handful
/// resolves, and past the nesting limit resolution gives up with one
/// warning in a debug build instead of recursing without bound.
pub(crate) fn followers_nested_past_the_limit_resolve_to_nothing() {
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    let mut link = LayerLink::new();
    tree.push_child(root, leader(link, 1.0, 0.0));
    let mut followers = Vec::new();
    for _ in 0..40 {
        let id = tree.push_child(root, Layer::from(follower(link)));
        link = LayerLink::new();
        tree.push_child(id, leader(link, 1.0, 0.0));
        followers.push(id);
    }

    assert_eq!(place(&tree, followers[2], 0.0, 0.0), (3.0, 0.0));
    let (resolved, warnings) = resolve_counting_warnings(&tree, followers[39]);
    assert_eq!(resolved, None);
    assert_eq!(warnings, usize::from(cfg!(debug_assertions)));
}

/// Two followers whose leaders each sit inside the other form a cycle; it
/// ends at the nesting limit with one warning in a debug build.
pub(crate) fn a_cycle_through_leaders_resolves_to_nothing() {
    let first_link = LayerLink::new();
    let second_link = LayerLink::new();
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    let first = tree.push_child(root, Layer::from(follower(first_link)));
    tree.push_child(first, leader(second_link, 1.0, 1.0));
    let second = tree.push_child(root, Layer::from(follower(second_link)));
    tree.push_child(second, leader(first_link, 1.0, 1.0));

    let (resolved, warnings) = resolve_counting_warnings(&tree, first);
    assert_eq!(resolved, None);
    assert_eq!(warnings, usize::from(cfg!(debug_assertions)));
}

/// A viewport-like clip around a scrolled leader, with the follower outside
/// it; `scroll` moves the content up.
fn scrolled_leader(scroll: f64, leader_clip: LeaderClip) -> (LayerTree, LayerId) {
    let link = LayerLink::new();
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    let viewport = tree.push_child(root, clip(Rect::from_xywh(0.0, 0.0, 100.0, 100.0)));
    let content = tree.push_child(viewport, offset(0.0, -scroll));
    tree.push_child(
        content,
        Layer::from(LeaderLayer::with_offset(
            link,
            Size::ZERO,
            Offset::new(10.0, 130.0),
        )),
    );
    let id = tree.push_child(
        root,
        Layer::from(follower(link).with_leader_clip(leader_clip)),
    );
    (tree, id)
}

/// Under `Hide`, a follower resolves to nothing while its leader's anchor is
/// outside a clip on the leader's chain, and comes back with it; an anchor on
/// the clip's edge counts as inside. `Escape` ignores the clip.
pub(crate) fn hide_follows_the_anchor_through_the_leader_clip() {
    for (scroll, visible) in [(0.0, false), (29.0, false), (30.0, true), (100.0, true)] {
        let (tree, id) = scrolled_leader(scroll, LeaderClip::Hide);
        assert_eq!(
            resolve_follower_transform(&tree, id).is_some(),
            visible,
            "scroll {scroll}"
        );
        let (tree, id) = scrolled_leader(scroll, LeaderClip::Escape);
        assert_eq!(
            place(&tree, id, 0.0, 0.0),
            (10.0, 130.0 - scroll),
            "escape, scroll {scroll}"
        );
    }
}

/// A clip at the common ancestor already clips the follower itself and is
/// not consulted.
pub(crate) fn a_clip_at_the_common_ancestor_is_not_consulted() {
    let link = LayerLink::new();
    let mut tree = LayerTree::new(clip(Rect::from_xywh(0.0, 0.0, 10.0, 10.0)));
    let root = tree.root();
    tree.push_child(root, leader(link, 50.0, 50.0));
    let id = tree.push_child(
        root,
        Layer::from(follower(link).with_leader_clip(LeaderClip::Hide)),
    );

    assert_eq!(place(&tree, id, 0.0, 0.0), (50.0, 50.0));
}

/// The anchor is tested in each clip's own coordinates: under a scale of 2 a
/// clip of 50 holds an anchor at 40, which is at 80 on screen.
pub(crate) fn a_clip_under_a_transform_tests_the_mapped_anchor() {
    for (anchor, visible) in [(40.0, true), (60.0, false)] {
        let link = LayerLink::new();
        let mut tree = LayerTree::new(offset(0.0, 0.0));
        let root = tree.root();
        let scaled = tree.push_child(root, transform(Matrix4::scaling(2.0, 2.0, 1.0)));
        let clipped = tree.push_child(scaled, clip(Rect::from_xywh(0.0, 0.0, 50.0, 50.0)));
        tree.push_child(
            clipped,
            Layer::from(LeaderLayer::with_offset(
                link,
                Size::ZERO,
                Offset::new(anchor, anchor),
            )),
        );
        let id = tree.push_child(
            root,
            Layer::from(follower(link).with_leader_clip(LeaderClip::Hide)),
        );
        assert_eq!(
            resolve_follower_transform(&tree, id).is_some(),
            visible,
            "anchor {anchor}"
        );
    }
}

/// Unlinked: shown at `target_offset` from the paint origin, or nothing.
pub(crate) fn an_unlinked_follower_takes_the_unlinked_contract() {
    let mut tree = LayerTree::new(offset(0.0, 0.0));
    let root = tree.root();
    let shown = tree.push_child(
        root,
        Layer::from(follower(LayerLink::new()).with_target_offset(Offset::new(3.0, 4.0))),
    );
    let hidden = tree.push_child(
        root,
        Layer::from(follower(LayerLink::new()).with_show_when_unlinked(false)),
    );

    assert_eq!(place(&tree, shown, 0.0, 0.0), (3.0, 4.0));
    assert_eq!(resolve_follower_transform(&tree, hidden), None);
}

/// A leader painted inside the follower's subtree cannot anchor it (it is
/// composited after the follower); the unlinked contract applies.
pub(crate) fn leader_inside_the_follower_subtree_falls_back_to_unlinked() {
    let link = LayerLink::new();
    let follower = FollowerLayer::new(link)
        .with_show_when_unlinked(true)
        .with_target_offset(Offset::new(3.0, 4.0));
    let mut tree = LayerTree::new(Layer::from(follower));
    let id = tree.root();
    tree.push_child(id, leader(link, 50.0, 50.0));

    assert_eq!(place(&tree, id, 0.0, 0.0), (3.0, 4.0));
}
