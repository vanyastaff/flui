//! The keyline model a carousel scrolls through.
//!
//! Items lie end to end in a strip, every one at the layout's large extent `L` with the item
//! spacing between them: item `i`'s centre is `i·(L + spacing) + L/2 − offset` in that strip
//! (its *unadjusted* position). A keyline is a point of the strip with the size an item whose
//! centre is there is masked to and the place in the viewport that centre is drawn. An item
//! between two keylines interpolates both, so it shrinks and slides as it passes from a large
//! keyline to a small one while its content keeps its full extent under the mask.
//!
//! One list of keylines is the layout at rest. Near either end of the strip that list would
//! leave the first or last item small forever, so the strategy also keeps *steps*: the rest
//! list with one keyline after another moved across the large ones, until the first item rests
//! on a large keyline at offset zero and the last one at the largest offset. The keylines for an
//! offset interpolate between the two steps the offset falls between.
//!
//! The step construction and the arrangement search follow Material's published carousel
//! model (Compose's `Strategy` and `Arrangement`); the numbers are logical pixels, the sizes are
//! checked by [`CarouselLayout`]'s constructors, and every list is capped at
//! [`MAX_VISIBLE`] keylines so a tiny extent over a huge viewport stays bounded.

use super::layout::{CarouselLayout, LayoutKind, MAX_SMALL, MIN_SMALL};

/// The size of the off-screen keylines at either end, logical pixels: an item entering or
/// leaving grows from or shrinks to it.
pub(crate) const ANCHOR_EXTENT: f64 = 10.0;

/// A medium item no larger than this share of the large one, so the change between them reads
/// as motion.
const MEDIUM_LARGE_RATIO: f64 = 0.85;

/// The most items one keyline list shows at once.
pub(crate) const MAX_VISIBLE: usize = 256;

/// A point of the strip: what an item whose centre is there looks like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Keyline {
    /// The extent the item is masked to.
    pub(crate) size: f64,
    /// Where the item's centre is drawn, from the viewport's leading edge.
    pub(crate) offset: f64,
    /// Where the item's centre is in the end-to-end strip, from the viewport's leading edge.
    pub(crate) unadjusted: f64,
    /// An item here is fully shown.
    pub(crate) focal: bool,
    /// One of the off-screen ends, which never moves between steps.
    pub(crate) anchor: bool,
    /// How far an item here reaches past the viewport's edge.
    pub(crate) cutoff: f64,
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}

fn lerp_keyline(a: &Keyline, b: &Keyline, t: f64) -> Keyline {
    let (near, _) = if t < 0.5 { (a, b) } else { (b, a) };
    Keyline {
        size: lerp(a.size, b.size, t),
        offset: lerp(a.offset, b.offset, t),
        unadjusted: lerp(a.unadjusted, b.unadjusted, t),
        focal: near.focal,
        anchor: near.anchor,
        cutoff: lerp(a.cutoff, b.cutoff, t),
    }
}

/// `value` mapped from `[in_min, in_max]` onto `[out_min, out_max]`, held at the ends.
fn lerp_range(out_min: f64, out_max: f64, in_min: f64, in_max: f64, value: f64) -> f64 {
    if value <= in_min {
        out_min
    } else if value >= in_max {
        out_max
    } else {
        lerp(out_min, out_max, (value - in_min) / (in_max - in_min))
    }
}

/// A keyline before its offsets are placed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Draft {
    size: f64,
    anchor: bool,
}

/// Keylines in strip order, with at least one focal keyline between the two anchors.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Keylines {
    lines: Vec<Keyline>,
    pivot: usize,
    first_focal: usize,
    last_focal: usize,
}

