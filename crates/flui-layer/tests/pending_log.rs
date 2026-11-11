//! The owner publishes a version against the version the raster side last
//! acknowledged, and its damage is every change since: folded, never lost
//! (ADR-0304 §7).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::{Offset, Rect};
use flui_foundation::panic::commit;
use flui_foundation::{RenderId, SurfaceGeneration};
use flui_layer::{
    AnnotatedRegionLayer, BoundaryContent, Chunk, DamageRect, DamageRegion, FullReason, Layer,
    Placement, ResyncReason, SceneNodeId, SceneOp, SceneOwner, SceneVersionId, SurfaceKey,
};
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

const SURFACE: SurfaceKey = SurfaceKey {
    size: (800, 600),
    generation: SurfaceGeneration::ZERO,
};

const RESIZED: SurfaceKey = SurfaceKey {
    size: (1024, 768),
    generation: SurfaceGeneration::ZERO,
};

/// An owner whose root holds `n` leaves, each a 10 x 10 chunk; the raster
/// side has acknowledged that version.
fn acked_owner(n: usize) -> (SceneOwner, Vec<SceneNodeId>) {
    let mut owner = SceneOwner::new();
    let mut writer = owner.writer();
    let root = writer.current().root();
    let mut leaves = Vec::new();
    let mut root_content = BoundaryContent::builder();
    for index in 0..n {
        let leaf = writer.mint_node();
        let version = writer.next_content_version().expect("a fresh sequence");
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
            &Paint::fill(Color::RED),
        );
        let mut content = BoundaryContent::builder();
        content.chunk(Arc::new(Chunk::new(
            RenderId::new(index + 1),
            0,
            version,
            canvas.finish(),
        )));
        writer.push(SceneOp::Publish {
            node: leaf,
            content: Arc::new(content.finish()),
        });
        root_content.slot(leaf);
        leaves.push(leaf);
    }
    writer.push(SceneOp::Publish {
        node: root,
        content: Arc::new(root_content.finish()),
    });
    let id = owner
        .apply(SURFACE)
        .expect("a well-formed patch applies")
        .id;
    drop(owner.completed(Some(id)));
    (owner, leaves)
}

/// Moves `leaf` to `(x, y)` for `surface` and returns the version.
fn move_to(
    owner: &mut SceneOwner,
    leaf: SceneNodeId,
    x: f64,
    y: f64,
    surface: SurfaceKey,
) -> SceneVersionId {
    let mut writer = owner.writer();
    writer.push(SceneOp::Place {
        node: leaf,
        placement: Placement::try_new(Offset::new(x, y)).expect("a finite offset"),
    });
    owner.apply(surface).expect("a move applies").id
}

/// Whether one rect of the publication's damage covers the 10 x 10 leaf at
/// `(x, y)`.
fn covers(damage: DamageRegion, x: f64, y: f64) -> bool {
    let DamageRegion::Region(region) = damage else {
        panic!("expected a region, got {damage:?}");
    };
    let leaf = DamageRect::try_covering(Rect::from_xywh(x, y, 10.0, 10.0), SURFACE.size)
        .expect("finite")
        .expect("on the surface");
    region.rects().iter().any(|outer| {
        outer.left() <= leaf.left()
            && outer.top() <= leaf.top()
            && outer.right() >= leaf.right()
            && outer.bottom() >= leaf.bottom()
    })
}

// pending_log_contract

/// A seventeenth unacknowledged version folds the two oldest into one with
/// the newer id: the damage still covers the folded changes, and
/// acknowledging the older of the two drops nothing.
pub(crate) fn a_full_log_folds_its_two_oldest_entries() {
    let (mut owner, leaves) = acked_owner(1);
    let base = owner.publication().base;
    let ids: Vec<SceneVersionId> = (0..17)
        .map(|step| {
            move_to(
                &mut owner,
                leaves[0],
                40.0 * f64::from(step),
                100.0,
                SURFACE,
            )
        })
        .collect();
    assert_eq!(owner.pending_len(), 16);
    assert_eq!(owner.pending_log_folds(), 1);

    let publication = owner.publication();
    assert_eq!(publication.base, base);
    for step in 0..17 {
        assert!(
            covers(publication.damage, 40.0 * f64::from(step), 100.0),
            "move {step} is in the damage"
        );
    }
    // The first move is folded under the second's id: its own
    // acknowledgement is lost, so the entry waits for the second.
    drop(owner.completed(Some(ids[0])));
    assert_eq!(owner.pending_len(), 16);
    assert!(covers(owner.publication().damage, 0.0, 100.0));
    drop(owner.completed(Some(ids[1])));
    assert_eq!(owner.pending_len(), 15);
    assert!(!covers(owner.publication().damage, 0.0, 100.0));
}

