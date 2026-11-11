//! The retained scene: patches applied all or nothing, content versions that
//! never repeat across owners, and a write that costs the patch, not the
//! scene (ADR-0304 §1–§3).

use std::sync::Arc;

use flui_foundation::geometry::{Offset, Rect, Size};
use flui_foundation::{RenderId, SurfaceGeneration};
use flui_layer::{
    BackdropFilterLayer, BoundaryContent, Chunk, DamageRegion, FollowerLayer, FullReason, Layer,
    LayerLink, LeaderLayer, OpacityLayer, Placement, ResyncReason, SceneError, SceneNode,
    SceneNodeId, SceneOp, SceneOwner, SceneVersion, SurfaceKey,
};
use flui_painting::paint::{BlendMode, FilterQuality, ImageFilter, TextureId};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

const SURFACE: SurfaceKey = SurfaceKey {
    size: (800, 600),
    generation: SurfaceGeneration::ZERO,
};

fn owner_id() -> RenderId {
    RenderId::new(1)
}

/// Content of slots for `children`, in order.
fn slots(children: &[SceneNodeId]) -> Arc<BoundaryContent> {
    let mut content = BoundaryContent::builder();
    for child in children {
        content.slot(*child);
    }
    Arc::new(content.finish())
}

fn empty() -> Arc<BoundaryContent> {
    Arc::new(BoundaryContent::default())
}

fn publish(node: SceneNodeId, content: Arc<BoundaryContent>) -> SceneOp {
    SceneOp::Publish { node, content }
}

/// An owner whose root names `n` empty leaves; returns the leaves.
fn owner_with_leaves(n: usize) -> (SceneOwner, Vec<SceneNodeId>) {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let leaves: Vec<SceneNodeId> = (0..n).map(|_| writer.mint_node()).collect();
    for leaf in &leaves {
        writer.push(publish(*leaf, empty()));
    }
    writer.push(publish(root, slots(&leaves)));
    owner.apply(SURFACE).expect("a well-formed patch applies");
    (owner, leaves)
}

/// Applies `ops` as one patch.
fn apply(owner: &mut SceneOwner, ops: impl IntoIterator<Item = SceneOp>) -> Result<(), SceneError> {
    let mut writer = owner.writer();
    for op in ops {
        writer.push(op);
    }
    owner.apply(SURFACE).map(|_| ())
}

/// The version is the one before the patch, node for node.
fn assert_same_version(before: &SceneVersion, after: &SceneVersion) {
    assert_eq!(before.id(), after.id());
    assert_eq!(before.len(), after.len());
    for (old, new) in before.nodes().zip(after.nodes()) {
        assert_eq!(old.id(), new.id());
        assert_eq!(old.parent(), new.parent());
        assert_eq!(old.depth(), new.depth());
        assert_eq!(old.placement(), new.placement());
        assert!(Arc::ptr_eq(old.content(), new.content()));
    }
}

fn picture() -> flui_painting::DisplayList {
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 40.0, 20.0),
        &Paint::fill(Color::RED),
    );
    canvas.finish()
}

// scene_apply_contract

/// A slot that names the node it sits in, or an ancestor of it, is refused:
/// the scene stays a tree.
pub(crate) fn a_slot_naming_its_own_ancestor_is_refused() {
    let (mut owner, leaves) = owner_with_leaves(1);
    let a = leaves[0];
    let mut writer = owner.writer();
    let b = writer.mint_node();
    writer.push(publish(b, empty()));
    writer.push(publish(a, slots(&[b])));
    owner.apply(SURFACE).expect("root -> a -> b");
    assert_eq!(owner.current().node(b).map(SceneNode::depth), Some(2));

    let before = owner.current().clone();
    // The root lets go of `a`, so the only slot naming it is under it.
    let root = owner.current().root();
    assert_eq!(
        apply(
            &mut owner,
            [publish(root, empty()), publish(b, slots(&[a]))]
        ),
        Err(SceneError::OwnAncestor(a))
    );
    assert_eq!(
        apply(&mut owner, [publish(b, slots(&[b]))]),
        Err(SceneError::OwnAncestor(b))
    );
    assert_eq!(
        apply(&mut owner, [publish(b, slots(&[root]))]),
        Err(SceneError::OwnAncestor(root))
    );
    assert_same_version(&before, owner.current());
}