impl Keylines {
    /// Keylines from `drafts`, placed outward from the one at `pivot`, whose centre is drawn at
    /// `pivot_offset`. The focal keylines are the first run of the largest non-anchor size.
    fn with_pivot(
        drafts: &[Draft],
        pivot: usize,
        pivot_offset: f64,
        viewport: f64,
        spacing: f64,
    ) -> Self {
        let mut first_focal = 0;
        let mut focal_size = 0.0;
        for (index, draft) in drafts.iter().enumerate() {
            if !draft.anchor && draft.size > focal_size {
                first_focal = index;
                focal_size = draft.size;
            }
        }
        let mut last_focal = first_focal;
        while last_focal + 1 < drafts.len() && drafts[last_focal + 1].size == focal_size {
            last_focal += 1;
        }
        let cut_start =
            |size: f64, offset: f64| offset - size / 2.0 < 0.0 && offset + size / 2.0 > 0.0;
        let cut_end = |size: f64, offset: f64| {
            offset - size / 2.0 < viewport && offset + size / 2.0 > viewport
        };
        let cutoff = |size: f64, offset: f64| {
            if cut_start(size, offset) {
                (offset - size / 2.0).abs()
            } else if cut_end(size, offset) {
                offset + size / 2.0 - viewport
            } else {
                0.0
            }
        };
        let keyline = |index: usize, offset: f64, unadjusted: f64| Keyline {
            size: drafts[index].size,
            offset,
            unadjusted,
            focal: (first_focal..=last_focal).contains(&index),
            anchor: drafts[index].anchor,
            cutoff: cutoff(drafts[index].size, offset),
        };
        let mut lines = Vec::with_capacity(drafts.len());
        lines.push(keyline(pivot, pivot_offset, pivot_offset));
        let mut offset = pivot_offset - focal_size / 2.0 - spacing;
        let mut unadjusted = offset;
        for index in (0..pivot).rev() {
            let size = drafts[index].size;
            lines.push(keyline(
                index,
                offset - size / 2.0,
                unadjusted - focal_size / 2.0,
            ));
            offset -= size + spacing;
            unadjusted -= focal_size + spacing;
        }
        lines.reverse();
        let mut offset = pivot_offset + focal_size / 2.0 + spacing;
        let mut unadjusted = offset;
        for (index, draft) in drafts.iter().enumerate().skip(pivot + 1) {
            let size = draft.size;
            lines.push(keyline(
                index,
                offset + size / 2.0,
                unadjusted + focal_size / 2.0,
            ));
            offset += size + spacing;
            unadjusted += focal_size + spacing;
        }
        Self {
            lines,
            pivot,
            first_focal,
            last_focal,
        }
    }

    fn drafts(&self) -> Vec<Draft> {
        self.lines
            .iter()
            .map(|line| Draft {
                size: line.size,
                anchor: line.anchor,
            })
            .collect()
    }

    fn first(&self) -> &Keyline {
        &self.lines[0]
    }

    fn last(&self) -> &Keyline {
        &self.lines[self.lines.len() - 1]
    }

    fn first_focal(&self) -> &Keyline {
        &self.lines[self.first_focal]
    }

    fn last_focal(&self) -> &Keyline {
        &self.lines[self.last_focal]
    }

    fn focal_count(&self) -> usize {
        self.last_focal - self.first_focal + 1
    }

    fn first_non_anchor(&self) -> usize {
        self.lines.iter().position(|line| !line.anchor).unwrap_or(0)
    }

    fn last_non_anchor(&self) -> usize {
        self.lines
            .iter()
            .rposition(|line| !line.anchor)
            .unwrap_or(self.lines.len() - 1)
    }

    /// The first focal item starts inside the viewport and nothing but an anchor is before it.
    fn first_focal_at_start(&self) -> bool {
        let focal = self.first_focal();
        focal.offset - focal.size / 2.0 >= 0.0 && self.first_focal == self.first_non_anchor()
    }

    /// The last focal item ends inside the viewport and nothing but an anchor is after it.
    fn last_focal_at_end(&self, viewport: f64) -> bool {
        let focal = self.last_focal();
        focal.offset + focal.size / 2.0 <= viewport && self.last_focal == self.last_non_anchor()
    }

    fn first_index_after_focal_with_size(&self, size: f64) -> usize {
        (self.last_focal..self.lines.len())
            .find(|&index| self.lines[index].size == size)
            .unwrap_or(self.lines.len() - 1)
    }

    fn last_index_before_focal_with_size(&self, size: f64) -> usize {
        (0..self.first_focal)
            .rev()
            .find(|&index| self.lines[index].size == size)
            .unwrap_or(0)
    }

