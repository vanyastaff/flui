//! Locating elements (ADR-0238 §4, R6, R7): a [`Locator`] resolved over the
//! published tree.
//!
//! A locator describes elements; it is resolved afresh at every use against
//! the tree as the adapter would read it, so it cannot act on a node that has
//! since been replaced. The criteria read the same wire [`Node`] an agent gets
//! from `semantics.read`, so a locator matches exactly what the outline shows.
//!
//! # Scope
//!
//! `within` and `has` are evaluated over the filtered tree: an ancestor or a
//! descendant is a node that tree shows, so an unlabelled container the
//! adapters lift away is not a scope. The scoping locators' own `pick` is
//! ignored: a scope is every element they describe.
//!
//! # Strictness
//!
//! [`SemanticsOwner::locate_wire`] lists what a locator matches, so a caller
//! can see what is ambiguous; `first`, `last` and `nth` narrow the list to the
//! one match they name. [`SemanticsOwner::resolve_wire_locator`] is where one
//! element is needed: it answers `ambiguous` for a strict locator with several
//! matches, and `not_found` with the nearest candidates for none.

use std::cmp::Reverse;

use accesskit_consumer::{NodeRef, TreeState};
use flui_protocol::{
    ElementRef, LocateParams, Locator, Match, Matches, Node, Pick, StateFilter, TextMatch,
    WindowId, outline,
};

use super::{Placement, Published, WireReadError, element_id, surface_rect, wire_node};
use crate::owner::SemanticsOwner;

/// Candidates an `ambiguous` or `not_found` answer lists.
const CANDIDATES: usize = 10;

/// Why a locator did not resolve to an element.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WireLocateError {
    /// No semantics tree has been assembled yet.
    #[error("no semantics tree has been assembled yet")]
    NoTree,
    /// The published tree could not be read.
    #[error("the published semantics tree is malformed")]
    Malformed,
    /// Nothing matches the locator (or `nth` is past the last match).
    #[error("no element matches the locator")]
    NotFound {
        /// The closest elements by role, name distance and identifier, as outline lines,
        /// closest first.
        nearest: Vec<String>,
    },
    /// A strict locator matches more than one element.
    #[error("the locator matches {count} elements; pick one with first, last or nth")]
    Ambiguous {
        /// How many elements match.
        count: usize,
        /// The first matches in tree order, as outline lines.
        candidates: Vec<String>,
    },
}

impl From<WireReadError> for WireLocateError {
    fn from(error: WireReadError) -> Self {
        match error {
            WireReadError::NoTree => Self::NoTree,
            // A locate never scopes a read, so `NotFound` cannot arise; the
            // tree itself is what is wrong.
            _ => Self::Malformed,
        }
    }
}

impl SemanticsOwner {
    /// The elements `params.locator` matches now, in tree order, each with the
    /// revision it was read on.
    ///
    /// A strict locator lists every match; `first`, `last` and `nth` list the
    /// one they name, or none. No match is an empty list, not an error: a wait
    /// for an element to appear polls this. `max_matches` keeps the first
    /// matches and counts the rest in `truncated`.
    ///
    /// # Errors
    ///
    /// [`WireLocateError::NoTree`] before the first assembly,
    /// [`WireLocateError::Malformed`] when the published tree breaks
    /// AccessKit's contract.
    pub fn locate_wire(
        &self,
        params: &LocateParams,
        placement: Placement,
    ) -> Result<Matches, WireLocateError> {
        let revision = self.revision();
        let located = self.with_published(|state, root_claims_focus| {
            let flat = Flat::new(
                state,
                self.device_pixel_ratio(),
                super::AgentFilter::of(self),
                root_claims_focus,
                placement,
            );
            let hits = flat.matching(&params.locator);
            let mut picked = pick(params.locator.pick, hits);
            let mut truncated = 0;
            if let Some(max) = params.max_matches {
                let max = usize::try_from(max).unwrap_or(usize::MAX);
                if picked.len() > max {
                    truncated = u32::try_from(picked.len() - max).unwrap_or(u32::MAX);
                    picked.truncate(max);
                }
            }
            let matches = picked
                .into_iter()
                .map(|index| flat.matched(index, placement, revision))
                .collect();
            let mut out = Matches::new(matches);
            out.truncated = truncated;
            out
        });
        located.map_err(published_error)
    }

