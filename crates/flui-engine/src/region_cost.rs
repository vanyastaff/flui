//! How a partial frame redraws a region of several rects: one pass per rect,
//! or one pass under the region's bounding box (ADR-0304 §9).
//!
//! A pass costs what it records or replays of every chunk it reaches and the
//! pixels it clears and fills. One pass per rect redraws only the rects, but
//! draws a chunk that meets several of them once in each; the bounding box
//! draws every chunk it reaches once, the gap between the rects included. The
//! planner predicts both from one culled walk of the version under the box,
//! counting for each chunk it reaches its commands and the rects it meets,
//! and takes the cheaper:
//!
//! - per rect: `Σ commands · rects met + PIXEL_COST · Σ rect area + PASS_COST · rects`;
//! - bounding box: `Σ commands + PIXEL_COST · box area + PASS_COST`.
//!
//! A chunk meets at most every rect, so the rects cost at most
//! `(rects − 1) · C` more in commands than the box, where `C` is every
//! command of the version: when the box's extra pixels and the rects' extra
//! passes already outweigh that, the rects are taken without the walk
//! ([`shape`]).
//!
//! An effect costs only through the commands under it: what a pass pays
//! for each effect it reaches (a backdrop snapshot, a save-layer's
//! offscreen, an opacity group) is not weighed, so a region over many
//! effects is planned as if they were free, part of the hypothesis.
//!
//! Flutter's raster cache redraws one bounding rect of its damage, and
//! Chromium's compositor collapses damage to a few rects or their union; the
//! two weights are hypotheses fitted on lavapipe alone (the `raster_share`
//! bench's region rows and `a_region_is_redrawn_the_way_predicted_cheaper`),
//! owed a run on hardware (the `partial-frames-redrawn-by-their-damage-region`
//! row of `crates/flui-native/platform-parity.toml`).

use flui_foundation::geometry::Rect;
use flui_layer::{Region, SceneVersion};

use crate::layer_walk::{Item, SceneVisitor, Site, Step};

/// What a pixel a pass clears and fills costs, in the commands a pass
/// records or replays: a hypothesis fitted on lavapipe.
pub(crate) const PIXEL_COST: f64 = 0.15;

/// What a pass costs beyond its commands and pixels (its clear, scissor and
/// walk), in commands: a hypothesis fitted on lavapipe.
pub(crate) const PASS_COST: f64 = 500.0;

/// How a partial frame redraws a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegionShape {
    /// One pass per rect.
    Rects,
    /// One pass under the region's bounding box.
    Bounds,
}

/// What each shape of a region is predicted to cost.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RegionCost {
    /// One pass per rect.
    pub(crate) rects: f64,
    /// One pass under the bounding box.
    pub(crate) bounds: f64,
}

impl RegionCost {
    /// The cheaper shape; the rects on a tie, which write fewer pixels.
    pub(crate) fn shape(self) -> RegionShape {
        if self.bounds < self.rects {
            RegionShape::Bounds
        } else {
            RegionShape::Rects
        }
    }
}

/// How `region` of `version`, whose chunks hold `commands` commands in
/// all, is redrawn: the rects without a walk when the box cannot win,
/// otherwise the cheaper of [`predict`].
pub(crate) fn shape(version: &SceneVersion, region: &Region, commands: u64) -> RegionShape {
    let rects = region.rects().len();
    if rects <= 1 || box_cannot_win(region, commands) {
        return RegionShape::Rects;
    }
    predict(version, region).shape()
}

/// The commands of every chunk `version` holds, attached or not, summed
/// over its nodes, each content's own sum read rather than its chunks: what
/// bounds a chunk's share of either plan, at a cost that follows the
/// boundaries, not the chunks.
pub(crate) fn version_commands(version: &SceneVersion) -> u64 {
    version
        .nodes()
        .map(|node| node.content().commands())
        .fold(0_u64, u64::saturating_add)
}

