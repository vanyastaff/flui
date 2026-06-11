//! Per-node layout calculation cache (Flutter's `_LayoutCacheStorage`).
//!
//! Mirrors `box.dart`'s four lazy maps: intrinsic dimensions keyed by
//! `(dimension, extent)`, dry-layout sizes keyed by the incoming
//! constraints, and one dry-baseline map per [`TextBaseline`] variant.
//! The storage lives on the framework side (`RenderState`), not on the
//! render object: objects stay pure `compute_*` functions and the
//! pipeline owns memoization and invalidation.
//!
//! Invalidation contract (`box.dart:2840`): `mark_needs_layout` clears
//! this storage; a non-empty clear means SOME ancestor's layout read
//! this node's intrinsics/baseline, so the dirty walk must escalate to
//! the parent even across a relayout boundary — the boundary only
//! isolates constraint-driven layout, not intrinsic queries.

use flui_types::Size;
use rustc_hash::FxHashMap;

use crate::constraints::BoxConstraints;
use crate::traits::TextBaseline;

/// Which intrinsic dimension a query asks for.
///
/// `MinWidth`/`MaxWidth` take a height extent; `MinHeight`/`MaxHeight`
/// take a width extent (Flutter `_IntrinsicDimension`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntrinsicDimension {
    /// Minimum width for a given height.
    MinWidth,
    /// Maximum width for a given height.
    MaxWidth,
    /// Minimum height for a given width.
    MinHeight,
    /// Maximum height for a given width.
    MaxHeight,
}

/// `f32` map key by exact bit pattern.
///
/// Cache keys need `Eq + Hash`, which `f32` lacks. Bit-exact keying is
/// the right equivalence for memoization: two extents that differ in
/// any bit (including `0.0` vs `-0.0`, or NaN payloads) at worst
/// recompute — they can never alias to a wrong cached value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct F32Key(u32);

impl From<f32> for F32Key {
    fn from(value: f32) -> Self {
        Self(value.to_bits())
    }
}

/// Constraint key for the dry-layout/baseline maps: the four bounds,
/// bit-exact (same equivalence argument as [`F32Key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ConstraintsKey([u32; 4]);

impl From<BoxConstraints> for ConstraintsKey {
    fn from(c: BoxConstraints) -> Self {
        Self([
            c.min_width.get().to_bits(),
            c.max_width.get().to_bits(),
            c.min_height.get().to_bits(),
            c.max_height.get().to_bits(),
        ])
    }
}

/// Box-protocol layout cache: Flutter's four maps, lazily allocated
/// behind ONE boxed allocation — most nodes are never probed for
/// intrinsics, and `RenderState` carries a per-node size budget, so a
/// cold cache costs a single null pointer rather than four map headers.
///
/// Average lookup O(1); worst case O(n) on hash collision (FxHashMap),
/// with n bounded by the number of distinct extents/constraints a
/// parent probes per layout pass (single digits in practice).
#[derive(Debug, Default)]
pub struct BoxLayoutCache {
    maps: Option<Box<CacheMaps>>,
}

/// The four maps, allocated together on first insert.
#[derive(Debug, Default)]
struct CacheMaps {
    /// `(dimension, extent) → intrinsic size`.
    intrinsic_dimensions: FxHashMap<(IntrinsicDimension, F32Key), f32>,
    /// `constraints → dry-layout size`.
    dry_layout_sizes: FxHashMap<ConstraintsKey, Size>,
    /// `constraints → dry alphabetic baseline` (`None` = computed, no baseline).
    alphabetic_baselines: FxHashMap<ConstraintsKey, Option<f32>>,
    /// `constraints → dry ideographic baseline`.
    ideographic_baselines: FxHashMap<ConstraintsKey, Option<f32>>,
}

impl BoxLayoutCache {
    /// Cached intrinsic value for `(dimension, extent)`, if present.
    ///
    /// Split from the insert (rather than a closure-memoize API)
    /// because the pipeline's walk computes the miss while the node is
    /// temporarily moved OUT of the borrow map — the cache cannot stay
    /// mutably borrowed across that recursion.
    #[must_use]
    pub fn peek_intrinsic(&self, dimension: IntrinsicDimension, extent: f32) -> Option<f32> {
        self.maps
            .as_ref()?
            .intrinsic_dimensions
            .get(&(dimension, extent.into()))
            .copied()
    }

    /// Stores a computed intrinsic value.
    pub fn insert_intrinsic(&mut self, dimension: IntrinsicDimension, extent: f32, value: f32) {
        self.maps
            .get_or_insert_default()
            .intrinsic_dimensions
            .insert((dimension, extent.into()), value);
    }

    /// Cached dry-layout size for `constraints`, if present.
    #[must_use]
    pub fn peek_dry_layout(&self, constraints: BoxConstraints) -> Option<Size> {
        self.maps
            .as_ref()?
            .dry_layout_sizes
            .get(&constraints.into())
            .copied()
    }