    /// These keylines with the one at `from` moved to `to`, shifted so the moved keyline's
    /// space is taken by its neighbours.
    fn moved(&self, from: usize, to: usize, viewport: f64, spacing: f64) -> Self {
        let direction = if from > to { 1.0 } else { -1.0 };
        let moving = &self.lines[from];
        let delta = (moving.size - moving.cutoff + spacing) * direction;
        let pivot = if from > to {
            self.pivot + 1
        } else {
            self.pivot.saturating_sub(1)
        };
        let mut drafts = self.drafts();
        let draft = drafts.remove(from);
        drafts.insert(to, draft);
        Self::with_pivot(
            &drafts,
            pivot,
            self.lines[self.pivot].offset + delta,
            viewport,
            spacing,
        )
    }

    fn lerp(&self, other: &Self, t: f64) -> Self {
        let (near, _) = if t < 0.5 {
            (self, other)
        } else {
            (other, self)
        };
        Self {
            lines: self
                .lines
                .iter()
                .zip(&other.lines)
                .map(|(a, b)| lerp_keyline(a, b, t))
                .collect(),
            pivot: near.pivot,
            first_focal: near.first_focal,
            last_focal: near.last_focal,
        }
    }

    /// The last keyline whose unadjusted position is before `unadjusted`, else the first.
    fn before(&self, unadjusted: f64) -> &Keyline {
        self.lines
            .iter()
            .rev()
            .find(|line| line.unadjusted < unadjusted)
            .unwrap_or_else(|| self.first())
    }

    /// The first keyline whose unadjusted position is at or after `unadjusted`, else the last.
    fn after(&self, unadjusted: f64) -> &Keyline {
        self.lines
            .iter()
            .find(|line| line.unadjusted >= unadjusted)
            .unwrap_or_else(|| self.last())
    }

    /// The strip span, unadjusted, in which an item is drawn at all.
    pub(crate) fn strip_span(&self) -> (f64, f64) {
        (self.first().unadjusted, self.last().unadjusted)
    }
}

/// Small, medium and large counts and sizes that fill a viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Arrangement {
    priority: usize,
    small: f64,
    small_count: usize,
    medium: f64,
    medium_count: usize,
    large: f64,
    large_count: usize,
}

/// What [`Arrangement::search`] aims for.
#[derive(Debug, Clone, Copy)]
struct Targets {
    available: f64,
    spacing: f64,
    small: f64,
    min_small: f64,
    max_small: f64,
    medium: f64,
    large: f64,
}

impl Arrangement {
    fn count(&self) -> usize {
        self.large_count + self.medium_count + self.small_count
    }

    fn valid(&self) -> bool {
        if self.large <= 0.0 || !self.large.is_finite() {
            return false;
        }
        if self.large_count > 0 && self.small_count > 0 && self.medium_count > 0 {
            self.large > self.medium && self.medium > self.small
        } else if self.large_count > 0 && self.small_count > 0 {
            self.large > self.small
        } else {
            true
        }
    }

    fn cost(&self, target_large: f64) -> f64 {
        if self.valid() {
            (target_large - self.large).abs() * self.priority as f64
        } else {
            f64::MAX
        }
    }

    /// The counts' arrangement closest to the large target, earlier counts winning ties.
    fn search(
        targets: Targets,
        small_counts: &[usize],
        medium_counts: &[usize],
        large_counts: &[usize],
    ) -> Option<Self> {
        let mut best: Option<Self> = None;
        let mut priority = 1;
        for &large_count in large_counts {
            for &medium_count in medium_counts {
                for &small_count in small_counts {
                    let candidate =
                        Self::fit(targets, priority, small_count, medium_count, large_count);
                    let cost = candidate.cost(targets.large);
                    if best.is_none_or(|best| cost < best.cost(targets.large)) {
                        best = Some(candidate);
                        if cost == 0.0 {
                            return best;
                        }
                    }
                    priority += 1;
                }
            }
        }
        best.filter(Self::valid)
    }

