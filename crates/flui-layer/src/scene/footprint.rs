//! The damage of one applied patch: the footprint of its operations under
//! the version before and after it, closed over what filters read
//! (ADR-0304 §4, §6).

use std::cell::Cell;
use std::cmp::Reverse;
use std::sync::Arc;

use flui_foundation::geometry::{Matrix4, Rect};
use smallvec::SmallVec;

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

/// The working vectors of the damage pass, kept by the scene owner so a
/// patch allocates only when it outgrows them. Each step clears what it uses
/// before it reads it, so vectors a refused or unwound patch left filled
/// serve the next one as well as empty ones.
#[derive(Debug, Default)]
pub(crate) struct DamageScratch {
    /// The footprints found so far, in surface pixels.
    rects: Vec<Rect<f64>>,
    /// The followers whose transform the patch changed.
    moved: Vec<(Address, Address)>,
    /// The nodes whose bounds are recomputed, deepest first.
    exact: Vec<(Reverse<u32>, NodeKey)>,
    pairs: FollowerPairs,
    content: ContentScratch,
    closures: Vec<Closure>,
}

/// The vectors the general pairing of one republished content uses.
#[derive(Debug, Default)]
struct ContentScratch {
    pair: Vec<Option<usize>>,
    order: Vec<(usize, usize)>,
    order_moved: Vec<(usize, usize)>,
    stable: Vec<bool>,
    kept: Vec<bool>,
    old_keys: Vec<(PairingKey, usize)>,
    new_keys: Vec<(PairingKey, usize)>,
    tails: Vec<usize>,
    links: Vec<Option<usize>>,
    in_place: Vec<bool>,
}

/// The footprints a patch damages, in surface pixels.
struct Footprints<'s> {
    rects: &'s mut Vec<Rect<f64>>,
    full: Option<FullReason>,
}

impl<'s> Footprints<'s> {
    fn new(rects: &'s mut Vec<Rect<f64>>) -> Self {
        rects.clear();
        Self { rects, full: None }
    }

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