    /// Stores a computed dry-layout size.
    pub fn insert_dry_layout(&mut self, constraints: BoxConstraints, size: Size) {
        self.maps
            .get_or_insert_default()
            .dry_layout_sizes
            .insert(constraints.into(), size);
    }

    /// Cached dry baseline for `(constraints, baseline)`. The outer
    /// `Option` is the cache hit; the inner is the computed answer — a
    /// computed `None` ("this box has no baseline") is a valid cached
    /// value, so baseline-less boxes don't recompute every query.
    #[must_use]
    pub fn peek_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
    ) -> Option<Option<f32>> {
        let maps = self.maps.as_ref()?;
        let map = match baseline {
            TextBaseline::Alphabetic => &maps.alphabetic_baselines,
            TextBaseline::Ideographic => &maps.ideographic_baselines,
        };
        map.get(&constraints.into()).copied()
    }

    /// Stores a computed dry baseline (including a computed `None`).
    pub fn insert_dry_baseline(
        &mut self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        value: Option<f32>,
    ) {
        let maps = self.maps.get_or_insert_default();
        let map = match baseline {
            TextBaseline::Alphabetic => &mut maps.alphabetic_baselines,
            TextBaseline::Ideographic => &mut maps.ideographic_baselines,
        };
        map.insert(constraints.into(), value);
    }
}

/// Erased clear hook for the dirty-walk escalation; implemented by the
/// real Box cache and by the Sliver protocol's `()` placeholder.
pub trait ProtocolLayoutCache: std::fmt::Debug + Default + Send + Sync + 'static {
    /// Drops every cached entry. Returns `true` if anything WAS cached —
    /// the signal that an ancestor's layout consumed this node's
    /// intrinsics and the invalidation must escalate past relayout
    /// boundaries (Flutter `RenderBox.markNeedsLayout`, box.dart:2840).
    fn clear(&mut self) -> bool;
}

impl ProtocolLayoutCache for BoxLayoutCache {
    fn clear(&mut self) -> bool {
        let Some(maps) = &mut self.maps else {
            return false;
        };
        let had_cache = !maps.intrinsic_dimensions.is_empty()
            || !maps.dry_layout_sizes.is_empty()
            || !maps.alphabetic_baselines.is_empty()
            || !maps.ideographic_baselines.is_empty();
        if had_cache {
            // Keep the allocation (Flutter clears the maps, not the
            // fields): the same parent will re-probe the same extents
            // next frame.
            maps.intrinsic_dimensions.clear();
            maps.dry_layout_sizes.clear();
            maps.alphabetic_baselines.clear();
            maps.ideographic_baselines.clear();
        }
        had_cache
    }
}

/// Sliver nodes carry no layout cache yet: the sliver protocol's
/// geometry is scroll-driven and none of its objects expose intrinsic
/// queries today. `clear` reporting `false` means sliver invalidation
/// never escalates past a boundary on cache grounds.
impl ProtocolLayoutCache for () {
    fn clear(&mut self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn intrinsic_keyed_per_dimension_and_extent() {
        let mut cache = BoxLayoutCache::default();
        cache.insert_intrinsic(IntrinsicDimension::MinWidth, 100.0, 42.0);

        assert_eq!(
            cache.peek_intrinsic(IntrinsicDimension::MinWidth, 100.0),
            Some(42.0)
        );
        assert_eq!(
            cache.peek_intrinsic(IntrinsicDimension::MaxWidth, 100.0),
            None,
            "another dimension at the same extent is a distinct key"
        );
        assert_eq!(
            cache.peek_intrinsic(IntrinsicDimension::MinWidth, 50.0),
            None,
            "another extent in the same dimension is a distinct key"
        );
    }

    #[test]
    fn dry_baseline_caches_computed_none() {
        let mut cache = BoxLayoutCache::default();
        let constraints = BoxConstraints::tight(Size::new(px(10.0), px(10.0)));

        assert_eq!(
            cache.peek_dry_baseline(constraints, TextBaseline::Alphabetic),
            None,
            "cold cache misses"
        );
        cache.insert_dry_baseline(constraints, TextBaseline::Alphabetic, None);
        assert_eq!(
            cache.peek_dry_baseline(constraints, TextBaseline::Alphabetic),
            Some(None),
            "a computed no-baseline answer is a HIT, not a recompute"
        );
        assert_eq!(
            cache.peek_dry_baseline(constraints, TextBaseline::Ideographic),
            None,
            "the two baseline kinds are separate maps"
        );
    }

    #[test]
    fn clear_reports_whether_anything_was_cached() {
        let mut cache = BoxLayoutCache::default();
        assert!(!cache.clear(), "empty cache clears silently");

        cache.insert_intrinsic(IntrinsicDimension::MaxHeight, 80.0, 7.0);
        assert!(cache.clear(), "non-empty clear signals the escalation");
        assert!(!cache.clear(), "second clear is empty again");
    }
}