    fn fit(
        targets: Targets,
        priority: usize,
        small_count: usize,
        medium_count: usize,
        large_count: usize,
    ) -> Self {
        let total = (large_count + medium_count + small_count) as f64;
        let available = targets.available - (total - 1.0).max(0.0) * targets.spacing;
        let mut small = targets.small.clamp(targets.min_small, targets.max_small);
        let taken = targets.large * large_count as f64
            + targets.medium * medium_count as f64
            + small * small_count as f64;
        let delta = available - taken;
        if small_count > 0 {
            let share = delta / small_count as f64;
            if delta > 0.0 {
                small += share.min(targets.max_small - small);
            } else if delta < 0.0 {
                small += share.max(targets.min_small - small);
            }
        } else {
            small = 0.0;
        }
        let (small_n, medium_n, large_n) =
            (small_count as f64, medium_count as f64, large_count as f64);
        let mut large =
            (available - (small_n + medium_n / 2.0) * small) / (large_n + medium_n / 2.0);
        let mut medium = f64::midpoint(large, small);
        if medium_count > 0 && large != targets.large {
            let adjustment = (targets.large - large) * large_n;
            let flex = medium * 0.1 * medium_n;
            let distribute = adjustment.abs().min(flex);
            if adjustment > 0.0 {
                medium -= distribute / medium_n;
                large += distribute / large_n;
            } else {
                medium += distribute / medium_n;
                large -= distribute / large_n;
            }
        }
        Self {
            priority,
            small,
            small_count,
            medium,
            medium_count,
            large,
            large_count,
        }
    }

    /// Anchor, the large items, the medium, the small, anchor.
    fn start_aligned(&self, viewport: f64, spacing: f64) -> Keylines {
        let mut drafts = vec![Draft {
            size: ANCHOR_EXTENT,
            anchor: true,
        }];
        let mut push = |size: f64, count: usize| {
            drafts.extend((0..count).map(|_| Draft {
                size,
                anchor: false,
            }));
        };
        push(self.large, self.large_count);
        push(self.medium, self.medium_count);
        push(self.small, self.small_count);
        drafts.push(Draft {
            size: ANCHOR_EXTENT,
            anchor: true,
        });
        Keylines::with_pivot(&drafts, 1, self.large / 2.0, viewport, spacing)
    }
}

fn large_counts(available: f64, large: f64, min_available: f64) -> Vec<usize> {
    let max = ((available / large).ceil() as usize).clamp(1, MAX_VISIBLE);
    let min = ((min_available / large).floor().max(1.0) as usize).min(max);
    (min..=max).rev().collect()
}

fn multi_browse(
    viewport: f64,
    spacing: f64,
    count: usize,
    min_small: f64,
    max_small: f64,
    target_large: f64,
) -> Option<Keylines> {
    let large = target_large.min(viewport);
    let small = (large / 3.0).clamp(min_small, max_small);
    let medium = f64::midpoint(large, small);
    let small_counts: &[usize] = if viewport < min_small * 2.0 {
        &[0]
    } else {
        &[1]
    };
    let min_available = viewport - medium - max_small * small_counts[0] as f64;
    let large_options = large_counts(viewport, large, min_available);
    let targets = Targets {
        available: viewport,
        spacing,
        small,
        min_small,
        max_small,
        medium,
        large,
    };
    let mut arrangement = Arrangement::search(targets, small_counts, &[1, 0], &large_options)?;
    if arrangement.count() > count {
        let (mut small_count, mut medium_count) =
            (arrangement.small_count, arrangement.medium_count);
        for _ in 0..arrangement.count() - count {
            if small_count > 0 {
                small_count -= 1;
            } else if medium_count > 1 {
                medium_count -= 1;
            }
        }
        arrangement = Arrangement::search(targets, &[small_count], &[medium_count], &large_options)?;
    }
    Some(arrangement.start_aligned(viewport, spacing))
}

fn uncontained(viewport: f64, spacing: f64, item_extent: f64) -> Keylines {
    let large = item_extent.min(viewport);
    let large_count =
        (((viewport + spacing) / (large + spacing)).floor().max(1.0) as usize).min(MAX_VISIBLE);
    let remaining = (viewport - large_count as f64 * (large + spacing)).max(0.0);
    let medium_count = usize::from(remaining > 0.0);
    let mut medium = (remaining * 1.5).max(ANCHOR_EXTENT);
    if medium > large * MEDIUM_LARGE_RATIO {
        medium = (large * MEDIUM_LARGE_RATIO).max(remaining * 1.2).min(large);
    }
    let leading = ANCHOR_EXTENT.min(item_extent).max(medium * 0.5);
    let mut drafts = vec![Draft {
        size: leading,
        anchor: true,
    }];
    drafts.extend((0..large_count).map(|_| Draft {
        size: large,
        anchor: false,
    }));
    drafts.extend((0..medium_count).map(|_| Draft {
        size: medium,
        anchor: false,
    }));
    drafts.push(Draft {
        size: ANCHOR_EXTENT,
        anchor: true,
    });
    Keylines::with_pivot(&drafts, 1, large / 2.0, viewport, spacing)
}