/// An id whose node was retired, or whose patch was never applied, names
/// nothing again.
pub(crate) fn a_stale_generation_is_refused() {
    let (mut owner, leaves) = owner_with_leaves(2);
    let root = owner.current().root();
    let gone = leaves[1];
    apply(
        &mut owner,
        [
            publish(root, slots(&leaves[..1])),
            SceneOp::Retire { node: gone },
        ],
    )
    .expect("a detached node retires");
    assert!(owner.current().node(gone).is_none());
    let place = |node| SceneOp::Place {
        node,
        placement: Placement::ZERO,
    };
    assert_eq!(
        apply(&mut owner, [place(gone)]),
        Err(SceneError::StaleNode(gone))
    );

    // The retired slot is reused under its next generation; the old id stays
    // stale.
    let mut writer = owner.writer();
    let reused = writer.mint_node();
    writer.push(publish(reused, empty()));
    owner.apply(SURFACE).expect("a fresh id publishes");
    assert_eq!(
        apply(&mut owner, [place(gone)]),
        Err(SceneError::StaleNode(gone))
    );

    // An id minted for a patch that was never applied.
    let unapplied = owner.writer().mint_node();
    // The next patch opens without applying the one `unapplied` was minted for.
    assert_eq!(
        apply(&mut owner, [publish(unapplied, empty())]),
        Err(SceneError::StaleNode(unapplied))
    );
}

/// A node named by two slots, in one content or in two, is refused.
pub(crate) fn a_node_named_by_two_slots_is_refused() {
    let (mut owner, leaves) = owner_with_leaves(2);
    let (a, b) = (leaves[0], leaves[1]);
    let before = owner.current().clone();
    // `root` still names `a`, and `b` names it too.
    assert_eq!(
        apply(&mut owner, [publish(b, slots(&[a]))]),
        Err(SceneError::TwoParents(a))
    );
    // One content naming it twice.
    let mut writer = owner.writer();
    let c = writer.mint_node();
    writer.push(publish(c, empty()));
    writer.push(publish(b, slots(&[c, c])));
    assert_eq!(owner.apply(SURFACE), Err(SceneError::TwoParents(c)));
    assert_same_version(&before, owner.current());

    // Moving `a` under `b` while republishing the root without it is one
    // parent, in either order.
    let root = owner.current().root();
    apply(
        &mut owner,
        [publish(b, slots(&[a])), publish(root, slots(&[b]))],
    )
    .expect("a moves");
    assert_eq!(owner.current().node(a).and_then(SceneNode::parent), Some(b));
    assert_eq!(owner.current().node(a).map(SceneNode::depth), Some(2));
    apply(
        &mut owner,
        [publish(root, slots(&[a, b])), publish(b, empty())],
    )
    .expect("a moves back");
    assert_eq!(
        owner.current().node(a).and_then(SceneNode::parent),
        Some(root)
    );
    assert_eq!(owner.current().node(a).map(SceneNode::depth), Some(1));
}

/// A refused patch changes nothing: the operations before the refused one
/// are not applied, the publication is the one before, and the next patch
/// applies against the old version.
pub(crate) fn a_refused_patch_leaves_the_version_and_log_unchanged() {
    let (mut owner, leaves) = owner_with_leaves(3);
    let before = owner.current().clone();
    let published = owner.publication();
    let moved = Placement::try_new(Offset::new(5.0, 5.0)).expect("finite");
    let mut writer = owner.writer();
    let fresh = writer.mint_node();
    writer.push(SceneOp::Place {
        node: leaves[0],
        placement: moved,
    });
    writer.push(publish(fresh, empty()));
    writer.push(publish(leaves[1], slots(&[fresh])));
    writer.push(SceneOp::Retire { node: leaves[2] });
    assert_eq!(
        owner.apply(SURFACE),
        Err(SceneError::RetiredInUse(leaves[2]))
    );

    assert_same_version(&before, owner.current());
    assert!(owner.current().node(fresh).is_none());
    let after = owner.publication();
    assert_eq!(after.base, published.base);
    assert_eq!(after.scene.id(), published.scene.id());
    assert!(matches!(
        after.damage,
        DamageRegion::Full(FullReason::FirstFrame)
    ));

    // The refused patch's id is spent; the rest of the scene still applies.
    assert_eq!(
        apply(&mut owner, [publish(fresh, empty())]),
        Err(SceneError::StaleNode(fresh))
    );
    apply(
        &mut owner,
        [SceneOp::Place {
            node: leaves[0],
            placement: moved,
        }],
    )
    .expect("applies");
    assert_eq!(
        owner.current().node(leaves[0]).map(SceneNode::placement),
        Some(moved)
    );
    assert_eq!(
        before.node(leaves[0]).map(SceneNode::placement),
        Some(Placement::ZERO)
    );
}

