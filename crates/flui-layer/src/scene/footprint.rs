//! The damage of one applied patch: the footprint of its operations under
//! the version before and after it, closed over what filters read
//! (ADR-0304 §4, §6).

use std::cell::Cell;
use std::cmp::Reverse;
use std::sync::Arc;

use flui_foundation::geometry::{Matrix4, Rect};

use super::content::{BoundaryContent, Bounds, Entry, EntryIndex, LinkRole};
use super::geometry::{Geometry, Link, Links, Reach, Scope, Watch};
use super::id::NodeKey;
use super::owner::SlotMarks;
use super::table::TableCost;
use super::version::{Address, VersionInner};
use crate::damage::closure::{Closure, close_over, read_closure};
use crate::layer::Layer;
use crate::scene_snapshot::{DamageRect, DamageRegion, FullReason, Region};

/// What the damage pass reads of the patch.
pub(crate) struct PatchFacts<'p> {
    /// Every node the patch published or retired, in first-touch order.
    pub(crate) touched: &'p [NodeKey],
    /// Every node the patch placed, each once.
    pub(crate) placed: &'p [NodeKey],
    /// `touched` as a set.
    pub(crate) touched_marks: &'p SlotMarks,
    /// `placed` as a set.
    pub(crate) placed_marks: &'p SlotMarks,
    /// A node whose operations leave no footprint: the planted defect the
    /// property test must catch.
    pub(crate) skip: Option<NodeKey>,
}

/// What the pass costs, for the owner's counters.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct FootprintCost {
    pub(crate) followers_resolved: u64,
    /// Follower link entries read to pair a republished boundary's followers.
    pub(crate) pairing_reads: u64,
    /// Entries folded to read the inputs of foreground image filters.
    pub(crate) filter_input_reads: u64,
}

impl FootprintCost {
    /// Adds `other`'s counts to these.
    pub(crate) fn add(&mut self, other: Self) {
        self.followers_resolved = self
            .followers_resolved
            .saturating_add(other.followers_resolved);
        self.pairing_reads = self.pairing_reads.saturating_add(other.pairing_reads);
        self.filter_input_reads = self
            .filter_input_reads
            .saturating_add(other.filter_input_reads);
    }
}

/// The footprints a patch damages, in surface pixels.
#[derive(Default)]
struct Footprints {
    rects: Vec<Rect<f64>>,
    full: Option<FullReason>,
}

impl Footprints {
    fn add(&mut self, reach: Reach) {
        match reach {
            Reach::Nothing => {}
            Reach::Rect(rect) => self.rects.push(rect),
            Reach::Unbounded => {
                self.full.get_or_insert(FullReason::Unbounded);
            }
            Reach::NonFinite => {
                self.full.get_or_insert(FullReason::NonFinite);
            }
        }
    }

    fn region(self, surface: (u32, u32)) -> DamageRegion {
        if let Some(reason) = self.full {
            return DamageRegion::Full(reason);
        }
        let mut region: Option<Region> = None;
        for rect in self.rects {
            match DamageRect::try_covering(rect, surface) {
                Ok(Some(rect)) => match &mut region {
                    Some(region) => region.insert(rect),
                    None => region = Some(Region::new(rect)),
                },
                Ok(None) => {}
                Err(_) => return DamageRegion::Full(FullReason::NonFinite),
            }
        }
        region.map_or(DamageRegion::Unchanged, DamageRegion::Region)
    }
}