fn hero(viewport: f64, spacing: f64, count: usize, alignment: f64) -> Keylines {
    let small = (viewport / 3.0).clamp(MIN_SMALL, MAX_SMALL);
    let sides = 2.0 - alignment.abs();
    let fits = viewport >= MIN_SMALL * sides.ceil() + MIN_SMALL * 1.25;
    let total = if count > 1 && fits {
        small * sides
    } else {
        0.0
    };
    let before = total * (alignment + 1.0) / 2.0;
    let after = total - before;
    let present = |size: f64| size > 1e-9;
    let gaps = f64::from(u8::from(present(before)) + u8::from(present(after)));
    let large = viewport - before - after - gaps * spacing;
    let anchor = Draft {
        size: ANCHOR_EXTENT,
        anchor: true,
    };
    let mut drafts = vec![anchor];
    if present(before) {
        drafts.push(Draft {
            size: before,
            anchor: false,
        });
    }
    let pivot = drafts.len();
    drafts.push(Draft {
        size: large,
        anchor: false,
    });
    if present(after) {
        drafts.push(Draft {
            size: after,
            anchor: false,
        });
    }
    drafts.push(anchor);
    let lead = if present(before) {
        before + spacing
    } else {
        0.0
    };
    Keylines::with_pivot(&drafts, pivot, lead + large / 2.0, viewport, spacing)
}

fn weighted(viewport: f64, spacing: f64, weights: &[f64]) -> Option<Keylines> {
    let shown: Vec<f64> = weights
        .iter()
        .copied()
        .filter(|&weight| weight > 0.0)
        .take(MAX_VISIBLE)
        .collect();
    let sum: f64 = shown.iter().sum();
    let available = viewport - (shown.len() as f64 - 1.0) * spacing;
    if shown.is_empty() || available <= 0.0 {
        return None;
    }
    let anchor = Draft {
        size: ANCHOR_EXTENT,
        anchor: true,
    };
    let mut drafts = vec![anchor];
    drafts.extend(shown.iter().map(|weight| Draft {
        size: weight / sum * available,
        anchor: false,
    }));
    drafts.push(anchor);
    let focal = (1..drafts.len() - 1)
        .find(|&index| shown[index - 1] == 1.0)
        .unwrap_or(1);
    let lead: f64 = drafts[1..focal]
        .iter()
        .map(|draft| draft.size + spacing)
        .sum();
    Some(Keylines::with_pivot(
        &drafts,
        focal,
        lead + drafts[focal].size / 2.0,
        viewport,
        spacing,
    ))
}

/// Where an item is drawn at an offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ItemPlacement {
    /// The extent of the item's mask, centred on the item.
    pub(crate) mask_extent: f64,
    /// Where the item's centre is drawn, from the viewport's leading edge.
    pub(crate) center: f64,
    /// How far the item is from the keyline before it to the one after, in `[0, 1]`.
    pub(crate) progress: f64,
}

/// The keylines of a layout for one viewport and item count, with the steps at both ends.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Strategy {
    rest: Keylines,
    start_steps: Vec<Keylines>,
    end_steps: Vec<Keylines>,
    start_shift: f64,
    end_shift: f64,
    start_points: Vec<f64>,
    end_points: Vec<f64>,
    item_extent: f64,
    spacing: f64,
    viewport: f64,
}