/// An id minted but never published names no node yet.
pub(crate) fn an_id_never_published_is_unknown() {
    let (mut owner, _) = owner_with_leaves(1);
    let mut writer = owner.writer();
    let minted = writer.mint_node();
    writer.push(SceneOp::Place {
        node: minted,
        placement: Placement::ZERO,
    });
    assert_eq!(owner.apply(SURFACE), Err(SceneError::UnknownNode(minted)));
}

/// An id of another live owner is refused before its slot and generation
/// are read, though both roots sit at slot 0 with the first generation.
pub(crate) fn a_node_of_another_owner_is_refused() {
    let (mut owner, leaves) = owner_with_leaves(1);
    let (other, other_leaves) = owner_with_leaves(1);
    let root = owner.current().root();
    let foreign_root = other.current().root();
    assert_ne!(foreign_root, root);
    assert!(owner.current().node(foreign_root).is_none());
    let before = owner.current().clone();
    let published = owner.publication();

    let place = SceneOp::Place {
        node: foreign_root,
        placement: Placement::ZERO,
    };
    assert_eq!(
        apply(&mut owner, [place]),
        Err(SceneError::ForeignNode(foreign_root))
    );
    // As a slot: without the lineage it would read as this owner's root.
    assert_eq!(
        apply(&mut owner, [publish(leaves[0], slots(&[foreign_root]))]),
        Err(SceneError::ForeignNode(foreign_root))
    );
    assert_eq!(
        apply(&mut owner, [publish(other_leaves[0], empty())]),
        Err(SceneError::ForeignNode(other_leaves[0]))
    );
    assert_same_version(&before, owner.current());
    let after = owner.publication();
    assert_eq!(after.scene.id(), published.scene.id());
    assert_eq!(after.damage, published.damage);
}

/// The root cannot be retired, and a placement is finite by construction.
pub(crate) fn the_root_stays_and_placements_are_finite() {
    let (mut owner, _) = owner_with_leaves(1);
    let root = owner.current().root();
    assert_eq!(
        apply(&mut owner, [SceneOp::Retire { node: root }]),
        Err(SceneError::RetiredInUse(root))
    );
    assert!(Placement::try_new(Offset::new(f64::NAN, 0.0)).is_err());
    assert!(Placement::try_new(Offset::new(0.0, f64::INFINITY)).is_err());
}

// content_version_contract

/// A replaced owner starts its sequence again under a new lineage, so its
/// content versions never equal the old owner's.
pub(crate) fn a_replaced_owner_never_reports_unchanged() {
    let mut first = SceneOwner::new();
    let old = first.writer().next_content_version().expect("fresh");
    let mut second = SceneOwner::new();
    let new = second.writer().next_content_version().expect("fresh");
    assert_eq!(old.seq(), new.seq());
    assert_ne!(old, new);
    assert_ne!(first.lineage(), second.lineage());

    second.renew(ResyncReason::Rebuilt);
    let renewed = second.writer().next_content_version().expect("fresh");
    assert_eq!(renewed.seq(), new.seq());
    assert_ne!(renewed, new);
}