/// Brings `next`'s subtree bounds up to date and returns the patch's damage
/// against `base`, on a surface of `surface` device pixels.
pub(crate) fn patch_damage(
    base: &VersionInner,
    next: &mut VersionInner,
    facts: &PatchFacts<'_>,
    surface: (u32, u32),
    table: &mut TableCost,
) -> (DamageRegion, FootprintCost) {
    let mut cost = FootprintCost::default();
    let moved = moved_followers(base, next, facts, &mut cost);
    update_bounds(base, next, facts, &moved, table);

    let (old, new) = (Geometry::new(base), Geometry::new(next));
    let mut footprints = Footprints::default();
    for &key in facts.touched {
        if Some(key) == facts.skip {
            continue;
        }
        let (Some(before), Some(after)) = (base.record(key), next.record(key)) else {
            // Created: its parent's new slot damages it. Retired: no slot
            // names it, so it is drawn nowhere.
            continue;
        };
        if std::sync::Arc::ptr_eq(before.content(), after.content()) {
            continue;
        }
        content_damage(
            key,
            before.content(),
            after.content(),
            old,
            new,
            &mut footprints,
        );
    }
    for &key in facts.placed {
        if Some(key) == facts.skip {
            continue;
        }
        let (Some(before), Some(after)) = (base.record(key), next.record(key)) else {
            continue;
        };
        if before.placement() != after.placement() {
            footprints.add(old.subtree_reach(key));
            footprints.add(new.subtree_reach(key));
        }
    }
    for &(before, after) in &moved {
        footprints.add(old.entry_reach(before.node, before.entry));
        footprints.add(new.entry_reach(after.node, after.entry));
    }
    let region = footprints.region(surface);
    let damage = close_filtered(region, [(old, base), (new, next)], surface, &mut cost);
    (damage, cost)
}

/// The followers of `next` whose transform differs from their pair's in
/// `base`, as `(before, after)` addresses. A follower is paired with the one
/// of the same rank on the same link in the same node.
///
/// A follower that hides when its leader's branch clips its anchor counts as
/// moved too when the patch touched or placed a node on that branch, in
/// either version: a clip there can show or hide it with the transform
/// unchanged, and its footprint covers it either way. So does one whose
/// chain passes through such a follower: hidden, it hides the chain.
fn moved_followers(
    base: &VersionInner,
    next: &VersionInner,
    facts: &PatchFacts<'_>,
    cost: &mut FootprintCost,
) -> Vec<(Address, Address)> {
    let changed = |key: NodeKey| {
        facts.touched_marks.contains(key.slot()) || facts.placed_marks.contains(key.slot())
    };
    let watch = Watch {
        resolutions: Cell::new(0),
        changed: &changed,
        hiding_changed: Cell::new(false),
    };
    let (old, new) = (
        Geometry::watching(base, &watch),
        Geometry::watching(next, &watch),
    );
    let pairs = FollowerPairs::of(base, next, facts.touched, &mut cost.pairing_reads);
    let mut moved = Vec::new();
    for (_, users) in next.links.iter() {
        for &after in &users.followers {
            let Some(before) = pairs.before(base, next, after) else {
                continue;
            };
            watch.hiding_changed.set(false);
            let then = old.resolve(before, 0);
            let now = new.resolve(after, 0);
            if then != now || watch.hiding_changed.get() {
                moved.push((before, after));
            }
        }
    }
    cost.followers_resolved = cost
        .followers_resolved
        .saturating_add(watch.resolutions.get());
    moved
}

/// How the followers of each republished boundary pair across the patch:
/// per node, sorted by table key, its followers' entries after the patch
/// with the entry each continues, sorted by the entry after.
struct FollowerPairs(Vec<(u64, Vec<(EntryIndex, EntryIndex)>)>);

impl FollowerPairs {
    /// The pairing of every node the patch republished, built in one pass
    /// over each content's links.
    fn of(base: &VersionInner, next: &VersionInner, touched: &[NodeKey], reads: &mut u64) -> Self {
        let mut keys: Vec<NodeKey> = touched.to_vec();
        keys.sort_unstable_by_key(|key| key.table_key());
        keys.dedup();
        let mut nodes: Vec<(u64, Vec<(EntryIndex, EntryIndex)>)> = Vec::new();
        for key in keys {
            let (Some(before), Some(after)) = (base.record(key), next.record(key)) else {
                continue;
            };
            if Arc::ptr_eq(before.content(), after.content()) {
                continue;
            }
            nodes.push((
                key.table_key(),
                pair_followers(before.content(), after.content(), reads),
            ));
        }
        Self(nodes)
    }