/// Whether the box costs at least the rects whatever the chunks: the
/// rects' surplus in commands is at most `(rects − 1) · commands`, and the
/// box's surplus in pixels less the rects' extra passes already covers it.
fn box_cannot_win(region: &Region, commands: u64) -> bool {
    let extra_passes = region.rects().len().saturating_sub(1) as f64;
    let extra_pixels = region.bounds().area().saturating_sub(region.area()) as f64;
    PIXEL_COST * extra_pixels - PASS_COST * extra_passes >= extra_passes * commands as f64
}

/// Predicts what redrawing `region` of `version` costs either way, in one
/// walk culled by the region's bounding box.
pub(crate) fn predict(version: &SceneVersion, region: &Region) -> RegionCost {
    let bounds = region.bounds().to_rect();
    let mut rects = [Rect::ZERO; Region::MAX];
    for (slot, rect) in rects.iter_mut().zip(region.rects()) {
        *slot = rect.to_rect();
    }
    let rects = &rects[..region.rects().len()];
    let mut counter = Counter {
        cull: version.subtree_cull(bounds),
        rects,
        commands: 0.0,
        commands_per_pass: 0.0,
    };
    crate::layer_walk::walk_scene(
        flui_layer::SceneContent::Version(version),
        Some(bounds),
        &mut counter,
    );
    let passes = rects.len() as f64;
    RegionCost {
        rects: counter.commands_per_pass + PIXEL_COST * region.area() as f64 + PASS_COST * passes,
        bounds: counter.commands + PIXEL_COST * region.bounds().area() as f64 + PASS_COST,
    }
}

/// Sums the commands of the chunks the walk reaches, once and once per rect
/// each meets.
struct Counter<'v, 'r> {
    cull: flui_layer::SubtreeCull<'v>,
    rects: &'r [Rect<f64>],
    commands: f64,
    commands_per_pass: f64,
}

impl SceneVisitor for Counter<'_, '_> {
    fn enter(&mut self, site: Site<'_>, item: &Item<'_>) -> Step {
        if let (Site::Entry(_, at), Item::Chunk(chunk)) = (site, item) {
            let commands = chunk.commands().len() as f64;
            let met = self.cull.rects_met_by_chunk(at, self.rects) as f64;
            self.commands += commands;
            self.commands_per_pass += commands * met;
        }
        Step::Descend
    }

    fn exit(&mut self, _site: Site<'_>, _item: &Item<'_>) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_foundation::RenderId;
    use flui_foundation::geometry::Rect;
    use flui_layer::{
        BoundaryContent, Chunk, DamageRect, DamageRegion, Region, Scene, SceneOp, SceneOwner,
        SurfaceKey,
    };
    use flui_painting::styling::Color;
    use flui_painting::{Canvas, paint::Paint};

    use crate::frame_protocol::FrameProtocol;