/// A spent sequence refuses with `VersionsExhausted`, never wraps, and
/// renewing the lineage starts over with a full publication.
pub(crate) fn an_exhausted_mint_renews_the_lineage_and_publishes_full() {
    let mut owner = SceneOwner::with_sequence_after(u64::MAX - 1);
    let root = owner.current().root();
    assert_eq!(owner.current().id().seq().get(), u64::MAX);
    assert_eq!(
        owner.writer().next_content_version(),
        Err(SceneError::VersionsExhausted)
    );

    let before = owner.current().clone();
    let mut writer = owner.writer();
    let child = writer.mint_node();
    writer.push(publish(child, empty()));
    writer.push(publish(root, slots(&[child])));
    assert_eq!(owner.apply(SURFACE), Err(SceneError::VersionsExhausted));
    assert_same_version(&before, owner.current());

    let lineage = owner.lineage();
    owner.renew(ResyncReason::Rebuilt);
    assert_ne!(owner.lineage(), lineage);
    let publication = owner.publication();
    assert_eq!(publication.base, None);
    assert!(matches!(
        publication.damage,
        DamageRegion::Full(FullReason::Resync(ResyncReason::Rebuilt))
    ));
    assert_eq!(publication.scene.len(), 1);
    assert_eq!(publication.scene.id().lineage(), owner.lineage());
    assert_ne!(owner.current().root(), root);

    // No id of the old lineage resolves, and the new one applies.
    let place = SceneOp::Place {
        node: root,
        placement: Placement::ZERO,
    };
    assert_eq!(
        apply(&mut owner, [place]),
        Err(SceneError::ForeignNode(root))
    );
    let new_root = owner.current().root();
    let mut writer = owner.writer();
    let child = writer.mint_node();
    writer.push(publish(child, empty()));
    writer.push(publish(new_root, slots(&[child])));
    owner.apply(SURFACE).expect("the renewed lineage applies");
    assert_eq!(owner.current().len(), 2);
}

// scene_indexes

/// Links, texture users and filtered nodes follow every publish and retire.
pub(crate) fn the_indexes_follow_publish_and_retire() {
    let link = LayerLink::new();
    let texture = TextureId::new(7);
    let mut canvas = Canvas::new();
    canvas.draw_texture(
        texture,
        Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
        None,
        FilterQuality::Low,
        1.0,
    );

    let (mut owner, leaves) = owner_with_leaves(2);
    let (a, b) = (leaves[0], leaves[1]);
    let mut writer = owner.writer();
    let version = writer.next_content_version().expect("fresh");
    let mut content = BoundaryContent::builder();
    content.push_effect(
        Layer::from(LeaderLayer::new(link, Size::new(10.0, 10.0))),
        owner_id(),
        0,
    );
    content.chunk(Arc::new(Chunk::new(
        owner_id(),
        0,
        version,
        canvas.finish(),
    )));
    content.pop_effect();
    content.push_effect(
        Layer::from(BackdropFilterLayer::new(
            ImageFilter::blur(4.0),
            BlendMode::SrcOver,
            Rect::from_xywh(0.0, 0.0, 9.0, 9.0),
        )),
        owner_id(),
        1,
    );
    writer.push(publish(a, Arc::new(content.finish())));
    let mut follower = BoundaryContent::builder();
    follower.push_effect(Layer::from(FollowerLayer::new(link)), owner_id(), 0);
    writer.push(publish(b, Arc::new(follower.finish())));
    owner.apply(SURFACE).expect("applies");

    let scene = owner.current();
    let leaders: Vec<_> = scene.leaders(link).collect();
    assert_eq!(leaders.len(), 1);
    assert_eq!(leaders[0].node, a);
    assert_eq!(
        scene
            .followers(link)
            .map(|address| address.node)
            .collect::<Vec<_>>(),
        vec![b]
    );
    let users: Vec<_> = scene.texture_users(texture).collect();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].node, a);
    assert_eq!(users[0].entry.get(), 1);
    assert_eq!(scene.filtered_nodes().collect::<Vec<_>>(), vec![a]);

    let indexed = owner.current().clone();
    let root = owner.current().root();
    apply(
        &mut owner,
        [
            publish(a, empty()),
            publish(root, slots(&[a])),
            SceneOp::Retire { node: b },
        ],
    )
    .expect("applies");
    let scene = owner.current();
    assert_eq!(scene.leaders(link).count(), 0);
    assert_eq!(scene.followers(link).count(), 0);
    assert_eq!(scene.texture_users(texture).count(), 0);
    assert_eq!(scene.filtered_nodes().count(), 0);
    // The version before keeps its indexes.
    assert_eq!(indexed.leaders(link).count(), 1);
    assert_eq!(indexed.texture_users(texture).count(), 1);
}