    /// The one element `locator` names: what an action through a locator acts
    /// on.
    ///
    /// # Errors
    ///
    /// [`WireLocateError::NotFound`], listing the nearest elements, when
    /// nothing matches (or `nth` is past the last match);
    /// [`WireLocateError::Ambiguous`], listing the first matches, when a
    /// strict locator matches several; and the errors of
    /// [`Self::locate_wire`].
    pub fn resolve_wire_locator(
        &self,
        locator: &Locator,
        placement: Placement,
    ) -> Result<Match, WireLocateError> {
        let revision = self.revision();
        let resolved = self.with_published(|state, root_claims_focus| {
            let flat = Flat::new(
                state,
                self.device_pixel_ratio(),
                super::AgentFilter::of(self),
                root_claims_focus,
                placement,
            );
            let hits = flat.matching(locator);
            if locator.pick == Pick::Strict && hits.len() > 1 {
                return Err(WireLocateError::Ambiguous {
                    count: hits.len(),
                    candidates: hits
                        .iter()
                        .take(CANDIDATES)
                        .map(|&index| flat.line(index))
                        .collect(),
                });
            }
            match pick(locator.pick, hits).first() {
                Some(&index) => Ok(flat.matched(index, placement, revision)),
                None => Err(WireLocateError::NotFound {
                    nearest: flat.nearest(locator),
                }),
            }
        });
        resolved.map_err(published_error)?
    }
}

fn published_error(error: Published) -> WireLocateError {
    match error {
        Published::NoTree => WireLocateError::NoTree,
        Published::Malformed => WireLocateError::Malformed,
    }
}

/// The matches `pick` names, from all of them in tree order.
fn pick(pick: Pick, mut hits: Vec<usize>) -> Vec<usize> {
    match pick {
        Pick::First => {
            hits.truncate(1);
            hits
        }
        Pick::Last => hits.pop().into_iter().collect(),
        Pick::Nth(n) => hits
            .get(usize::try_from(n).unwrap_or(usize::MAX))
            .copied()
            .into_iter()
            .collect(),
        // Strict, and any pick a later schema adds, lists every match.
        _ => hits,
    }
}

/// One node of the filtered tree, with what a locator reads of it.
struct Entry {
    wire: Node,
    /// Itself or an ancestor is disabled.
    disabled: bool,
    parent: Option<usize>,
    /// The last index of this node's subtree.
    end: usize,
}

/// The filtered tree in document order, so a subtree is a range of indices.
struct Flat {
    entries: Vec<Entry>,
    window: WindowId,
}