/// Damage from an older base covers every change since a newer one, and an
/// acknowledgement moves the base forward only.
pub(crate) fn damage_from_an_older_base_covers_the_newer_one() {
    let (mut owner, leaves) = acked_owner(2);
    let first = move_to(&mut owner, leaves[0], 100.0, 100.0, SURFACE);
    let second = move_to(&mut owner, leaves[1], 300.0, 300.0, SURFACE);
    let older = owner.publication();
    assert!(covers(older.damage, 100.0, 100.0));
    assert!(covers(older.damage, 300.0, 300.0));

    drop(owner.completed(Some(first)));
    let newer = owner.publication();
    assert_eq!(newer.base, Some(first));
    assert_eq!(newer.scene.id(), second);
    assert!(covers(newer.damage, 300.0, 300.0));
    assert!(!covers(newer.damage, 100.0, 100.0));

    // An older acknowledgement arriving late does not move the base back.
    drop(owner.completed(older.base));
    assert_eq!(owner.publication().base, Some(first));

    drop(owner.completed(Some(second)));
    let caught_up = owner.publication();
    assert_eq!(caught_up.base, Some(second));
    assert_eq!(caught_up.damage, DamageRegion::Unchanged);
}

/// A renewed lineage publishes full with no base, and an acknowledgement of
/// another lineage, the old one's or another owner's, changes nothing.
pub(crate) fn a_base_from_another_lineage_renders_full() {
    let (mut owner, leaves) = acked_owner(1);
    let old = move_to(&mut owner, leaves[0], 100.0, 100.0, SURFACE);
    drop(owner.renew(ResyncReason::RasterReset));
    // The new lineage's sequence passes the old id's, so only the lineage
    // tells them apart.
    for _ in 0..5 {
        let root = owner.current().root();
        owner.writer().push(SceneOp::Publish {
            node: root,
            content: Arc::new(BoundaryContent::default()),
        });
        owner.apply(SURFACE).expect("the renewed lineage applies");
    }
    assert!(owner.current().id().seq() > old.seq());
    drop(owner.completed(Some(old)));
    let renewed = owner.publication();
    assert_eq!(renewed.base, None);
    assert_eq!(
        renewed.damage,
        DamageRegion::Full(FullReason::Resync(ResyncReason::RasterReset))
    );

    let (mut other, other_leaves) = acked_owner(1);
    let foreign = move_to(&mut other, other_leaves[0], 50.0, 50.0, SURFACE);
    drop(owner.completed(Some(foreign)));
    assert_eq!(owner.publication().base, None);

    // The renewed lineage's own acknowledgement restores a base.
    let own = owner.current().id();
    drop(owner.completed(Some(own)));
    let publication = owner.publication();
    assert_eq!(publication.base, Some(own));
    assert_eq!(publication.damage, DamageRegion::Unchanged);
}

/// A completion that never arrives costs a larger region at the next base,
/// never a change left out, and `None` (the raster side holds nothing)
/// changes nothing.
pub(crate) fn a_lost_completion_over_reports_and_never_under_reports() {
    let (mut owner, leaves) = acked_owner(3);
    let base = owner.publication().base;
    let _lost = move_to(&mut owner, leaves[0], 100.0, 100.0, SURFACE);
    let second = move_to(&mut owner, leaves[1], 200.0, 200.0, SURFACE);
    let third = move_to(&mut owner, leaves[2], 300.0, 300.0, SURFACE);

    drop(owner.completed(None));
    let publication = owner.publication();
    assert_eq!(publication.base, base);
    for (x, y) in [(100.0, 100.0), (200.0, 200.0), (300.0, 300.0)] {
        assert!(covers(publication.damage, x, y));
    }

    drop(owner.completed(Some(second)));
    let publication = owner.publication();
    assert_eq!(publication.base, Some(second));
    assert!(covers(publication.damage, 300.0, 300.0));
    assert!(!covers(publication.damage, 100.0, 100.0));

    drop(owner.completed(Some(third)));
    assert_eq!(owner.pending_len(), 0);
    assert_eq!(owner.publication().damage, DamageRegion::Unchanged);
}