    /// The follower in `base` that `after` (in `next`) continues: the same
    /// entry when the node kept its content, the one of the same link and
    /// rank among that link's followers when it was republished.
    fn before(&self, base: &VersionInner, next: &VersionInner, after: Address) -> Option<Address> {
        let (old, new) = (base.record(after.node)?, next.record(after.node)?);
        if Arc::ptr_eq(old.content(), new.content()) {
            return Some(after);
        }
        let at = self
            .0
            .binary_search_by_key(&after.node.table_key(), |(key, _)| *key)
            .ok()?;
        let pairs = &self.0[at].1;
        let pair = pairs
            .binary_search_by_key(&after.entry, |(entry, _)| *entry)
            .ok()?;
        Some(Address {
            node: after.node,
            entry: pairs[pair].1,
        })
    }
}

/// Each follower of `after` with the follower of `before` of the same link
/// and rank among that link's followers, sorted by the entry in `after`,
/// counting each link it reads into `reads`.
fn pair_followers(
    before: &BoundaryContent,
    after: &BoundaryContent,
    reads: &mut u64,
) -> Vec<(EntryIndex, EntryIndex)> {
    let mut followers = |content: &BoundaryContent| {
        let links = content.links();
        *reads = reads.saturating_add(u64::try_from(links.len()).unwrap_or(u64::MAX));
        let mut out: Vec<(u64, EntryIndex)> = links
            .iter()
            .filter(|(_, role, _)| *role == LinkRole::Follower)
            .map(|(link, _, entry)| (link.id(), *entry))
            .collect();
        // Within a link, by entry: its followers in paint order.
        out.sort_unstable();
        out
    };
    let (old, new) = (followers(before), followers(after));
    let mut pairs: Vec<(EntryIndex, EntryIndex)> = Vec::with_capacity(new.len().min(old.len()));
    let mut cursor = 0;
    let mut previous: Option<(u64, usize)> = None;
    for &(link, entry) in &new {
        let rank = match previous {
            Some((last, rank)) if last == link => rank + 1,
            _ => 0,
        };
        previous = Some((link, rank));
        while cursor < old.len() && old[cursor].0 < link {
            cursor += 1;
        }
        if let Some(&(other, before_entry)) = old.get(cursor + rank)
            && other == link
        {
            pairs.push((entry, before_entry));
        }
    }
    pairs.sort_unstable_by_key(|(entry, _)| *entry);
    pairs
}

/// Recomputes, deepest first, the subtree bounds of every node the patch
/// published or whose follower moved, then grows each changed node's
/// ancestors by union until one already covers it.
fn update_bounds(
    base: &VersionInner,
    next: &mut VersionInner,
    facts: &PatchFacts<'_>,
    moved: &[(Address, Address)],
    table: &mut TableCost,
) {
    let mut exact: Vec<(Reverse<u32>, NodeKey)> = Vec::new();
    for &key in facts.touched {
        if let Some(after) = next.record(key)
            && base
                .record(key)
                .is_none_or(|before| !std::sync::Arc::ptr_eq(before.content(), after.content()))
        {
            exact.push((Reverse(after.depth()), key));
        }
    }
    for (_, after) in moved {
        if let Some(record) = next.record(after.node) {
            exact.push((Reverse(record.depth()), after.node));
        }
    }
    exact.sort_unstable_by_key(|(depth, key)| (*depth, key.slot()));
    exact.dedup();
    for &(_, key) in &exact {
        let geometry = Geometry::new(next);
        let scope = geometry.subtree_scope(key);
        let (bounds, kernel) = (scope.local.bounds(), scope.kernel);
        let record = next
            .nodes
            .get_mut(key.table_key(), table)
            .expect("BUG: a node whose bounds are recomputed is in the table");
        if record.subtree() != bounds || record.kernel() != kernel {
            record.set_subtree(bounds);
            record.set_kernel(kernel);
        }
    }
    let grown = exact
        .iter()
        .map(|(_, key)| *key)
        .chain(facts.placed.iter().copied());
    for key in grown {
        grow_ancestors(next, key, table);
    }
}