/// A version flattens to the layer tree it paints, unattached nodes left
/// out.
pub(crate) fn a_version_flattens_to_the_tree_it_paints() {
    let (mut owner, leaves) = owner_with_leaves(2);
    let (a, b) = (leaves[0], leaves[1]);
    let root = owner.current().root();
    let mut writer = owner.writer();
    let version = writer.next_content_version().expect("fresh");
    let mut content = BoundaryContent::builder();
    content.push_effect(Layer::from(OpacityLayer::new(0.5)), owner_id(), 0);
    content.chunk(Arc::new(Chunk::new(owner_id(), 0, version, picture())));
    content.slot(b);
    content.pop_effect();
    writer.push(publish(a, Arc::new(content.finish())));
    writer.push(publish(root, slots(&[a])));
    writer.push(SceneOp::Place {
        node: b,
        placement: Placement::try_new(Offset::new(3.0, 4.0)).expect("finite"),
    });
    owner.apply(SURFACE).expect("applies");
    assert_eq!(owner.current().node(b).map(SceneNode::depth), Some(2));

    let tree = owner.current().flatten();
    let kinds = |id| tree.get_layer(id).expect("in the tree").kind_name();
    let [a_offset] = tree.children(tree.root()).expect("root") else {
        panic!("the root names one slot");
    };
    assert_eq!(kinds(*a_offset), "Offset");
    let [opacity] = tree.children(*a_offset).expect("a") else {
        panic!("a holds one effect");
    };
    assert_eq!(kinds(*opacity), "Opacity");
    let [chunk, b_offset] = tree.children(*opacity).expect("opacity") else {
        panic!("the opacity holds the chunk and b's slot");
    };
    assert_eq!(kinds(*chunk), "Picture");
    match tree.get_layer(*b_offset) {
        Some(Layer::Offset(offset)) => assert_eq!(offset.offset(), Offset::new(3.0, 4.0)),
        other => panic!("b's slot is an offset, got {other:?}"),
    }
    assert_eq!(tree.len(), 5);
}

/// A version crosses to the raster thread by its auto traits.
pub(crate) fn a_version_is_send_and_sync() {
    fn shareable<T: Send + Sync>() {}
    shareable::<SceneVersion>();
}

// scene_table_cost

/// The fewest fanout-32 levels that index `slots` slots.
fn height(slots: usize) -> u64 {
    let mut height = 1;
    let mut capacity = 32usize;
    while capacity < slots {
        capacity *= 32;
        height += 1;
    }
    height
}

/// Table nodes `ops` copy out of the version before them.
fn copies_of(owner: &mut SceneOwner, ops: Vec<SceneOp>) -> u64 {
    let before = owner.table_copies();
    // Another holder of the version, as the raster side is.
    let _held = owner.current().clone();
    apply(owner, ops).expect("applies");
    owner.table_copies() - before
}

/// Places of `k` nodes `stride` slots apart.
fn places(leaves: &[SceneNodeId], k: usize, stride: usize) -> Vec<SceneOp> {
    let placement = Placement::try_new(Offset::new(1.0, 1.0)).expect("finite");
    (0..k)
        .map(|i| SceneOp::Place {
            node: leaves[i * stride],
            placement,
        })
        .collect()
}

/// An apply of `k` operations copies at most `k` paths of the table's height;
/// at one height the copies of the same operations do not depend on how many
/// nodes the scene holds, and operations in one leaf share one path.
pub(crate) fn an_apply_copies_the_patch_not_the_scene() {
    const K: usize = 8;
    let mut same_slots = Vec::new();
    for n in [100usize, 1_100, 30_000, 100_000] {
        let (mut owner, leaves) = owner_with_leaves(n);
        // The root takes slot 0 and the leaves the slots after it.
        let height = height(n + 1);
        let spread = copies_of(&mut owner, places(&leaves, K, n / K));
        assert!(
            spread <= K as u64 * height,
            "{n} nodes: {spread} copies over height {height}"
        );
        let dense = copies_of(&mut owner, places(&leaves[32..], K, 1));
        assert_eq!(
            dense, height,
            "{n} nodes: {K} places in one leaf copy one path"
        );
        same_slots.push((n, height, copies_of(&mut owner, places(&leaves, K, 12))));
    }
    let at = |n| {
        *same_slots
            .iter()
            .find(|(nodes, ..)| *nodes == n)
            .expect("measured")
    };
    let (_, height_small, copies_small) = at(1_100);
    let (_, height_large, copies_large) = at(30_000);
    assert_eq!(height_small, height_large);
    assert_eq!(copies_small, copies_large, "{same_slots:?}");
}