impl Strategy {
    /// The strategy of `layout` over a `viewport` long main axis; `None` when the viewport is not
    /// finite or not greater than zero, or the layout has nothing to show in it.
    pub(crate) fn new(
        layout: &CarouselLayout,
        viewport: f64,
        spacing: f64,
        count: usize,
    ) -> Option<Self> {
        if !viewport.is_finite() || viewport <= 0.0 || !spacing.is_finite() || spacing < 0.0 {
            return None;
        }
        let rest = match &layout.kind {
            LayoutKind::MultiBrowse {
                min_small,
                max_small,
                large,
            } => multi_browse(
                viewport,
                spacing,
                count,
                *min_small,
                *max_small,
                large.unwrap_or(viewport / 2.0),
            )?,
            LayoutKind::Uncontained { item_extent } => uncontained(viewport, spacing, *item_extent),
            LayoutKind::Hero { alignment } => hero(viewport, spacing, count, *alignment),
            LayoutKind::FullScreen => hero(viewport, spacing, 1, -1.0),
            LayoutKind::Weighted { weights } => weighted(viewport, spacing, weights)?,
        };
        let item_extent = rest.first_focal().size;
        if !(item_extent.is_finite() && item_extent > 0.0) {
            return None;
        }
        let start_steps = start_steps(&rest, viewport, spacing);
        let end_steps = end_steps(&rest, viewport, spacing);
        let start_shift = start_steps.last().map_or(0.0, |last| {
            (last.first().unadjusted - start_steps[0].first().unadjusted).max(0.0)
        });
        let end_shift = end_steps.last().map_or(0.0, |last| {
            (end_steps[0].last().unadjusted - last.last().unadjusted).max(0.0)
        });
        let start_points = step_points(start_shift, &start_steps, true);
        let end_points = step_points(end_shift, &end_steps, false);
        Some(Self {
            rest,
            start_steps,
            end_steps,
            start_shift,
            end_shift,
            start_points,
            end_points,
            item_extent,
            spacing,
            viewport,
        })
    }

    /// The extent every item is laid out at.
    pub(crate) fn item_extent(&self) -> f64 {
        self.item_extent
    }

    /// The distance from one item's start to the next one's.
    pub(crate) fn stride(&self) -> f64 {
        self.item_extent + self.spacing
    }

    /// The largest offset `count` items scroll to.
    pub(crate) fn max_offset(&self, count: usize) -> f64 {
        if count == 0 {
            return 0.0;
        }
        let strip = count as f64 * self.item_extent + (count - 1) as f64 * self.spacing;
        (strip - self.viewport).max(0.0)
    }

    /// The keylines at `offset` of a strip that scrolls to `max_offset`.
    pub(crate) fn keylines_at(&self, offset: f64, max_offset: f64) -> Keylines {
        let offset = offset.max(0.0);
        let start_end = self.start_shift;
        let end_start = (max_offset - self.end_shift).max(0.0);
        if (start_end..=end_start).contains(&offset) {
            return self.rest.clone();
        }
        let (mut steps, points, position) = if offset > end_start {
            (
                &self.end_steps[..],
                &self.end_points,
                lerp_range(0.0, 1.0, end_start, max_offset, offset),
            )
        } else {
            (
                &self.start_steps[..],
                &self.start_points,
                lerp_range(1.0, 0.0, 0.0, start_end, offset),
            )
        };
        // A strip too short for either end's shift goes from the start's last step straight to
        // the end's.
        let both;
        if offset > end_start
            && end_start < 0.01
            && self.start_steps.len() == 2
            && self.end_steps.len() == 2
        {
            both = [self.start_steps[1].clone(), self.end_steps[1].clone()];
            steps = &both;
        }
        if steps.len() < 2 || points.len() < steps.len() {
            return steps.last().unwrap_or(&self.rest).clone();
        }
        for index in 1..steps.len() {
            if position <= points[index] {
                let t = lerp_range(0.0, 1.0, points[index - 1], points[index], position);
                return steps[index - 1].lerp(&steps[index], t);
            }
        }
        steps[steps.len() - 1].clone()
    }

    /// Where item `index` is drawn at `offset` under `keylines`.
    pub(crate) fn place(&self, keylines: &Keylines, index: usize, offset: f64) -> ItemPlacement {
        let center = index as f64 * self.stride() + self.item_extent / 2.0 - offset;
        let (before, after) = (keylines.before(center), keylines.after(center));
        let span = after.unadjusted - before.unadjusted;
        let progress = if before == after || span <= 0.0 {
            1.0
        } else {
            ((center - before.unadjusted) / span).clamp(0.0, 1.0)
        };
        let keyline = lerp_keyline(before, after, progress);
        ItemPlacement {
            mask_extent: keyline.size.clamp(0.0, self.item_extent),
            center: keyline.offset,
            progress,
        }
    }