/// Grows the bounds and kernel above `key` to cover its subtree, stopping at
/// the first ancestor that already does.
fn grow_ancestors(next: &mut VersionInner, key: NodeKey, table: &mut TableCost) {
    let mut child = key;
    while let Some((parent, grown, kernel)) = growth(next, child) {
        let record = next
            .nodes
            .get_mut(parent.table_key(), table)
            .expect("BUG: a node's parent is in its version");
        record.set_subtree(grown);
        record.set_kernel(kernel);
        child = parent;
    }
}

/// `child`'s parent with the bounds and kernel that cover its subtree too,
/// `None` when it has no parent or the parent's already do.
fn growth(next: &VersionInner, child: NodeKey) -> Option<(NodeKey, Bounds, f64)> {
    let record = next.record(child)?;
    let parent = record.parent()?;
    let parent_record = next
        .record(parent)
        .expect("BUG: a node's parent is in its version");
    let slot = parent_record
        .content()
        .slot_entry(child)
        .expect("BUG: a node's parent names it in a slot");
    let mut links = Links::new();
    Geometry::new(next).content_links(parent, slot, &mut links, 0);
    let offset = record.placement().offset();
    links.push(Link::Matrix(Matrix4::translation(
        offset.dx, offset.dy, 0.0,
    )));
    // The effects over the slot carry the child's scope as they do in the
    // parent's own content.
    let scope = Scope {
        local: Reach::of(record.subtree()),
        kernel: record.kernel(),
    }
    .through(&links);
    let reach = scope.local;
    let current = Reach::of(parent_record.subtree());
    let kernel = scope.kernel.max(parent_record.kernel());
    (!current.contains(reach) || kernel > parent_record.kernel())
        .then(|| (parent, current.union(reach).bounds(), kernel))
}

/// The key a `Publish` pairs an entry by: a chunk and an effect by owner and
/// ordinal, a slot by its node.
fn pairing_key(entry: &Entry) -> (u8, u64, u32) {
    match entry {
        Entry::Chunk { chunk, .. } => (0, chunk.owner().as_u64(), chunk.ordinal()),
        Entry::Effect { owner, ordinal, .. } => (1, owner.as_u64(), *ordinal),
        Entry::Slot { node, .. } => (2, node.key().table_key(), 0),
    }
}

/// Whether a paired entry draws as before, its own part of the stream.
fn same_entry(before: &Entry, after: &Entry) -> bool {
    match (before, after) {
        (Entry::Chunk { chunk: a, .. }, Entry::Chunk { chunk: b, .. }) => {
            std::sync::Arc::ptr_eq(a, b) || a.version() == b.version()
        }
        (Entry::Effect { layer: a, .. }, Entry::Effect { layer: b, .. }) => a.same_effect(b),
        (Entry::Slot { node: a, .. }, Entry::Slot { node: b, .. }) => a == b,
        _ => false,
    }
}