    /// A version whose root draws `root` (one chunk of that many 4×4
    /// rects, spread over the surface) and then a chunk per cell of a
    /// `cells` × `cells` grid, each a filled cell and its border.
    fn version(side: u32, root: usize, cells: u32) -> flui_layer::SceneVersion {
        let mut owner = SceneOwner::new();
        let mut writer = owner.writer();
        let root_node = writer.current().root();
        let mut content = BoundaryContent::builder();
        let chunk = |canvas: Canvas, index: usize, writer: &mut flui_layer::SceneWriter<'_>| {
            let version = writer.next_content_version().expect("a content version");
            Arc::new(Chunk::new(
                RenderId::new(index + 2),
                0,
                version,
                canvas.finish(),
            ))
        };
        if root > 0 {
            let mut canvas = Canvas::new();
            let step = f64::from(side) / (root as f64).sqrt();
            let across = (f64::from(side) / step) as usize;
            for index in 0..root {
                let (x, y) = (
                    (index % across) as f64 * step,
                    (index / across) as f64 * step,
                );
                canvas.draw_rect(Rect::from_xywh(x, y, 4.0, 4.0), &Paint::fill(Color::BLUE));
            }
            content.chunk(chunk(canvas, 0, &mut writer));
        }
        let cell = f64::from(side) / f64::from(cells.max(1));
        for index in 0..(cells * cells) as usize {
            let (x, y) = (
                (index as u32 % cells) as f64 * cell,
                (index as u32 / cells) as f64 * cell,
            );
            let mut canvas = Canvas::new();
            canvas.draw_rect(Rect::from_xywh(x, y, cell, cell), &Paint::fill(Color::RED));
            canvas.draw_rect(
                Rect::from_xywh(x + 1.0, y + 1.0, cell - 2.0, cell - 2.0),
                &Paint::fill(Color::GREEN),
            );
            content.chunk(chunk(canvas, index + 1, &mut writer));
        }
        writer.push(SceneOp::Publish {
            node: root_node,
            content: Arc::new(content.finish()),
        });
        owner
            .apply(SurfaceKey {
                size: (side, side),
                generation: flui_foundation::SurfaceGeneration::ZERO,
            })
            .expect("the scene applies");
        owner.current().clone()
    }

    fn square(x: u32, y: u32, side: u32) -> DamageRect {
        DamageRect::try_enclosing(
            Rect::from_xywh(f64::from(x), f64::from(y), f64::from(side), f64::from(side)),
            (4096, 4096),
        )
        .expect("finite")
        .expect("on the surface")
    }

    /// The rects a frame owing `rects` over `version` redraws, as the frame
    /// protocol plans them.
    fn planned(version: &flui_layer::SceneVersion, size: u32, rects: &[DamageRect]) -> usize {
        let mut region = Region::new(rects[0]);
        for &rect in &rects[1..] {
            region.insert(rect);
        }
        let mut frame = FrameProtocol::new();
        frame.presented();
        frame.apply_damage(DamageRegion::Region(region));
        frame.include_backdrop_dependencies(&Scene::of_version(version.clone()), (size, size));
        frame.owed_region().expect("a partial frame").rects().len()
    }

    /// Each case is a measured one (lavapipe, the `raster_share` bench and
    /// `a_heavy_chunk_redraws_a_region_under_its_bounding_box`): eight small rects over one chunk of 20 000 rects redraw faster
    /// as their bounding box, which replays the chunk once, than in eight
    /// passes that replay it in each; two rects in opposite corners of a grid
    /// of small chunks redraw faster in two passes than under a box that
    /// covers nearly the whole surface; the bound skips the walk of every
    /// region but the one over the large chunk. Each
    /// fails with the plan fixed to either shape; a bound that skipped a box
    /// the walk takes, or that never skipped, fails too.
    #[test]
    fn a_region_is_redrawn_the_way_predicted_cheaper() {
        let eight: Vec<DamageRect> = (0..8)
            .map(|i| square(16 + 124 * (i % 4), if i < 4 { 16 } else { 380 }, 18))
            .collect();
        let far = [square(20, 20, 22), square(1160, 1160, 22)];
        // (case, version, surface side, rects, passes the frame redraws in,
        // whether the bound skips the walk)
        let cases = [
            (
                "eight small rects over one large chunk",
                version(512, 20_000, 0),
                512,
                eight.clone(),
                1,
                false,
            ),
            (
                "eight small rects over nothing heavy",
                version(512, 0, 8),
                512,
                eight,
                8,
                true,
            ),
            (
                "two far rects over a grid of small chunks",
                version(1200, 0, 60),
                1200,
                far.to_vec(),
                2,
                true,
            ),
        ];
        for (case, version, side, rects, passes, skipped) in cases {
            assert_eq!(planned(&version, side, &rects), passes, "{case}");
            let mut region = Region::new(rects[0]);
            for &rect in &rects[1..] {
                region.insert(rect);
            }
            let commands = super::version_commands(&version);
            assert_eq!(
                super::box_cannot_win(&region, commands),
                skipped,
                "{case}: whether the bound skips the walk"
            );
            if skipped {
                assert_eq!(
                    super::predict(&version, &region).shape(),
                    super::RegionShape::Rects,
                    "{case}: the bound skips only a box the walk would not take"
                );
            }
        }
    }
}