    fn region(&self, surface: (u32, u32)) -> DamageRegion {
        if let Some(reason) = self.full {
            return DamageRegion::Full(reason);
        }
        let mut region: Option<Region> = None;
        for &rect in self.rects.iter() {
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
    scratch: &mut DamageScratch,
) -> (DamageRegion, FootprintCost) {
    let mut cost = FootprintCost::default();
    let DamageScratch {
        rects,
        moved,
        exact,
        pairs,
        content,
        closures,
    } = scratch;
    moved_followers(base, next, facts, &mut cost, pairs, moved);
    update_bounds(base, next, facts, moved, table, exact);

    let (old, new) = (Geometry::new(base), Geometry::new(next));
    let mut footprints = Footprints::new(rects);
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
            content,
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
    for &(before, after) in moved.iter() {
        footprints.add(old.entry_reach(before.node, before.entry));
        footprints.add(new.entry_reach(after.node, after.entry));
    }
    let region = footprints.region(surface);
    let damage = close_filtered(
        region,
        [(old, base), (new, next)],
        surface,
        &mut cost,
        closures,
    );
    (damage, cost)
}

/// The followers of `next` whose transform differs from their pair's in
/// `base`, as `(before, after)` addresses, into `moved`. A follower is paired
/// with the one of the same rank on the same link in the same node.
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
    pairs: &mut FollowerPairs,
    moved: &mut Vec<(Address, Address)>,
) {
    moved.clear();
    // No content of either version holds a link: no follower to pair, and
    // no link to count.
    if base.links.len() == 0 && next.links.len() == 0 {
        return;
    }
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
    pairs.fill(base, next, facts.touched, &mut cost.pairing_reads);
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
}

/// How the followers of each republished boundary pair across the patch:
/// per node, sorted by table key, its followers' entries after the patch
/// with the entry each continues, sorted by the entry after.
#[derive(Debug, Default)]
struct FollowerPairs {
    /// Each republished node's table key and its range of `pairs`.
    nodes: Vec<(u64, std::ops::Range<usize>)>,
    pairs: Vec<(EntryIndex, EntryIndex)>,
    keys: Vec<NodeKey>,
    old: Vec<(u64, EntryIndex)>,
    new: Vec<(u64, EntryIndex)>,
}

impl FollowerPairs {
    /// The pairing of every node the patch republished, built in one pass
    /// over each content's links.
    fn fill(
        &mut self,
        base: &VersionInner,
        next: &VersionInner,
        touched: &[NodeKey],
        reads: &mut u64,
    ) {
        self.nodes.clear();
        self.pairs.clear();
        self.keys.clear();
        self.keys.extend_from_slice(touched);
        self.keys.sort_unstable_by_key(|key| key.table_key());
        self.keys.dedup();
        for &key in &self.keys {
            let (Some(before), Some(after)) = (base.record(key), next.record(key)) else {
                continue;
            };
            if Arc::ptr_eq(before.content(), after.content()) {
                continue;
            }
            let start = self.pairs.len();
            pair_followers(
                before.content(),
                after.content(),
                reads,
                [&mut self.old, &mut self.new],
                &mut self.pairs,
            );
            self.nodes.push((key.table_key(), start..self.pairs.len()));
        }
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
            .nodes
            .binary_search_by_key(&after.node.table_key(), |(key, _)| *key)
            .ok()?;
        let pairs = &self.pairs[self.nodes[at].1.clone()];
        let pair = pairs
            .binary_search_by_key(&after.entry, |(entry, _)| *entry)
            .ok()?;
        Some(Address {
            node: after.node,
            entry: pairs[pair].1,
        })
    }
}

/// Appends to `pairs` each follower of `after` with the follower of `before`
/// of the same link and rank among that link's followers, sorted by the
/// entry in `after`, counting each link it reads into `reads`; `lists` holds
/// the two contents' followers while they pair.
fn pair_followers(
    before: &BoundaryContent,
    after: &BoundaryContent,
    reads: &mut u64,
    lists: [&mut Vec<(u64, EntryIndex)>; 2],
    pairs: &mut Vec<(EntryIndex, EntryIndex)>,
) {
    let followers =
        |content: &BoundaryContent, out: &mut Vec<(u64, EntryIndex)>, reads: &mut u64| {
            let links = content.links();
            *reads = reads.saturating_add(u64::try_from(links.len()).unwrap_or(u64::MAX));
            out.clear();
            out.extend(
                links
                    .iter()
                    .filter(|(_, role, _)| *role == LinkRole::Follower)
                    .map(|(link, _, entry)| (link.id(), *entry)),
            );
            // Within a link, by entry: its followers in paint order.
            out.sort_unstable();
        };
    let [old, new] = lists;
    followers(before, old, reads);
    followers(after, new, reads);
    let start = pairs.len();
    let mut cursor = 0;
    let mut previous: Option<(u64, usize)> = None;
    for &(link, entry) in new.iter() {
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
    pairs[start..].sort_unstable_by_key(|(entry, _)| *entry);
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
    exact: &mut Vec<(Reverse<u32>, NodeKey)>,
) {
    exact.clear();
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
    for &(_, key) in exact.iter() {
        let geometry = Geometry::new(next);
        let scope = geometry.subtree_scope(key);
        let (bounds, kernel) = (scope.local.bounds(), scope.kernel);
        let unchanged = next
            .record(key)
            .is_some_and(|record| record.subtree() == bounds && record.kernel() == kernel);
        if !unchanged {
            let record = next
                .nodes
                .get_mut(key.table_key(), table)
                .expect("BUG: a node whose bounds are recomputed is in the table");
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

/// An entry's pairing key: its kind, then its owner or node, then its ordinal.
type PairingKey = (u8, u64, u32);

/// The key a `Publish` pairs an entry by: a chunk and an effect by owner and
/// ordinal, a slot by its node.
fn pairing_key(entry: &Entry) -> PairingKey {
    match entry {
        Entry::Chunk { chunk, .. } => (0, chunk.owner().as_u64(), chunk.ordinal()),
        Entry::Effect { owner, ordinal, .. } => (1, owner.as_u64(), *ordinal),
        Entry::Slot { node, .. } => (2, node.key().table_key(), 0),
    }
}

/// Whether the paired entries at `old_index` of `before` and `index` of
/// `after` draw as before, their own part of the stream.
fn same_entry(
    before: &BoundaryContent,
    old_index: usize,
    after: &BoundaryContent,
    index: usize,
) -> bool {
    match (&before.entries()[old_index], &after.entries()[index]) {
        (Entry::Chunk { chunk: a, .. }, Entry::Chunk { chunk: b, .. }) => {
            std::sync::Arc::ptr_eq(a, b) || a.version() == b.version()
        }
        (Entry::Effect { .. }, Entry::Effect { .. }) => {
            match (
                before.effect_layer(EntryIndex::at(old_index)),
                after.effect_layer(EntryIndex::at(index)),
            ) {
                (Some(a), Some(b)) => a.same_effect(b),
                _ => false,
            }
        }
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
    footprints: &mut Footprints<'_>,
    scratch: &mut ContentScratch,
) {
    if before.shares_stream(after) {
        effects_damage(key, before, after, old, new, footprints);
        return;
    }
    let ContentScratch {
        pair,
        order,
        order_moved,
        stable,
        kept,
        old_keys,
        new_keys,
        tails,
        links,
        in_place,
    } = scratch;
    let (old_entries, new_entries) = (before.entries(), after.entries());
    // A content republished entry for entry in its old order (a layer
    // update, a repaint that drew the same objects) pairs each entry with the
    // one at its own position: no sort and no reordering to find.
    let aligned = old_entries.len() == new_entries.len()
        && old_entries
            .iter()
            .zip(new_entries)
            .all(|(old, new)| pairing_key(old) == pairing_key(new));
    pair.clear();
    order_moved.clear();
    if aligned {
        pair.extend((0..new_entries.len()).map(Some));
    } else {
        pair_by_key(old_entries, new_entries, [old_keys, new_keys], pair);
        order.clear();
        order.extend(
            pair.iter()
                .enumerate()
                .filter_map(|(index, old_index)| old_index.map(|old_index| (index, old_index))),
        );
        out_of_order(order, tails, links, in_place, order_moved);
    }
    // A new entry is stable when it continues an old one that draws the same
    // under the continuation of the same parent.
    stable.clear();
    stable.resize(new_entries.len(), false);
    kept.clear();
    kept.resize(old_entries.len(), false);
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
        if same_parent && same_entry(before, old_index, after, index) {
            stable[index] = true;
            kept[old_index] = true;
        }
    }
    // Paired entries out of their old order: every one outside a longest run
    // still in order moved past another, by the rule the differ uses.
    for &(index, old_index) in order_moved.iter() {
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
        if top(new_entries, stable, index) {
            footprints.add(new.entry_reach(key, EntryIndex::at(index)));
        }
    }
    for index in 0..old_entries.len() {
        if top(old_entries, kept, index) {
            footprints.add(old.entry_reach(key, EntryIndex::at(index)));
        }
    }
}

/// Damages what a composited-layer update changed: `after` shares
/// `before`'s stream, so only the effects whose layer changed draw
/// differently, and each topmost one (none of the effects it sits under
/// changed) damages its scope under both versions, as the general pairing
/// would. It costs the content's effects and the scopes of the changed ones,
/// never the content's other chunks.
fn effects_damage(
    key: NodeKey,
    before: &BoundaryContent,
    after: &BoundaryContent,
    old: Geometry<'_>,
    new: Geometry<'_>,
    footprints: &mut Footprints<'_>,
) {
    let (entries, effects) = (after.entries(), after.effects());
    let (old_layers, new_layers) = (before.layers(), after.layers());
    // Whether each effect, or one it sits under, changed.
    let mut changed: SmallVec<[bool; 32]> = SmallVec::with_capacity(effects.len());
    let mut tops: SmallVec<[EntryIndex; 4]> = SmallVec::new();
    for (position, &index) in effects.iter().enumerate() {
        let own = !old_layers[position].same_effect(&new_layers[position]);
        let above = entries[index.get()].parent().is_some_and(|parent| {
            let at = effects[..position]
                .binary_search(&parent)
                .expect("BUG: an effect's parent is an earlier effect");
            changed[at]
        });
        changed.push(own || above);
        if own && !above {
            tops.push(index);
        }
    }
    for &index in &tops {
        footprints.add(new.entry_reach(key, index));
    }
    for &index in &tops {
        footprints.add(old.entry_reach(key, index));
    }
}

/// Fills `pair` so `pair[j]` is the old entry new entry `j` continues, by
/// pairing key; a key that repeats pairs its first occurrences only. `keys`
/// holds the two streams' keys while they pair.
fn pair_by_key(
    old_entries: &[Entry],
    new_entries: &[Entry],
    keys: [&mut Vec<(PairingKey, usize)>; 2],
    pair: &mut Vec<Option<usize>>,
) {
    let [old_keys, new_keys] = keys;
    old_keys.clear();
    old_keys.extend(
        old_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (pairing_key(entry), index)),
    );
    new_keys.clear();
    new_keys.extend(
        new_entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (pairing_key(entry), index)),
    );
    old_keys.sort_unstable();
    new_keys.sort_unstable();
    pair.clear();
    pair.resize(new_entries.len(), None);
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
}

/// Fills `out` with the `(new, old)` pairs, listed in new order, that lie
/// outside a longest run whose old indexes increase; `tails`, `links` and
/// `in_place` are its working vectors.
fn out_of_order(
    order: &[(usize, usize)],
    tails: &mut Vec<usize>,
    links: &mut Vec<Option<usize>>,
    in_place: &mut Vec<bool>,
    out: &mut Vec<(usize, usize)>,
) {
    // Patience sort: `tails[k]` ends the best increasing run of length
    // `k + 1`; `links` walks each run back.
    tails.clear();
    links.clear();
    links.resize(order.len(), None);
    for (index, &(_, old)) in order.iter().enumerate() {
        let length = tails.partition_point(|&tail| order[tail].1 < old);
        links[index] = length.checked_sub(1).map(|shorter| tails[shorter]);
        if length == tails.len() {
            tails.push(index);
        } else {
            tails[length] = index;
        }
    }
    in_place.clear();
    in_place.resize(order.len(), false);
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        in_place[index] = true;
        cursor = links[index];
    }
    out.clear();
    out.extend(
        order
            .iter()
            .zip(in_place.iter())
            .filter_map(|(&pair, &stays)| (!stays).then_some(pair)),
    );
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
    closures: &mut Vec<Closure>,
) -> DamageRegion {
    closures.clear();
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
                closures,
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
                let Some(layer) = version
                    .record(key)
                    .and_then(|record| record.content().effect_layer(at.entry))
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
    for index in content.closure_entries() {
        *reads = reads.saturating_add(1);
        let Some(entry) = content.entries().get(index.get()) else {
            continue;
        };
        let at = Address {
            node: key,
            entry: index,
        };
        match entry {
            Entry::Effect { .. } => match content
                .effect_layer(index)
                .expect("BUG: an effect entry has a layer")
            {
                layer @ (Layer::BackdropFilter(_) | Layer::ColorFilter(_)) => {
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