/// Damages what a republished content changed: each entry that is new,
/// gone, changed, under a changed effect chain or out of its old paint order
/// is damaged under the version it is drawn in, at the top of each changed
/// scope.
fn content_damage(
    key: NodeKey,
    before: &BoundaryContent,
    after: &BoundaryContent,
    old: Geometry<'_>,
    new: Geometry<'_>,
    footprints: &mut Footprints,
) {
    let (old_entries, new_entries) = (before.entries(), after.entries());
    let mut old_keys: Vec<((u8, u64, u32), usize)> = old_entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (pairing_key(entry), index))
        .collect();
    let mut new_keys: Vec<((u8, u64, u32), usize)> = new_entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (pairing_key(entry), index))
        .collect();
    old_keys.sort_unstable();
    new_keys.sort_unstable();
    // `pair[j]` is the old entry new entry `j` continues; a key that repeats
    // pairs its first occurrences only.
    let mut pair: Vec<Option<usize>> = vec![None; new_entries.len()];
    let (mut i, mut j) = (0, 0);
    while i < old_keys.len() && j < new_keys.len() {
        match old_keys[i].0.cmp(&new_keys[j].0) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                pair[new_keys[j].1] = Some(old_keys[i].1);
                let repeated = old_keys[i].0;
                while i < old_keys.len() && old_keys[i].0 == repeated {
                    i += 1;
                }
                while j < new_keys.len() && new_keys[j].0 == repeated {
                    j += 1;
                }
            }
        }
    }
    // A new entry is stable when it continues an old one that draws the same
    // under the continuation of the same parent.
    let mut stable = vec![false; new_entries.len()];
    let mut kept = vec![false; old_entries.len()];
    for (index, entry) in new_entries.iter().enumerate() {
        let Some(old_index) = pair[index] else {
            continue;
        };
        let old_entry = &old_entries[old_index];
        let same_parent = match (entry.parent(), old_entry.parent()) {
            (None, None) => true,
            (Some(parent), Some(old_parent)) => {
                stable[parent.get()] && pair[parent.get()] == Some(old_parent.get())
            }
            _ => false,
        };
        if same_parent && same_entry(old_entry, entry) {
            stable[index] = true;
            kept[old_index] = true;
        }
    }
    // Paired entries out of their old order: every one outside a longest run
    // still in order moved past another, by the rule the differ uses.
    let order: Vec<(usize, usize)> = pair
        .iter()
        .enumerate()
        .filter_map(|(index, old_index)| old_index.map(|old_index| (index, old_index)))
        .collect();
    for (index, old_index) in out_of_order(&order) {
        stable[index] = false;
        kept[old_index] = false;
    }
    let top = |entries: &[Entry], flags: &[bool], index: usize| {
        !flags[index]
            && entries[index]
                .parent()
                .is_none_or(|parent| flags[parent.get()])
    };
    for index in 0..new_entries.len() {
        if top(new_entries, &stable, index) {
            footprints.add(new.entry_reach(key, EntryIndex::at(index)));
        }
    }
    for index in 0..old_entries.len() {
        if top(old_entries, &kept, index) {
            footprints.add(old.entry_reach(key, EntryIndex::at(index)));
        }
    }
}

/// The `(new, old)` pairs, listed in new order, that lie outside a longest
/// run whose old indexes increase.
fn out_of_order(order: &[(usize, usize)]) -> Vec<(usize, usize)> {
    // Patience sort: `tails[k]` ends the best increasing run of length
    // `k + 1`; `links` walks each run back.
    let mut tails: Vec<usize> = Vec::new();
    let mut links: Vec<Option<usize>> = vec![None; order.len()];
    for (index, &(_, old)) in order.iter().enumerate() {
        let length = tails.partition_point(|&tail| order[tail].1 < old);
        links[index] = length.checked_sub(1).map(|shorter| tails[shorter]);
        if length == tails.len() {
            tails.push(index);
        } else {
            tails[length] = index;
        }
    }
    let mut in_place = vec![false; order.len()];
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        in_place[index] = true;
        cursor = links[index];
    }
    order
        .iter()
        .zip(in_place)
        .filter_map(|(&pair, stays)| (!stays).then_some(pair))
        .collect::<Vec<(usize, usize)>>()
}