/// A parent republished with the slots it had, plus one, copies the same
/// table nodes whether it names 40 children or 1 000: both tables are two
/// levels high, and a child named before and after is not written.
pub(crate) fn a_republished_parent_costs_its_change_not_its_slots() {
    let mut copies = Vec::new();
    for n in [40usize, 1_000] {
        let (mut owner, leaves) = owner_with_leaves(n);
        assert_eq!(height(n + 1), 2);
        let root = owner.current().root();
        let unchanged = copies_of(&mut owner, vec![publish(root, slots(&leaves))]);
        let mut writer = owner.writer();
        let added = writer.mint_node();
        let mut named = leaves.clone();
        named.push(added);
        writer.push(publish(added, empty()));
        writer.push(publish(root, slots(&named)));
        let before = owner.table_copies();
        let _held = owner.current().clone();
        owner.apply(SURFACE).expect("applies");
        copies.push((n, unchanged, owner.table_copies() - before));
        assert_eq!(
            owner.current().node(added).and_then(SceneNode::parent),
            Some(root)
        );
    }
    assert_eq!(copies[0].1, copies[1].1, "{copies:?}");
    assert_eq!(copies[0].2, copies[1].2, "{copies:?}");
}

/// Content with a leader and a texture, recorded anew.
fn linked(owner: &mut SceneOwner, link: LayerLink, texture: TextureId) -> Arc<BoundaryContent> {
    let mut canvas = Canvas::new();
    canvas.draw_texture(
        texture,
        Rect::from_xywh(0.0, 0.0, 8.0, 8.0),
        None,
        FilterQuality::Low,
        1.0,
    );
    let version = owner.writer().next_content_version().expect("fresh");
    let mut content = BoundaryContent::builder();
    content.push_effect(
        Layer::from(LeaderLayer::new(link, Size::new(10.0, 10.0))),
        owner_id(),
        0,
    );
    content.chunk(Arc::new(Chunk::new(
        owner_id(),
        0,
        version,
        canvas.finish(),
    )));
    Arc::new(content.finish())
}

/// A republish whose link and texture entries are the old ones writes none
/// of the indexes: it copies what a republish of plain content copies.
pub(crate) fn a_repaint_with_the_same_links_leaves_the_indexes() {
    let link = LayerLink::new();
    let texture = TextureId::new(9);
    let (mut owner, leaves) = owner_with_leaves(2);
    let (a, b) = (leaves[0], leaves[1]);
    let first = linked(&mut owner, link, texture);
    apply(&mut owner, [publish(a, first)]).expect("applies");
    let plain = copies_of(&mut owner, vec![publish(b, empty())]);
    let repaint = linked(&mut owner, link, texture);
    let relinked = copies_of(&mut owner, vec![publish(a, repaint)]);
    assert_eq!(relinked, plain);
    let leaders: Vec<_> = owner.current().leaders(link).collect();
    assert_eq!(leaders.len(), 1);
    assert_eq!(leaders[0].node, a);
    assert_eq!(owner.current().texture_users(texture).count(), 1);
}

/// A new boundary inserted above a subtree of `n` nodes rewrites `n` depths
/// and its own.
pub(crate) fn wrapping_a_subtree_rewrites_its_depths() {
    for n in [10usize, 100] {
        let (mut owner, leaves) = owner_with_leaves(1);
        let top = leaves[0];
        let mut writer = owner.writer();
        let below: Vec<SceneNodeId> = (1..n).map(|_| writer.mint_node()).collect();
        for node in &below {
            writer.push(publish(*node, empty()));
        }
        writer.push(publish(top, slots(&below)));
        owner
            .apply(SURFACE)
            .expect("a subtree of n nodes under the root");

        let root = owner.current().root();
        let before = owner.depth_updates();
        let mut writer = owner.writer();
        let wrapper = writer.mint_node();
        writer.push(publish(wrapper, slots(&[top])));
        writer.push(publish(root, slots(&[wrapper])));
        owner.apply(SURFACE).expect("the wrapper takes the subtree");
        assert_eq!(owner.depth_updates() - before, n as u64 + 1, "{n} nodes");
        assert_eq!(
            owner.current().node(below[0]).map(SceneNode::depth),
            Some(3)
        );
    }
}
