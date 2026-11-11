//! The damage of one applied patch: the footprint of its operations under
//! the version before and after it, closed over what filters read
//! (ADR-0304 §4, §6).

use std::cmp::Reverse;

use flui_foundation::geometry::{Matrix4, Rect};

use super::content::{BoundaryContent, Bounds, Entry, EntryIndex, LinkRole};
use super::geometry::{Geometry, Link, Links, Reach, map_out};
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
    // A foreground filter whose scope or placement the patch changed seeds
    // the damage with its whole footprint: a subtree's stored bounds spread
    // a filter in the boundary's space, which a shrinking ancestor shrinks
    // below the renderer's device-pixel kernel, and the closure below only
    // grows a seed that meets it.
    let mut above_changes: Vec<u64> = Vec::new();
    for (geometry, version) in [(old, base), (new, &*next)] {
        let changed = facts
            .touched
            .iter()
            .chain(facts.placed)
            .copied()
            .chain(moved.iter().map(|(before, after)| {
                if std::ptr::eq(version, base) {
                    before.node
                } else {
                    after.node
                }
            }));
        for key in changed {
            if version.record(key).is_some() {
                above_changes.extend(geometry.path_up(key).map(NodeKey::table_key));
            }
        }
    }
    above_changes.sort_unstable();
    above_changes.dedup();
    let changed = |key: NodeKey| {
        facts.touched_marks.contains(key.slot()) || facts.placed_marks.contains(key.slot())
    };
    let affected = |geometry: Geometry<'_>, key: NodeKey| {
        above_changes.binary_search(&key.table_key()).is_ok() || geometry.path_up(key).any(changed)
    };
    let mut closures = Vec::new();
    let mut seeds = Vec::new();
    closures_of(
        old,
        base,
        &mut closures,
        &mut |key| affected(old, key),
        &mut seeds,
    );
    closures_of(
        new,
        next,
        &mut closures,
        &mut |key| affected(new, key),
        &mut seeds,
    );
    for seed in seeds {
        footprints.add(match seed {
            Closure::None => Reach::Nothing,
            Closure::Bounded { reads, writes } => Reach::rect(reads.union(&writes)),
            Closure::Unbounded => Reach::Unbounded,
        });
    }
    let region = footprints.region(surface);
    (close_over(region, closures, surface), cost)
}

/// The followers of `next` whose transform differs from their pair's in
/// `base`, as `(before, after)` addresses. A follower is paired with the one
/// of the same rank on the same link in the same node.
///
/// A follower that hides when its leader's branch clips its anchor counts as
/// moved too when the patch touched or placed a node on that branch, in
/// either version: a clip there can show or hide it with the transform
/// unchanged, and its footprint covers it either way.
fn moved_followers(
    base: &VersionInner,
    next: &VersionInner,
    facts: &PatchFacts<'_>,
    cost: &mut FootprintCost,
) -> Vec<(Address, Address)> {
    let (old, new) = (Geometry::new(base), Geometry::new(next));
    let changed = |key: NodeKey| {
        facts.touched_marks.contains(key.slot()) || facts.placed_marks.contains(key.slot())
    };
    let branch_changed = |geometry: Geometry<'_>, at: Address| {
        geometry
            .hiding_leader(at)
            .is_some_and(|leader| geometry.path_up(leader).any(changed))
    };
    let mut moved = Vec::new();
    for (_, users) in next.links.iter() {
        for &after in &users.followers {
            let Some(before) = paired_follower(base, next, after) else {
                continue;
            };
            cost.followers_resolved += 2;
            let then = old.resolve(before, 0);
            let now = new.resolve(after, 0);
            if then != now || branch_changed(old, before) || branch_changed(new, after) {
                moved.push((before, after));
            }
        }
    }
    moved
}

/// The follower in `base` that `after` (in `next`) continues.
fn paired_follower(base: &VersionInner, next: &VersionInner, after: Address) -> Option<Address> {
    let content = next.record(after.node)?.content();
    let (link, rank) = follower_rank(content, after.entry)?;
    let before = base.record(after.node)?.content();
    before
        .links()
        .iter()
        .filter(|(other, role, _)| *other == link && *role == LinkRole::Follower)
        .nth(rank)
        .map(|(_, _, entry)| Address {
            node: after.node,
            entry: *entry,
        })
}

fn follower_rank(
    content: &BoundaryContent,
    entry: EntryIndex,
) -> Option<(crate::link::LayerLink, usize)> {
    let (link, _, _) = content
        .links()
        .iter()
        .find(|(_, role, at)| *at == entry && *role == LinkRole::Follower)?;
    let rank = content
        .links()
        .iter()
        .filter(|(other, role, at)| other == link && *role == LinkRole::Follower && *at < entry)
        .count();
    Some((*link, rank))
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
        let bounds = Geometry::new(next).subtree_local(key).bounds();
        let record = next
            .nodes
            .get_mut(key.table_key(), table)
            .expect("BUG: a node whose bounds are recomputed is in the table");
        if record.subtree() != bounds {
            record.set_subtree(bounds);
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

/// Grows the bounds above `key` to cover its subtree, stopping at the first
/// ancestor that already does.
fn grow_ancestors(next: &mut VersionInner, key: NodeKey, table: &mut TableCost) {
    let mut child = key;
    while let Some((parent, grown)) = growth(next, child) {
        next.nodes
            .get_mut(parent.table_key(), table)
            .expect("BUG: a node's parent is in its version")
            .set_subtree(grown);
        child = parent;
    }
}

/// `child`'s parent and the bounds that cover its subtree too, `None` when
/// it has no parent or the parent's bounds already cover it.
fn growth(next: &VersionInner, child: NodeKey) -> Option<(NodeKey, Bounds)> {
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
    let reach = map_out(&links, Reach::of(record.subtree()));
    let current = Reach::of(parent_record.subtree());
    (!current.contains(reach)).then(|| (parent, current.union(reach).bounds()))
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
        .collect()
}

/// What each entry with a read closure in an attached filtered node of
/// `version` reads and writes; a foreground filter of a node `affected`
/// accepts is pushed to `seeds` too.
fn closures_of(
    geometry: Geometry<'_>,
    version: &VersionInner,
    out: &mut Vec<Closure>,
    affected: &mut dyn FnMut(NodeKey) -> bool,
    seeds: &mut Vec<Closure>,
) {
    for (_, &key) in version.filtered.iter() {
        let Some(record) = version.record(key) else {
            continue;
        };
        if !geometry.is_attached(key) {
            continue;
        }
        for (position, entry) in record.content().entries().iter().enumerate() {
            let at = Address {
                node: key,
                entry: EntryIndex::at(position),
            };
            match entry {
                Entry::Effect { layer, .. } => match layer {
                    Layer::BackdropFilter(_) | Layer::ColorFilter(_) => {
                        if let Some(ctm) = geometry.effect_ctm(at) {
                            out.push(read_closure(layer, &ctm, Rect::ZERO));
                        }
                    }
                    Layer::ImageFilter(_) => {
                        // What the filter takes in, on the surface: its
                        // scope's footprint, the filter's own spread aside.
                        let Some(ctm) = geometry.effect_ctm(at) else {
                            continue;
                        };
                        match geometry.filter_input(at) {
                            Reach::Rect(content) => {
                                let closure = read_closure(layer, &ctm, content);
                                if affected(key) {
                                    seeds.push(closure);
                                }
                                out.push(closure);
                            }
                            Reach::Nothing => {}
                            Reach::Unbounded | Reach::NonFinite => out.push(Closure::Unbounded),
                        }
                    }
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
    }
}