/// `region` closed over what the entries of the attached filtered nodes of
/// both versions read and write.
///
/// A backdrop or colour filter and a chunk that reads the destination cost
/// their chain each. A node's foreground image filters are read, all in one
/// fold of its content, only once the region meets the node's subtree
/// footprint, which holds all they write: a patch pays for the filtered
/// content its damage reaches, not for every filter in the scene. One whose
/// filter paints transparent black has no bound and is read at once.
fn close_filtered(
    region: DamageRegion,
    versions: [(Geometry<'_>, &VersionInner); 2],
    surface: (u32, u32),
    cost: &mut FootprintCost,
) -> DamageRegion {
    let mut closures = Vec::new();
    let mut waiting = Vec::new();
    for (geometry, version) in versions {
        for (_, &key) in version.filtered.iter() {
            let Some(record) = version.record(key) else {
                continue;
            };
            if !geometry.is_attached(key) {
                continue;
            }
            if direct_closures(
                geometry,
                key,
                record.content(),
                &mut closures,
                &mut cost.filter_input_reads,
            ) {
                waiting.push((geometry, version, key, geometry.subtree_reach(key)));
            }
        }
    }
    let mut region = close_over(region, closures.iter().copied(), surface);
    loop {
        let DamageRegion::Region(current) = &region else {
            return region;
        };
        let before = waiting.len();
        waiting.retain(|&(geometry, version, key, reach)| {
            if !meets(current, reach, surface) {
                return true;
            }
            for (at, ctm, input) in geometry.filter_inputs(key, &mut cost.filter_input_reads) {
                let Some(Entry::Effect { layer, .. }) = version
                    .record(key)
                    .and_then(|record| record.content().entries().get(at.entry.get()))
                else {
                    continue;
                };
                match input {
                    Reach::Rect(content) => closures.push(read_closure(layer, &ctm, content)),
                    Reach::Nothing => {}
                    Reach::Unbounded | Reach::NonFinite => closures.push(Closure::Unbounded),
                }
            }
            false
        });
        if waiting.len() == before {
            return region;
        }
        region = close_over(region, closures.iter().copied(), surface);
    }
}

/// Pushes the closures of `key`'s entries that cost their chain alone: its
/// backdrop and colour filters, the chunks that read the destination, and
/// the foreground image filters that paint transparent black. Whether it
/// holds other foreground image filters, read once the damage reaches it.
///
/// Visits only the content's closure entries, each counted in `reads`: a
/// node's ordinary chunks cost nothing here however many it holds.
fn direct_closures(
    geometry: Geometry<'_>,
    key: NodeKey,
    content: &BoundaryContent,
    out: &mut Vec<Closure>,
    reads: &mut u64,
) -> bool {
    let mut foreground = false;
    for &index in content.closure_entries() {
        *reads = reads.saturating_add(1);
        let Some(entry) = content.entries().get(index.get()) else {
            continue;
        };
        let at = Address {
            node: key,
            entry: index,
        };
        match entry {
            Entry::Effect { layer, .. } => match layer {
                Layer::BackdropFilter(_) | Layer::ColorFilter(_) => {
                    if let Some(ctm) = geometry.effect_ctm(at) {
                        out.push(read_closure(layer, &ctm, Rect::ZERO));
                    }
                }
                Layer::ImageFilter(filter) if filter.filter().modifies_transparent_black() => {
                    if geometry.effect_ctm(at).is_some() {
                        out.push(Closure::Unbounded);
                    }
                }
                Layer::ImageFilter(_) => foreground = true,
                _ => {}
            },
            Entry::Chunk { chunk, .. } if chunk.reads_destination() => {
                match geometry.entry_reach(key, at.entry) {
                    Reach::Rect(bounds) => out.push(Closure::Bounded {
                        reads: bounds,
                        writes: bounds,
                    }),
                    Reach::Nothing => {}
                    Reach::Unbounded | Reach::NonFinite => out.push(Closure::Unbounded),
                }
            }
            Entry::Chunk { .. } | Entry::Slot { .. } => {}
        }
    }
    foreground
}

/// Whether `reach`, grown by a pixel of rounding on each side, meets a rect
/// of `region`; one with no finite bound meets every region.
fn meets(region: &Region, reach: Reach, surface: (u32, u32)) -> bool {
    match reach {
        Reach::Nothing => false,
        Reach::Rect(rect) => match DamageRect::try_covering(rect.expand(1.0), surface) {
            Ok(Some(rect)) => region.rects().iter().any(|damage| damage.overlaps(rect)),
            Ok(None) => false,
            Err(_) => true,
        },
        Reach::Unbounded | Reach::NonFinite => true,
    }
}