/// A log that spans two surface keys publishes full until the version for
/// the new surface is acknowledged.
pub(crate) fn a_surface_change_in_the_log_publishes_full() {
    let (mut owner, leaves) = acked_owner(1);
    move_to(&mut owner, leaves[0], 100.0, 100.0, SURFACE);
    move_to(&mut owner, leaves[0], 200.0, 200.0, RESIZED);
    // Folding the version applied for the new surface keeps its full damage.
    for step in 0..20 {
        move_to(
            &mut owner,
            leaves[0],
            10.0 * f64::from(step),
            400.0,
            RESIZED,
        );
    }
    assert_eq!(owner.pending_len(), 16);
    assert_eq!(
        owner.publication().damage,
        DamageRegion::Full(FullReason::SurfaceChanged)
    );

    drop(owner.completed(Some(owner.current().id())));
    move_to(&mut owner, leaves[0], 300.0, 300.0, RESIZED);
    assert!(covers(owner.publication().damage, 300.0, 300.0));
}

thread_local! {
    /// The owner the probe's destructor reaches for.
    static OWNER: RefCell<Option<Rc<RefCell<SceneOwner>>>> = const { RefCell::new(None) };
    /// Whether the probe dropped, and whether it found the owner free.
    static DROPPED: Cell<Option<bool>> = const { Cell::new(None) };
}

/// An annotation value whose destructor calls the owner again.
#[derive(Debug)]
struct ReentryProbe;

impl Drop for ReentryProbe {
    fn drop(&mut self) {
        let free = OWNER.with_borrow(|owner| {
            owner
                .as_ref()
                .is_some_and(|owner| owner.try_borrow_mut().is_ok())
        });
        DROPPED.set(Some(free));
    }
}

/// The content an acknowledgement releases drops after the owner's borrow
/// An owner in a cell whose pending log holds a replaced content with a
/// probe in it, every apply's release handed to an `Outbox`; returns the
/// version that replaced it.
fn probed_owner() -> (Rc<RefCell<SceneOwner>>, SceneVersionId) {
    let (owner, leaves) = acked_owner(1);
    let owner = Rc::new(RefCell::new(owner));
    OWNER.set(Some(Rc::clone(&owner)));
    DROPPED.set(None);
    let mut annotated = BoundaryContent::builder();
    annotated.push_effect(
        Layer::AnnotatedRegion(AnnotatedRegionLayer::new(
            Rect::from_xywh(0.0, 0.0, 10.0, 10.0),
            Arc::new(ReentryProbe),
        )),
        RenderId::new(1),
        0,
    );
    annotated.pop_effect();
    let mut replaced = None;
    for content in [annotated.finish(), BoundaryContent::default()] {
        commit(&owner, |owner, outbox| {
            owner.writer().push(SceneOp::Publish {
                node: leaves[0],
                content: Arc::new(content),
            });
            let applied = owner.apply(SURFACE).expect("applies");
            outbox.release(applied.released);
            replaced = Some(applied.id);
        });
    }
    // The log keeps the replaced content until the raster side moves past it.
    assert_eq!(DROPPED.get(), None);
    (owner, replaced.expect("two applies ran"))
}

/// The content an acknowledgement releases drops after the owner's borrow
/// ends: a destructor that calls the owner again finds it free.
pub(crate) fn a_released_content_drops_after_the_owner_is_released() {
    let (owner, replaced) = probed_owner();
    commit(&owner, |owner, outbox| {
        outbox.release(owner.completed(Some(replaced)));
    });
    OWNER.set(None);
    assert_eq!(DROPPED.get(), Some(true));
}

/// A raster side that never acknowledges: the log stays at sixteen entries,
/// and the content a fold lets go of drops after the owner's borrow ends.
pub(crate) fn a_fold_releases_its_contents_after_the_owner_is_released() {
    let (owner, _) = probed_owner();
    let root = owner.borrow().current().root();
    for _ in 0..40 {
        commit(&owner, |owner, outbox| {
            owner.writer().push(SceneOp::Place {
                node: root,
                placement: Placement::ZERO,
            });
            outbox.release(owner.apply(SURFACE).expect("applies").released);
        });
        assert!(owner.borrow().pending_len() <= 16);
    }
    assert_eq!(owner.borrow().pending_log_folds(), 26);
    OWNER.set(None);
    assert_eq!(DROPPED.get(), Some(true));
}