    /// The offset at which item `index` of `count` rests: on the first large keyline, or on
    /// the large keyline of the step that brings it there near either end.
    pub(crate) fn rest_offset(&self, index: usize, count: usize) -> f64 {
        let lead = |focal: &Keyline| focal.unadjusted - self.item_extent / 2.0;
        let mut shift = lead(self.rest.first_focal());
        if let Some(last) = self.start_steps.len().checked_sub(1)
            && index <= last
        {
            let step = &self.start_steps[last - index];
            shift = lead(step.first_focal());
        }
        if let Some(last_step) = self.end_steps.len().checked_sub(1)
            && count > self.rest.focal_count()
            && index + last_step + 1 >= count
        {
            let from_end = count - 1 - index;
            let step = &self.end_steps[last_step.saturating_sub(from_end)];
            shift = lead(step.last_focal());
        }
        (index as f64 * self.stride() - shift).clamp(0.0, self.max_offset(count))
    }

    /// The item resting nearest `offset`, the lower index on a tie; `None` without items.
    pub(crate) fn nearest_item(&self, offset: f64, count: usize) -> Option<usize> {
        if count == 0 || !offset.is_finite() {
            return (count > 0).then_some(0);
        }
        let shift = self.rest.first_focal().unadjusted - self.item_extent / 2.0;
        let guess = ((offset + shift) / self.stride()).round().max(0.0);
        let guess = if guess >= count as f64 {
            count - 1
        } else {
            guess as usize
        };
        let radius = self.start_steps.len().max(self.end_steps.len()) + 2;
        let low = guess.saturating_sub(radius);
        let high = guess.saturating_add(radius).min(count - 1);
        let mut best = (low, f64::INFINITY);
        for index in low..=high {
            let distance = (self.rest_offset(index, count) - offset).abs();
            if distance < best.1 {
                best = (index, distance);
            }
        }
        Some(best.0)
    }
}

fn start_steps(rest: &Keylines, viewport: f64, spacing: f64) -> Vec<Keylines> {
    let mut steps = vec![rest.clone()];
    if rest.first_focal_at_start() {
        return steps;
    }
    let start = rest.first_non_anchor();
    let end = rest.first_focal;
    if end <= start {
        if rest.first_focal().cutoff > 0.0 {
            steps.push(rest.moved(0, 0, viewport, spacing));
        }
        return steps;
    }
    for item in start..end {
        let previous = &steps[steps.len() - 1];
        let to = if item > 0 {
            previous
                .first_index_after_focal_with_size(rest.lines[item - 1].size)
                .saturating_sub(1)
        } else {
            rest.lines.len() - 1
        };
        let next = previous.moved(rest.first_non_anchor(), to, viewport, spacing);
        steps.push(next);
    }
    steps
}

fn end_steps(rest: &Keylines, viewport: f64, spacing: f64) -> Vec<Keylines> {
    let mut steps = vec![rest.clone()];
    if rest.last_focal_at_end(viewport) {
        return steps;
    }
    let start = rest.last_focal;
    let end = rest.last_non_anchor();
    if end <= start {
        if rest.last_focal().cutoff > 0.0 {
            steps.push(rest.moved(0, 0, viewport, spacing));
        }
        return steps;
    }
    for step in 0..end - start {
        let item = end - step;
        let previous = &steps[steps.len() - 1];
        let to = if item + 1 < rest.lines.len() {
            previous.last_index_before_focal_with_size(rest.lines[item + 1].size) + 1
        } else {
            0
        };
        let next = previous.moved(rest.last_non_anchor(), to, viewport, spacing);
        steps.push(next);
    }
    steps
}

/// Where each step begins, as a share of the whole shift.
fn step_points(total: f64, steps: &[Keylines], leading: bool) -> Vec<f64> {
    let mut points = vec![0.0];
    if total == 0.0 || steps.is_empty() {
        return points;
    }
    for index in 1..steps.len() {
        let (previous, current) = (&steps[index - 1], &steps[index]);
        let shifted = if leading {
            current.first().unadjusted - previous.first().unadjusted
        } else {
            previous.last().unadjusted - current.last().unadjusted
        };
        let point = if index == steps.len() - 1 {
            1.0
        } else {
            points[index - 1] + shifted / total
        };
        points.push(point);
    }
    points
}