impl Flat {
    fn new(
        state: &TreeState,
        published_ratio: f64,
        filter: super::AgentFilter,
        root_claims_focus: bool,
        placement: Placement,
    ) -> Self {
        let mut entries: Vec<Entry> = Vec::new();
        let mut stack: Vec<(NodeRef<'_>, Option<usize>)> = vec![(state.root(), None)];
        while let Some((node, parent)) = stack.pop() {
            let Some(id) = element_id(&node) else {
                continue;
            };
            let mut wire = wire_node(&node, id, root_claims_focus);
            wire.surface_rect = super::logical_bounds(&node, published_ratio)
                .map(|logical| surface_rect(logical, placement));
            if node.is_root() {
                wire.window = Some(placement.window);
            }
            let disabled = parent.is_some_and(|p| entries[p].disabled) || wire.disabled;
            let index = entries.len();
            entries.push(Entry {
                wire,
                disabled,
                parent,
                end: index,
            });
            stack.extend(
                node.filtered_children(|child| filter.apply(child))
                    .rev()
                    .map(|child| (child, Some(index))),
            );
        }
        // A subtree ends at its last descendant: children are visited after
        // their parent, so walking back folds each end into its parent's.
        for index in (1..entries.len()).rev() {
            if let Some(parent) = entries[index].parent {
                entries[parent].end = entries[parent].end.max(entries[index].end);
            }
        }
        Self {
            entries,
            window: placement.window,
        }
    }

    /// Every entry `locator` describes, in tree order.
    fn matching(&self, locator: &Locator) -> Vec<usize> {
        self.evaluate(locator)
            .into_iter()
            .enumerate()
            .filter_map(|(index, holds)| holds.then_some(index))
            .collect()
    }

    /// Which entries satisfy `locator`, ignoring its `pick`.
    fn evaluate(&self, locator: &Locator) -> Vec<bool> {
        let count = self.entries.len();
        if locator.window.is_some_and(|window| window != self.window) {
            return vec![false; count];
        }
        let mut holds: Vec<bool> = self
            .entries
            .iter()
            .map(|entry| criteria(locator, entry).all())
            .collect();
        if let Some(scope) = &locator.within {
            let scopes = self.evaluate(scope);
            let mut under = vec![false; count];
            for (index, entry) in self.entries.iter().enumerate() {
                if let Some(parent) = entry.parent {
                    under[index] = scopes[parent] || under[parent];
                }
                holds[index] &= under[index];
            }
        }
        if let Some(inner) = &locator.has {
            // Matches in a subtree, from running totals: the subtree of
            // `index` is the range after it up to its `end`.
            let inner = self.evaluate(inner);
            let mut seen = Vec::with_capacity(count + 1);
            seen.push(0_usize);
            for &holds_inner in &inner {
                seen.push(seen[seen.len() - 1] + usize::from(holds_inner));
            }
            for (index, entry) in self.entries.iter().enumerate() {
                holds[index] &= seen[entry.end + 1] - seen[index + 1] > 0;
            }
        }
        holds
    }

    fn matched(
        &self,
        index: usize,
        placement: Placement,
        revision: flui_protocol::TreeRevision,
    ) -> Match {
        let wire = self.entries[index].wire.clone();
        Match::new(ElementRef::new(placement.window, wire.id), revision, wire)
    }

    /// The entry as one outline line.
    fn line(&self, index: usize) -> String {
        outline(std::slice::from_ref(&self.entries[index].wire))
            .trim_end()
            .to_owned()
    }

    /// The entries closest to `locator`, closest first: those that hold the
    /// most of its criteria (a whole locator whose scope fails holds all of
    /// them), then the nearest by name.
    fn nearest(&self, locator: &Locator) -> Vec<String> {
        let needle = locator
            .name
            .as_ref()
            .map(|name| name.needle().to_lowercase());
        let mut ranked: Vec<(Reverse<usize>, usize, usize)> = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let held = criteria(locator, entry).held();
            let distance = match (&needle, &entry.wire.name) {
                (Some(needle), Some(name)) => Some(edit_distance(needle, &name.to_lowercase())),
                _ => None,
            };
            let near = match (&needle, distance) {
                (Some(needle), Some(distance)) => distance <= (needle.chars().count() / 3).max(2),
                _ => false,
            };
            if held > 0 || near {
                ranked.push((Reverse(held), distance.unwrap_or(usize::MAX), index));
            }
        }
        ranked.sort_unstable();
        ranked
            .into_iter()
            .take(CANDIDATES)
            .map(|(_, _, index)| self.line(index))
            .collect()
    }
}

/// How an entry stands against a locator's own criteria (not its scoping).
struct Criteria {
    held: usize,
    total: usize,
}

impl Criteria {
    fn all(&self) -> bool {
        self.held == self.total
    }

    fn held(&self) -> usize {
        self.held
    }
}

fn criteria(locator: &Locator, entry: &Entry) -> Criteria {
    let wire = &entry.wire;
    let mut out = Criteria { held: 0, total: 0 };
    let mut check = |required: bool, holds: bool| {
        if required {
            out.total += 1;
            out.held += usize::from(holds);
        }
    };
    check(locator.role.is_some(), locator.role == Some(wire.role));
    let text = |criterion: &Option<TextMatch>, text: &Option<String>| {
        criterion
            .as_ref()
            .is_some_and(|m| text.as_deref().is_some_and(|text| m.matches(text)))
    };
    check(locator.name.is_some(), text(&locator.name, &wire.name));
    check(locator.value.is_some(), text(&locator.value, &wire.value));
    check(
        locator.description.is_some(),
        text(&locator.description, &wire.description),
    );
    check(
        locator.identifier.is_some(),
        locator.identifier.is_some() && locator.identifier == wire.identifier,
    );
    let StateFilter {
        checked,
        disabled,
        focused,
        expanded,
        selected,
        ..
    } = locator.state;
    check(
        checked.is_some(),
        checked.is_some() && checked == wire.checked,
    );
    check(disabled.is_some(), disabled == Some(entry.disabled));
    check(focused.is_some(), focused == Some(wire.focused));
    check(
        expanded.is_some(),
        expanded.is_some() && expanded == wire.expanded,
    );
    check(
        selected.is_some(),
        selected.is_some() && selected == wire.selected,
    );
    out
}

/// The Levenshtein distance between `a` and `b`, on their first 64 characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().take(64).collect();
    let b: Vec<char> = b.chars().take(64).collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(ca != cb));
            diagonal = above;
        }
    }
    row[b.len()]
}
