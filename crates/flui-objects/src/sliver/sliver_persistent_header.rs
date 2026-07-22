//! `RenderSliverPersistentHeader` family — headers that shrink to a minimum
//! extent as the viewport scrolls, in four combinations of "does it scroll
//! off?" and "does it float back into view on reverse scroll?".
//!
//! Flutter parity: `rendering/sliver_persistent_header.dart`. Oracle line
//! numbers below refer to `.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver_persistent_header.dart`.
//!
//! # The four variants
//!
//! | Type | Scrolls off? | Floats back in on reverse scroll? |
//! |------|-------------|-----------------------------------|
//! | [`RenderSliverScrollingPersistentHeader`] | yes | no |
//! | [`RenderSliverPinnedPersistentHeader`] | no (pinned) | n/a (always visible) |
//! | [`RenderSliverFloatingPersistentHeader`] | yes | yes |
//! | [`RenderSliverFloatingPinnedPersistentHeader`] | no (pinned) | yes (grows back) |
//!
//! # Rust shape — two families, not one generic
//!
//! Flutter's hierarchy is a straight chain (`RenderSliverPersistentHeader` ->
//! `RenderSliverFloatingPersistentHeader` -> `RenderSliverFloatingPinnedPersistentHeader`,
//! plus the sibling `RenderSliverScrollingPersistentHeader` /
//! `RenderSliverPinnedPersistentHeader`). The Rust translation splits along
//! where the *state*, not just the *behavior*, actually diverges:
//!
//! - **Scrolling / Pinned** (this module's [`RenderSliverScrollingPersistentHeader`]
//!   and [`RenderSliverPinnedPersistentHeader`]) carry no animation state at
//!   all and their `perform_layout`/geometry formulas differ enough (Pinned's
//!   `max_scroll_obstruction_extent` contribution, its always-`0.0` child
//!   position, its distinct cache-extent formula) that collapsing them into
//!   one generic — à la [`RenderClip<S>`](crate::RenderClip) —
//!   would add ceremony without deduplicating anything nontrivial. They are
//!   two small, independent structs, each embedding [`PersistentHeaderCore`]
//!   directly.
//! - **Floating / FloatingPinned** ([`RenderSliverFloatingPersistentHeader`],
//!   [`RenderSliverFloatingPinnedPersistentHeader`]) share the ~40-line
//!   re-reveal state machine in `perform_layout` **verbatim** (the oracle's
//!   own comment on `RenderSliverFloatingPinnedPersistentHeader`, `:797-836`,
//!   says as much: "Everything else ... is verbatim identical"). Hand-copying
//!   that state machine into two structs is a real duplication-bug risk, so
//!   this pair follows the `RenderClip<S: ClipGeometry>` pattern instead:
//!   [`RenderSliverFloatingHeaderBase<M>`] is generic over the sealed
//!   [`FloatingHeaderMode`] trait, which carries only the `update_geometry`
//!   formula difference between the two. [`RenderSliverFloatingPersistentHeader`]
//!   and [`RenderSliverFloatingPinnedPersistentHeader`] are type aliases
//!   monomorphizing it.
//!
//! # Scope of this pass
//!
//! - `update_scroll_start_direction` / `maybe_start_snap_animation` /
//!   `maybe_stop_snap_animation` are implemented on
//!   [`RenderSliverFloatingHeaderBase`] (real oracle methods, harness-testable
//!   directly) but **no caller is wired** — in the oracle these are driven by
//!   `_FloatingHeaderState`/`_isScrollingListener` (`widgets/sliver_persistent_header.dart:202-244`),
//!   which listens to a `Scrollable`'s `ScrollPosition.isScrollingNotifier`.
//!   That `Scrollable`/`SliverAppBar`-layer wiring is a separate future pass.
//! - `show_on_screen` overrides are omitted entirely: `RenderObject::show_on_screen`
//!   does not exist anywhere in `flui-rendering` yet, so there is no base
//!   method to override. Whoever adds that infrastructure should note the
//!   oracle's `show_on_screen` trims in **sliver space** for Pinned but
//!   **child space** for Floating (`:709-714`) — the two are not
//!   interchangeable.
//! - The widget-layer `SliverPersistentHeader` + its delegate + `SliverAppBar`
//!   are out of scope. Flutter's own newer sibling widgets (`PinnedHeaderSliver`,
//!   `SliverFloatingHeader`) bypass the delegate+rebuild-in-layout design
//!   entirely in favor of an ordinary `Child` — model the eventual FLUI widget
//!   on those, not on the original delegate, per the plan behind this pass.
//!
//! # Traps ported around
//!
//! 1. `update_child` is called only under a three-way change-detection guard
//!    (needs-update / shrink-offset changed / overlaps-content changed), never
//!    unconditionally — see [`PersistentHeaderCore::layout_child`].
//! 2. The floating variants' `update_geometry` uses `effective_scroll_offset`
//!    for `paint_extent` but **raw** `constraints.scroll_offset` for
//!    `layout_extent` — conflating the two breaks the "float back into view
//!    without pushing siblings" effect.
//! 3. The re-reveal state machine's outer gate is a conjunction with history:
//!    "have we laid out before, AND (scrolling backward OR already partially
//!    revealed)" — both disjuncts matter.
//! 4. `allow_floating_expansion` has two disjuncts (`user_scroll_direction ==
//!    Forward` OR `last_started_scroll_direction == Some(Forward)`) — the
//!    second exists specifically for pointer/wheel scrolling.
//! 5. `max_scroll_obstruction_extent` must be reported on the Pinned /
//!    FloatingPinned variants even though nothing inside this render object
//!    consumes it: it feeds `RenderViewport::max_scroll_obstruction_extent_before`
//!    (`crates/flui-objects/src/sliver/viewport.rs:190-200`), which
//!    `RenderViewport::get_offset_to_reveal`-style scroll-into-view machinery
//!    would use. **Correction to the source plan**: the plan's own citation
//!    described this as feeding the *next sibling's* `SliverConstraints.overlap`
//!    inside `layoutChildSequence` — tracing both the oracle
//!    (`rendering/viewport.dart:828` computes `overlap` from an accumulated
//!    `maxPaintOffset` built from `paintExtent`, not `maxScrollObstructionExtent`)
//!    and FLUI's own `viewport.rs` (same `paint_extent`-based accumulation,
//!    `:410-411`) shows that is not the actual consumer.
//!    `maxScrollObstructionExtent`'s real, oracle-confirmed consumer is
//!    `maxScrollObstructionExtentBefore` (`rendering/viewport.dart:1352,1905,2223`),
//!    used by `getOffsetToReveal` for `showOnScreen` — exactly mirrored by
//!    FLUI's `RenderViewport::max_scroll_obstruction_extent_before`. The
//!    requirement to report the field correctly stands; only the described
//!    mechanism was corrected.
//! 6. The stretch-trigger signal is **edge-triggered** (`stretch_offset >=
//!    trigger && last_stretch_offset <= trigger`), firing once per crossing,
//!    not once per frame spent above the trigger.
//! 7. A trap the oracle itself doesn't call out: `layout_child`'s own
//!    stretch-offset formula (used for the child's box constraints, gated on
//!    `constraints.scroll_offset == 0.0`) is a **different formula** from
//!    `update_geometry`'s stretch-offset (used for `max_paint_extent`, gated
//!    only on `stretch_configuration.is_some()`, no scroll-offset check).
//!    Reusing one for the other silently changes stretch behavior.

use std::{
    fmt,
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use flui_animation::{
    Animatable, Animation, AnimationController, ArcCurve, CurvedAnimation, Curves, FloatTween,
};
use flui_foundation::{
    Diagnosticable, DiagnosticsBuilder, DiagnosticsNode, Listenable, ListenerId,
};
use flui_tree::Single;
use flui_types::{geometry::px, layout::Axis};

use flui_rendering::{
    constraints::{SliverConstraints, SliverGeometry, child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    pipeline::RepaintHandle,
    protocol::SliverProtocol,
    traits::{RenderObject, RenderSliver},
    view::ScrollDirection,
};

// =============================================================================
// Configuration types
// =============================================================================

/// Data-plane signal raised when a stretched header crosses its trigger offset.
///
/// This is intentionally not a callback. `RenderSliverPersistentHeader` is a
/// render object and must not store executable UI closures; owner/widget code can
/// hold a clone, compare [`count`](Self::count), and invoke an owner-local
/// callback from the UI runtime.
#[derive(Clone, Default)]
pub struct StretchTriggerSignal {
    count: Arc<AtomicU64>,
}

impl StretchTriggerSignal {
    /// Creates an untriggered signal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of edge-trigger crossings reported so far.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::SeqCst)
    }

    fn notify(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

impl fmt::Debug for StretchTriggerSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StretchTriggerSignal")
            .field("count", &self.count())
            .finish()
    }
}

/// Specifies how a stretched header reports overscroll trigger crossings.
///
/// Flutter parity: `OverScrollHeaderStretchConfiguration` (`:33-46`). The
/// signal fires **edge-triggered** — exactly once per crossing of
/// `stretch_trigger_offset` — see `PersistentHeaderCore::layout_child`.
#[derive(Clone)]
pub struct OverScrollHeaderStretchConfiguration {
    /// The overscroll extent required to notify [`stretch_trigger`](Self::stretch_trigger).
    pub stretch_trigger_offset: f32,
    /// Data-only notification raised once per crossing of
    /// `stretch_trigger_offset`.
    pub stretch_trigger: Option<StretchTriggerSignal>,
}

impl OverScrollHeaderStretchConfiguration {
    /// Creates a stretch configuration with an explicit trigger offset and
    /// optional data-plane signal.
    #[must_use]
    pub fn new(stretch_trigger_offset: f32, stretch_trigger: Option<StretchTriggerSignal>) -> Self {
        Self {
            stretch_trigger_offset,
            stretch_trigger,
        }
    }
}

impl Default for OverScrollHeaderStretchConfiguration {
    fn default() -> Self {
        Self {
            stretch_trigger_offset: 100.0,
            stretch_trigger: None,
        }
    }
}

impl fmt::Debug for OverScrollHeaderStretchConfiguration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverScrollHeaderStretchConfiguration")
            .field("stretch_trigger_offset", &self.stretch_trigger_offset)
            .field("has_stretch_trigger", &self.stretch_trigger.is_some())
            .finish()
    }
}

/// Specifies how a floating header snaps (animates) into or out of view.
///
/// Flutter parity: `FloatingHeaderSnapConfiguration` (`:659-671`). Consumed by
/// `RenderSliverFloatingHeaderBase::maybe_start_snap_animation` — see the
/// module docs for why no caller is wired in this pass.
#[derive(Debug, Clone)]
pub struct FloatingHeaderSnapConfiguration {
    /// The snap animation curve.
    pub curve: ArcCurve,
    /// The snap animation's duration.
    pub duration: Duration,
}

impl FloatingHeaderSnapConfiguration {
    /// Creates a snap configuration with an explicit curve and duration.
    #[must_use]
    pub fn new(curve: ArcCurve, duration: Duration) -> Self {
        Self { curve, duration }
    }
}

impl Default for FloatingHeaderSnapConfiguration {
    fn default() -> Self {
        Self {
            curve: ArcCurve::new(Curves::Ease),
            duration: Duration::from_millis(300),
        }
    }
}

// =============================================================================
// PersistentHeaderCore — shared by all four variants
// =============================================================================

/// State and layout math shared by every persistent-header variant, mirroring
/// the abstract base class `RenderSliverPersistentHeader` (`:120-345`).
///
/// `min_extent`/`max_extent` are plain fields here, not a delegate: the base
/// oracle class only ever sees them as abstract getters implemented by the
/// concrete subclass (`:136,144`), and Flutter's own delegate (which supplies
/// them at the *widget* layer) is a `build`-producing, View-shaped concept
/// that doesn't belong in `flui-rendering` — see this module's parent plan.
#[derive(Debug)]
struct PersistentHeaderCore {
    stretch_configuration: Option<OverScrollHeaderStretchConfiguration>,
    /// Mirrors `_lastStretchOffset` (`:130`). The oracle leaves this `late`
    /// (uninitialized until first write); FLUI initializes it to `0.0`, the
    /// natural "not yet triggered" value — a documented, harmless divergence
    /// that only differs from the oracle in the corner case of a first-ever
    /// layout whose overscroll already exceeds the trigger threshold.
    last_stretch_offset: f32,
    /// Starts `true` (`:159`) — the very first `layout_child` call always
    /// invokes `update_child` regardless of shrink-offset/overlap history.
    needs_update_child: bool,
    last_shrink_offset: f32,
    last_overlaps_content: bool,
    min_extent: f32,
    max_extent: f32,
}

impl PersistentHeaderCore {
    fn new(
        min_extent: f32,
        max_extent: f32,
        stretch_configuration: Option<OverScrollHeaderStretchConfiguration>,
    ) -> Self {
        Self {
            stretch_configuration,
            last_stretch_offset: 0.0,
            needs_update_child: true,
            last_shrink_offset: 0.0,
            last_overlaps_content: false,
            min_extent,
            max_extent,
        }
    }

    fn set_min_extent(&mut self, min_extent: f32) -> bool {
        if self.min_extent == min_extent {
            return false;
        }
        self.min_extent = min_extent;
        // Mirrors the oracle's `markNeedsLayout` override (`:203-209`), which
        // forces `_needsUpdateChild = true` on every dirtying event, not just
        // a shrink-offset change — a future delegate-driven child rebuild
        // must see the extent change even if the shrink offset happens to
        // land on the same numeric value as before.
        self.needs_update_child = true;
        true
    }

    fn set_max_extent(&mut self, max_extent: f32) -> bool {
        if self.max_extent == max_extent {
            return false;
        }
        self.max_extent = max_extent;
        self.needs_update_child = true;
        true
    }

    fn set_stretch_configuration(
        &mut self,
        stretch_configuration: Option<OverScrollHeaderStretchConfiguration>,
    ) -> bool {
        let was_some = self.stretch_configuration.is_some();
        let now_some = stretch_configuration.is_some();
        self.stretch_configuration = stretch_configuration;
        was_some != now_some
    }

    /// `update_geometry`'s stretch-offset formula — gated ONLY on
    /// `stretch_configuration.is_some()`, with **no** `scroll_offset == 0.0`
    /// check. This is a genuinely different formula from
    /// [`Self::layout_child`]'s own stretch-offset computation (trap #7 in
    /// the module docs); conflating them is an easy, plan-uncalled-out bug.
    fn stretch_offset_for_geometry(&self, constraints: &SliverConstraints) -> f32 {
        if self.stretch_configuration.is_some() {
            constraints.overlap.abs()
        } else {
            0.0
        }
    }

    /// Lays out the child, mirroring `layoutChild` (`:220-262`) exactly,
    /// including:
    /// - the three-way change-detection guard before calling `update_child`
    ///   (trap #1);
    /// - the `min_extent <= max_extent` invariant check;
    /// - a stretch-offset formula gated on `constraints.scroll_offset ==
    ///   0.0` — **distinct** from [`Self::stretch_offset_for_geometry`]
    ///   (trap #7);
    /// - the edge-triggered stretch trigger signal (trap #6).
    ///
    /// Returns the child's post-layout main-axis extent (`0.0` if there is no
    /// child, matching the oracle's `childExtent` getter for a `null` child).
    #[allow(clippy::too_many_arguments)]
    fn layout_child(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
        constraints: &SliverConstraints,
        scroll_offset: f32,
        overlaps_content: bool,
        update_child: impl FnOnce(f32, bool),
    ) -> f32 {
        let shrink_offset = scroll_offset.min(self.max_extent);
        if self.needs_update_child
            || self.last_shrink_offset != shrink_offset
            || self.last_overlaps_content != overlaps_content
        {
            update_child(shrink_offset, overlaps_content);
            self.last_shrink_offset = shrink_offset;
            self.last_overlaps_content = overlaps_content;
            self.needs_update_child = false;
        }
        debug_assert!(
            self.min_extent <= self.max_extent,
            "min_extent ({}) must not exceed max_extent ({})",
            self.min_extent,
            self.max_extent,
        );

        // Stretch extent for the CHILD's own box constraints — gated on
        // `scroll_offset == 0.0` (`:243-246`). See trap #7: this is NOT the
        // same formula `update_geometry` uses.
        let stretch_offset =
            if self.stretch_configuration.is_some() && constraints.scroll_offset == 0.0 {
                constraints.overlap.abs()
            } else {
                0.0
            };

        let child_extent = if ctx.child_count() > 0 {
            let max_child_extent =
                self.min_extent.max(self.max_extent - shrink_offset) + stretch_offset;
            let child_size = ctx.layout_box_child(
                0,
                constraints.as_box_constraints(0.0, max_child_extent, None),
            );
            match constraints.axis() {
                Axis::Horizontal => child_size.width.get(),
                Axis::Vertical => child_size.height.get(),
            }
        } else {
            0.0
        };

        if let Some(cfg) = self.stretch_configuration.as_ref()
            && let Some(trigger) = cfg.stretch_trigger.as_ref()
            && stretch_offset >= cfg.stretch_trigger_offset
            && self.last_stretch_offset <= cfg.stretch_trigger_offset
        {
            trigger.notify();
        }
        self.last_stretch_offset = stretch_offset;

        child_extent
    }
}

/// Positions the sliver's Box child (if any) using the shared
/// `child_paint_offset` helper, reusing the trick already established by
/// `RenderSliverToBoxAdapter`/`RenderSliverFillRemaining*`: the helper wants
/// `layout_offset` measured from the sliver's own scroll origin, so we pass
/// `child_position + constraints.scroll_offset` and let it cancel back to
/// `child_position` internally.
fn position_persistent_header_child(
    ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
    constraints: &SliverConstraints,
    geometry: &SliverGeometry,
    child_position: f32,
    child_extent: f32,
) {
    if ctx.child_count() == 0 {
        return;
    }
    let layout_offset = child_position + constraints.scroll_offset;
    let offset = child_paint_offset(constraints, geometry, px(layout_offset), px(child_extent));
    ctx.position_child(0, offset);
}

// =============================================================================
// RenderSliverScrollingPersistentHeader — "no effort to avoid overlapping"
// =============================================================================

/// A header that shrinks to `min_extent` as it hits the leading edge of the
/// viewport, then scrolls off normally.
///
/// Flutter parity: `RenderSliverScrollingPersistentHeader` (`:352-397`).
#[derive(Debug)]
pub struct RenderSliverScrollingPersistentHeader {
    core: PersistentHeaderCore,
    /// Cached return value of `update_geometry`, mirroring `_childPosition`
    /// (`:361`) — read back by `child_main_axis_position`.
    child_position: f32,
}

impl RenderSliverScrollingPersistentHeader {
    /// Creates a scrolling persistent header with the given extents.
    #[must_use]
    pub fn new(min_extent: f32, max_extent: f32) -> Self {
        Self {
            core: PersistentHeaderCore::new(min_extent, max_extent, None),
            child_position: 0.0,
        }
    }

    /// Installs a stretch configuration (builder style).
    #[must_use]
    pub fn with_stretch_configuration(
        mut self,
        stretch: OverScrollHeaderStretchConfiguration,
    ) -> Self {
        self.core.stretch_configuration = Some(stretch);
        self
    }

    /// The current minimum extent.
    #[must_use]
    pub fn min_extent(&self) -> f32 {
        self.core.min_extent
    }

    /// The current maximum extent.
    #[must_use]
    pub fn max_extent(&self) -> f32 {
        self.core.max_extent
    }

    /// Replaces the minimum extent; returns `true` if it changed.
    pub fn set_min_extent(&mut self, min_extent: f32) -> bool {
        self.core.set_min_extent(min_extent)
    }

    /// Replaces the maximum extent; returns `true` if it changed.
    pub fn set_max_extent(&mut self, max_extent: f32) -> bool {
        self.core.set_max_extent(max_extent)
    }

    /// Replaces the stretch configuration; returns `true` if presence changed.
    pub fn set_stretch_configuration(
        &mut self,
        stretch: Option<OverScrollHeaderStretchConfiguration>,
    ) -> bool {
        self.core.set_stretch_configuration(stretch)
    }

    /// Mirrors `updateGeometry` (`:365-383`) exactly: the return value uses
    /// the **raw**, pre-clamp `paint_extent` local — not the clamped value
    /// stored on `SliverGeometry` — matching the oracle's own
    /// `paintExtent - childExtent` (not `geometry.paintExtent - childExtent`).
    fn update_geometry(
        &self,
        constraints: &SliverConstraints,
        child_extent: f32,
    ) -> (SliverGeometry, f32) {
        let stretch_offset = self.core.stretch_offset_for_geometry(constraints);
        let max_extent = self.core.max_extent;
        let raw_paint_extent = max_extent - constraints.scroll_offset;
        let cache_extent = self.calculate_cache_offset(constraints, 0.0, max_extent);
        let geometry = SliverGeometry {
            scroll_extent: max_extent,
            paint_origin: constraints.overlap.min(0.0),
            paint_extent: raw_paint_extent.clamp(0.0, constraints.remaining_paint_extent),
            max_paint_extent: max_extent + stretch_offset,
            cache_extent,
            has_visual_overflow: true,
            ..SliverGeometry::ZERO
        };
        let child_position = if stretch_offset > 0.0 {
            0.0
        } else {
            (raw_paint_extent - child_extent).min(0.0)
        };
        (geometry, child_position)
    }
}

impl Diagnosticable for RenderSliverScrollingPersistentHeader {
    fn debug_fill_properties(&self, builder: &mut DiagnosticsBuilder) {
        builder.add("min_extent", self.core.min_extent);
        builder.add("max_extent", self.core.max_extent);
    }
}

impl RenderSliver for RenderSliverScrollingPersistentHeader {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        // Mirrors `performLayout` (`:385-389`): no `overlaps_content` arg,
        // defaults to `false`.
        let child_extent = self.core.layout_child(
            ctx,
            &constraints,
            constraints.scroll_offset,
            false,
            |_, _| {},
        );
        let (geometry, child_position) = self.update_geometry(&constraints, child_extent);
        self.child_position = child_position;
        position_persistent_header_child(
            ctx,
            &constraints,
            &geometry,
            child_position,
            child_extent,
        );
        geometry
    }

    fn child_main_axis_position(
        &self,
        _constraints: &SliverConstraints,
        _child: &dyn RenderObject<SliverProtocol>,
    ) -> f32 {
        self.child_position
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Single, Self::ParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

// =============================================================================
// RenderSliverPinnedPersistentHeader — "never scrolls off"
// =============================================================================

/// A header that shrinks to `min_extent` as it hits the leading edge of the
/// viewport, then stays pinned there.
///
/// Flutter parity: `RenderSliverPinnedPersistentHeader` (`:404-473`), minus
/// the `show_on_screen` override — see the module docs.
#[derive(Debug)]
pub struct RenderSliverPinnedPersistentHeader {
    core: PersistentHeaderCore,
}

impl RenderSliverPinnedPersistentHeader {
    /// Creates a pinned persistent header with the given extents.
    #[must_use]
    pub fn new(min_extent: f32, max_extent: f32) -> Self {
        Self {
            core: PersistentHeaderCore::new(min_extent, max_extent, None),
        }
    }

    /// Installs a stretch configuration (builder style).
    #[must_use]
    pub fn with_stretch_configuration(
        mut self,
        stretch: OverScrollHeaderStretchConfiguration,
    ) -> Self {
        self.core.stretch_configuration = Some(stretch);
        self
    }

    /// The current minimum extent.
    #[must_use]
    pub fn min_extent(&self) -> f32 {
        self.core.min_extent
    }

    /// The current maximum extent.
    #[must_use]
    pub fn max_extent(&self) -> f32 {
        self.core.max_extent
    }

    /// Replaces the minimum extent; returns `true` if it changed.
    pub fn set_min_extent(&mut self, min_extent: f32) -> bool {
        self.core.set_min_extent(min_extent)
    }

    /// Replaces the maximum extent; returns `true` if it changed.
    pub fn set_max_extent(&mut self, max_extent: f32) -> bool {
        self.core.set_max_extent(max_extent)
    }

    /// Replaces the stretch configuration; returns `true` if presence changed.
    pub fn set_stretch_configuration(
        &mut self,
        stretch: Option<OverScrollHeaderStretchConfiguration>,
    ) -> bool {
        self.core.set_stretch_configuration(stretch)
    }
}

impl Diagnosticable for RenderSliverPinnedPersistentHeader {
    fn debug_fill_properties(&self, builder: &mut DiagnosticsBuilder) {
        builder.add("min_extent", self.core.min_extent);
        builder.add("max_extent", self.core.max_extent);
    }
}

impl RenderSliver for RenderSliverPinnedPersistentHeader {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let max_extent = self.core.max_extent;
        let min_extent = self.core.min_extent;
        let overlaps_content = constraints.overlap > 0.0;

        let child_extent = self.core.layout_child(
            ctx,
            &constraints,
            constraints.scroll_offset,
            overlaps_content,
            |_, _| {},
        );

        let effective_remaining_paint_extent =
            (constraints.remaining_paint_extent - constraints.overlap).max(0.0);
        let layout_extent =
            (max_extent - constraints.scroll_offset).clamp(0.0, effective_remaining_paint_extent);
        let stretch_offset = self.core.stretch_offset_for_geometry(&constraints);

        let geometry = SliverGeometry {
            scroll_extent: max_extent,
            paint_origin: constraints.overlap,
            paint_extent: child_extent.min(effective_remaining_paint_extent),
            layout_extent,
            max_paint_extent: max_extent + stretch_offset,
            max_scroll_obstruction_extent: min_extent,
            cache_extent: if layout_extent > 0.0 {
                -constraints.cache_origin + layout_extent
            } else {
                layout_extent
            },
            has_visual_overflow: true,
            ..SliverGeometry::ZERO
        };

        position_persistent_header_child(ctx, &constraints, &geometry, 0.0, child_extent);
        geometry
    }

    fn child_main_axis_position(
        &self,
        _constraints: &SliverConstraints,
        _child: &dyn RenderObject<SliverProtocol>,
    ) -> f32 {
        // The defining "pinned" behavior — always at the leading edge.
        0.0
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Single, Self::ParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

// =============================================================================
// FloatingHeaderMode — sealed trait for the animated pair
// =============================================================================

mod sealed {
    pub trait Sealed {}
}

/// Per-variant `update_geometry` formula for the floating pair.
///
/// Sealed: [`FloatingMode`] (scrolls off) and [`FloatingPinnedMode`] (stays
/// pinned) are the only implementors. This trait exists purely to
/// deduplicate the ~40-line re-reveal state machine in
/// [`RenderSliverFloatingHeaderBase::perform_layout`], which the oracle
/// itself documents as verbatim-identical between the two subclasses
/// (`:797-836`) — everything else about the two types is shared.
pub trait FloatingHeaderMode: sealed::Sealed + Send + Sync + 'static {
    /// Flutter-parity diagnostics label.
    const DIAGNOSTIC_NAME: &'static str;

    /// Computes this variant's [`SliverGeometry`] and the child's main-axis
    /// position, given the already-computed `effective_scroll_offset` and
    /// post-layout `child_extent`.
    fn update_geometry(
        core: &PersistentHeaderCoreView<'_>,
        constraints: &SliverConstraints,
        effective_scroll_offset: f32,
        child_extent: f32,
    ) -> (SliverGeometry, f32);
}

/// A read-only view of the fields of [`PersistentHeaderCore`] the sealed
/// [`FloatingHeaderMode`] formulas need, without giving them access to
/// `layout_child`/setters (which stay `perform_layout`'s job).
#[derive(Debug)]
pub struct PersistentHeaderCoreView<'a> {
    core: &'a PersistentHeaderCore,
}

impl PersistentHeaderCoreView<'_> {
    /// The minimum extent.
    #[must_use]
    pub fn min_extent(&self) -> f32 {
        self.core.min_extent
    }

    /// The maximum extent.
    #[must_use]
    pub fn max_extent(&self) -> f32 {
        self.core.max_extent
    }

    /// `update_geometry`'s stretch-offset formula — see
    /// [`PersistentHeaderCore::stretch_offset_for_geometry`].
    #[must_use]
    pub fn stretch_offset_for_geometry(&self, constraints: &SliverConstraints) -> f32 {
        self.core.stretch_offset_for_geometry(constraints)
    }
}

/// Marker mode for [`RenderSliverFloatingPersistentHeader`] — scrolls off the
/// leading edge when fully shrunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingMode;

/// Marker mode for [`RenderSliverFloatingPinnedPersistentHeader`] — stays
/// pinned at the leading edge when fully shrunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingPinnedMode;

impl sealed::Sealed for FloatingMode {}
impl sealed::Sealed for FloatingPinnedMode {}

impl FloatingHeaderMode for FloatingMode {
    const DIAGNOSTIC_NAME: &'static str = "RenderSliverFloatingPersistentHeader";

    fn update_geometry(
        core: &PersistentHeaderCoreView<'_>,
        constraints: &SliverConstraints,
        effective_scroll_offset: f32,
        child_extent: f32,
    ) -> (SliverGeometry, f32) {
        let stretch_offset = core.stretch_offset_for_geometry(constraints);
        let max_extent = core.max_extent();
        // Trap #2: paint_extent uses `effective_scroll_offset`, layout_extent
        // uses the RAW `constraints.scroll_offset` — do not conflate.
        let raw_paint_extent = max_extent - effective_scroll_offset;
        let raw_layout_extent = max_extent - constraints.scroll_offset;
        let paint_extent = raw_paint_extent.clamp(0.0, constraints.remaining_paint_extent);
        let layout_extent = raw_layout_extent.clamp(0.0, constraints.remaining_paint_extent);
        let geometry = SliverGeometry {
            scroll_extent: max_extent,
            paint_origin: constraints.overlap.min(0.0),
            paint_extent,
            layout_extent,
            max_paint_extent: max_extent + stretch_offset,
            // `cacheExtent` was not given explicitly by the oracle here, so it
            // falls back to `layoutExtent` per `SliverGeometry`'s own
            // constructor default chain (`cacheExtent ?? layoutExtent ??
            // paintExtent`, `sliver.dart:660-664`).
            cache_extent: layout_extent,
            has_visual_overflow: true,
            ..SliverGeometry::ZERO
        };
        // Uses the RAW (pre-clamp) `paint_extent` local, matching the
        // oracle's `paintExtent - childExtent` — not `geometry.paintExtent`.
        let child_position = if stretch_offset > 0.0 {
            0.0
        } else {
            (raw_paint_extent - child_extent).min(0.0)
        };
        (geometry, child_position)
    }
}

impl FloatingHeaderMode for FloatingPinnedMode {
    const DIAGNOSTIC_NAME: &'static str = "RenderSliverFloatingPinnedPersistentHeader";

    fn update_geometry(
        core: &PersistentHeaderCoreView<'_>,
        constraints: &SliverConstraints,
        effective_scroll_offset: f32,
        child_extent: f32,
    ) -> (SliverGeometry, f32) {
        let _ = child_extent; // Always pinned at 0.0 — unlike Floating, which can be negative.
        let min_extent = core.min_extent();
        let max_extent = core.max_extent();
        let min_allowed_extent = if constraints.remaining_paint_extent > min_extent {
            min_extent
        } else {
            constraints.remaining_paint_extent
        };
        let paint_extent = max_extent - effective_scroll_offset;
        let clamped_paint_extent =
            paint_extent.clamp(min_allowed_extent, constraints.remaining_paint_extent);
        let layout_extent =
            (max_extent - constraints.scroll_offset).clamp(0.0, clamped_paint_extent);
        let stretch_offset = core.stretch_offset_for_geometry(constraints);
        let geometry = SliverGeometry {
            scroll_extent: max_extent,
            paint_origin: constraints.overlap.min(0.0),
            paint_extent: clamped_paint_extent,
            layout_extent,
            max_paint_extent: max_extent + stretch_offset,
            max_scroll_obstruction_extent: min_extent,
            cache_extent: layout_extent,
            has_visual_overflow: true,
            ..SliverGeometry::ZERO
        };
        (geometry, 0.0)
    }
}

// =============================================================================
// RenderSliverFloatingHeaderBase<M> — the animated pair
// =============================================================================

/// A header that shrinks like [`RenderSliverScrollingPersistentHeader`]
/// (`M = FloatingMode`) or [`RenderSliverPinnedPersistentHeader`]
/// (`M = FloatingPinnedMode`), but immediately floats back into view when
/// the user scrolls in the reverse direction.
///
/// Flutter parity: `RenderSliverFloatingPersistentHeader` (`:508-787`) and
/// `RenderSliverFloatingPinnedPersistentHeader` (`:797-836`).
///
/// # Snap-animation controller injection
///
/// Like [`RenderAnimatedSize`](crate::RenderAnimatedSize), this render
/// object never builds its own [`AnimationController`] — it receives one
/// already-built (or `None`, meaning snapping is unavailable) at
/// construction, and subscribes to it in [`attach`](RenderSliver::attach).
pub type RenderSliverFloatingPersistentHeader = RenderSliverFloatingHeaderBase<FloatingMode>;

/// See `RenderSliverFloatingHeaderBase`, the generic engine both of this
/// family's aliases monomorphize.
pub type RenderSliverFloatingPinnedPersistentHeader =
    RenderSliverFloatingHeaderBase<FloatingPinnedMode>;

/// Generic engine behind [`RenderSliverFloatingPersistentHeader`] and
/// [`RenderSliverFloatingPinnedPersistentHeader`] — see the module docs for
/// why this pair is generic while Scrolling/Pinned are not.
pub struct RenderSliverFloatingHeaderBase<M: FloatingHeaderMode> {
    core: PersistentHeaderCore,
    /// Injected, already-built controller. `None` means this
    /// header never snaps/expands — `maybe_start_snap_animation` becomes an
    /// inert no-op.
    controller: Option<AnimationController>,
    /// Lazily (re)built by [`Self::update_animation`] on the first retarget —
    /// mirrors the oracle's `_animation`, rebuilt fresh per
    /// `_updateAnimation` call (`:599-614`), not fixed at construction.
    animation: Option<CurvedAnimation<ArcCurve>>,
    /// The begin/end span the animation interpolates over. Rebuilt alongside
    /// `animation` in [`Self::update_animation`].
    float_tween: FloatTween,
    /// The animation-driven value already folded into `effective_scroll_offset`
    /// as of the last `perform_layout` — mirrors the oracle's listener guard
    /// (`if (_effectiveScrollOffset == _animation.value) return;`,
    /// `:601-603`) without needing `&mut self` access from inside the
    /// `Arc<dyn Fn>` `attach` callback. Deliberately a **separate** field from
    /// `effective_scroll_offset`, not a direct comparison against it: once a
    /// drive settles, `effective_scroll_offset` legitimately keeps moving via
    /// later *real-scroll*-driven `perform_layout` calls, and comparing
    /// directly against the (now-stale, frozen) animation value would wrongly
    /// re-clobber that real tracking on every later, unrelated layout. Gating
    /// on `AnimationController::is_animating()` instead does not work: status
    /// flips to `Completed` on the exact same tick the value reaches the
    /// target, which would then skip applying that final tick's value.
    last_synced_animation_value: Option<f32>,
    snap_configuration: Option<FloatingHeaderSnapConfiguration>,
    last_actual_scroll_offset: Option<f32>,
    effective_scroll_offset: Option<f32>,
    /// Pointer/wheel-scrolling bookkeeping (trap #4) — set via
    /// [`Self::update_scroll_start_direction`], never internally driven in
    /// this pass (see module docs).
    last_started_scroll_direction: Option<ScrollDirection>,
    /// Cached return value of `update_geometry`, mirroring `_childPosition`.
    child_position: Option<f32>,
    /// Value-change subscription on `controller`, torn down in `detach`.
    listener_id: Option<ListenerId>,
    _mode: PhantomData<M>,
}

impl<M: FloatingHeaderMode> RenderSliverFloatingHeaderBase<M> {
    /// Creates a floating persistent header. `controller` is optional: pass
    /// `None` when this header will never snap or programmatically expand.
    #[must_use]
    pub fn new(min_extent: f32, max_extent: f32, controller: Option<AnimationController>) -> Self {
        Self {
            core: PersistentHeaderCore::new(min_extent, max_extent, None),
            controller,
            animation: None,
            float_tween: FloatTween::new(0.0, 0.0),
            last_synced_animation_value: None,
            snap_configuration: None,
            last_actual_scroll_offset: None,
            effective_scroll_offset: None,
            last_started_scroll_direction: None,
            child_position: None,
            listener_id: None,
            _mode: PhantomData,
        }
    }

    /// Installs a stretch configuration (builder style).
    #[must_use]
    pub fn with_stretch_configuration(
        mut self,
        stretch: OverScrollHeaderStretchConfiguration,
    ) -> Self {
        self.core.stretch_configuration = Some(stretch);
        self
    }

    /// Installs a snap configuration (builder style).
    #[must_use]
    pub fn with_snap_configuration(mut self, snap: FloatingHeaderSnapConfiguration) -> Self {
        self.snap_configuration = Some(snap);
        self
    }

    /// The current minimum extent.
    #[must_use]
    pub fn min_extent(&self) -> f32 {
        self.core.min_extent
    }

    /// The current maximum extent.
    #[must_use]
    pub fn max_extent(&self) -> f32 {
        self.core.max_extent
    }

    /// Replaces the minimum extent; returns `true` if it changed.
    pub fn set_min_extent(&mut self, min_extent: f32) -> bool {
        self.core.set_min_extent(min_extent)
    }

    /// Replaces the maximum extent; returns `true` if it changed.
    pub fn set_max_extent(&mut self, max_extent: f32) -> bool {
        self.core.set_max_extent(max_extent)
    }

    /// Replaces the stretch configuration; returns `true` if presence changed.
    pub fn set_stretch_configuration(
        &mut self,
        stretch: Option<OverScrollHeaderStretchConfiguration>,
    ) -> bool {
        self.core.set_stretch_configuration(stretch)
    }

    /// Replaces the snap configuration. Inert setter (matches the oracle's
    /// plain-assignment `snapConfiguration` field) — no dirty-marking.
    pub fn set_snap_configuration(&mut self, snap: Option<FloatingHeaderSnapConfiguration>) {
        self.snap_configuration = snap;
    }

    /// The scroll offset currently driving the header's shrink/reveal state
    /// (post-clamp, as tracked by the re-reveal state machine). `None` before
    /// the first layout.
    #[must_use]
    pub fn effective_scroll_offset(&self) -> Option<f32> {
        self.effective_scroll_offset
    }

    /// Records the scroll direction active when the current scroll gesture
    /// started. Mirrors `updateScrollStartDirection` (`:616-620`) — a pure
    /// setter with **no internal caller** in this pass (see module docs); it
    /// exists so a future `Scrollable`/`SliverAppBar` integration (or a test)
    /// can feed [`Self::maybe_start_snap_animation`]'s
    /// `allow_floating_expansion` disjunct (trap #4).
    pub fn update_scroll_start_direction(&mut self, direction: ScrollDirection) {
        self.last_started_scroll_direction = Some(direction);
    }

    /// If the header isn't already fully exposed, scrolls it into view.
    /// Mirrors `maybeStartSnapAnimation` (`:622-641`). A no-op if there is no
    /// [`FloatingHeaderSnapConfiguration`] or no injected controller.
    pub fn maybe_start_snap_animation(&mut self, direction: ScrollDirection) {
        let Some(snap) = self.snap_configuration.clone() else {
            return;
        };
        let effective = self.effective_scroll_offset.unwrap_or(0.0);
        if direction == ScrollDirection::Forward && effective <= 0.0 {
            return;
        }
        if direction == ScrollDirection::Reverse && effective >= self.core.max_extent {
            return;
        }

        let end_value = if direction == ScrollDirection::Forward {
            0.0
        } else {
            self.core.max_extent
        };
        self.update_animation(snap.duration, end_value, snap.curve);
        if let Some(controller) = self.controller.as_ref() {
            let _ = controller.forward_from(Some(0.0));
        }
    }

    /// Stops an in-flight snap (or `show_on_screen` expand) animation.
    /// Mirrors `maybeStopSnapAnimation` (`:643-647`) — the oracle itself never
    /// reads `direction` either; kept for signature parity with a future
    /// caller.
    pub fn maybe_stop_snap_animation(&mut self, _direction: ScrollDirection) {
        if let Some(controller) = self.controller.as_ref() {
            let _ = controller.stop();
        }
    }

    /// Rebuilds `float_tween`/`animation` targeting `end_value` over
    /// `duration` with `curve`, mirroring `_updateAnimation` (`:599-614`).
    ///
    /// Diverges from the oracle in one respect: the oracle's controller is
    /// lazily built on the FIRST call (`_controller ??= AnimationController(
    /// ..., duration: duration)`), so `duration` is silently ignored on every
    /// later call. FLUI's controller is always already-built —
    /// there is no "first creation" moment to gate on, so this method
    /// applies `duration` via `set_duration` on every call instead —
    /// documented divergence, not a silent behavior change.
    fn update_animation(&mut self, duration: Duration, end_value: f32, curve: ArcCurve) {
        let Some(controller) = self.controller.as_ref() else {
            return;
        };
        controller.set_duration(duration);
        let begin = self.effective_scroll_offset.unwrap_or(0.0);
        self.float_tween = FloatTween::new(begin, end_value);
        let parent: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
        self.animation = Some(CurvedAnimation::new(parent, curve));
        // The freshly-built tween's value AT the controller's pre-reset
        // position may not equal `begin` (the controller hasn't been driven
        // back to t=0 yet — that's `forward_from(Some(0.0))`'s job, called
        // by the caller right after this). Seed the sync marker with `begin`
        // itself so the very next `perform_layout` doesn't spuriously see a
        // "changed" value before the controller has actually moved.
        self.last_synced_animation_value = Some(begin);
    }
}

impl<M: FloatingHeaderMode> fmt::Debug for RenderSliverFloatingHeaderBase<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderSliverFloatingHeaderBase")
            .field("min_extent", &self.core.min_extent)
            .field("max_extent", &self.core.max_extent)
            .field("effective_scroll_offset", &self.effective_scroll_offset)
            .field(
                "last_started_scroll_direction",
                &self.last_started_scroll_direction,
            )
            .finish_non_exhaustive()
    }
}

impl<M: FloatingHeaderMode> Diagnosticable for RenderSliverFloatingHeaderBase<M> {
    fn to_diagnostics_node(&self) -> DiagnosticsNode {
        let mut node = DiagnosticsNode::new(M::DIAGNOSTIC_NAME);
        let mut builder = DiagnosticsBuilder::new();
        self.debug_fill_properties(&mut builder);
        *node.properties_mut() = builder.build();
        node
    }

    fn debug_fill_properties(&self, builder: &mut DiagnosticsBuilder) {
        builder.add("min_extent", self.core.min_extent);
        builder.add("max_extent", self.core.max_extent);
        if let Some(offset) = self.effective_scroll_offset {
            builder.add("effective_scroll_offset", offset);
        }
    }
}

impl<M: FloatingHeaderMode> RenderSliver for RenderSliverFloatingHeaderBase<M> {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    /// The re-reveal state machine — mirrors `performLayout`
    /// (`:649-689`) exactly, including traps #2, #3, and #4 (see module
    /// docs).
    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let max_extent = self.core.max_extent;

        // Mirrors the oracle's animation-value listener (`_controller
        // .addListener` writes `_effectiveScrollOffset = _animation.value` on
        // every tick, before the next `performLayout` runs, guarded by
        // `if (_effectiveScrollOffset == _animation.value) return;`,
        // `:601-603`). FLUI's `attach` listener can only call
        // `mark_needs_layout` — ADR-0013's dirty handle grants no `&mut self`
        // access from inside the `Arc<dyn Fn>` callback — so the mirrored
        // read+guard happens here instead, at the top of the very layout that
        // listener's `mark_needs_layout` triggered. `last_synced_animation_value`
        // (not `is_animating()`) is the guard: it changes on every tick
        // (including the final tick that also flips status to `Completed`,
        // which `is_animating()` would already report `false` for — a subtly
        // different, wrong gate), and stops changing the instant the drive
        // settles, so later unrelated real-scroll-driven layouts never see a
        // "changed" value and never get clobbered.
        if let Some(animation) = self.animation.as_ref() {
            let value = self.float_tween.transform(animation.value());
            if self.last_synced_animation_value != Some(value) {
                self.effective_scroll_offset = Some(value);
                self.last_synced_animation_value = Some(value);
            }
        }

        // Trap #3: the outer gate is a conjunction with history — "have we
        // laid out before, AND (scrolling backward OR already partially
        // revealed)". Both disjuncts of the inner condition matter.
        if let Some(last_actual) = self.last_actual_scroll_offset
            && (constraints.scroll_offset < last_actual
                || self.effective_scroll_offset.unwrap_or(0.0) < max_extent)
        {
            let mut delta = last_actual - constraints.scroll_offset;
            // Trap #4: two disjuncts, not one. The second exists specifically
            // for pointer/wheel scrolling, which has no "hold and release"
            // concept the way a drag gesture does.
            let allow_floating_expansion = constraints.user_scroll_direction
                == ScrollDirection::Forward
                || self.last_started_scroll_direction == Some(ScrollDirection::Forward);
            let mut effective = self.effective_scroll_offset.unwrap_or(0.0);
            if allow_floating_expansion {
                if effective > max_extent {
                    effective = max_extent;
                }
            } else if delta > 0.0 {
                delta = 0.0;
            }
            self.effective_scroll_offset =
                Some((effective - delta).clamp(0.0, constraints.scroll_offset));
        } else {
            self.effective_scroll_offset = Some(constraints.scroll_offset);
        }

        let effective_scroll_offset = self.effective_scroll_offset.unwrap_or(0.0);
        let overlaps_content = effective_scroll_offset < constraints.scroll_offset;

        let child_extent = self.core.layout_child(
            ctx,
            &constraints,
            effective_scroll_offset,
            overlaps_content,
            |_, _| {},
        );
        let core_view = PersistentHeaderCoreView { core: &self.core };
        let (geometry, child_position) = M::update_geometry(
            &core_view,
            &constraints,
            effective_scroll_offset,
            child_extent,
        );
        self.child_position = Some(child_position);
        self.last_actual_scroll_offset = Some(constraints.scroll_offset);

        position_persistent_header_child(
            ctx,
            &constraints,
            &geometry,
            child_position,
            child_extent,
        );
        geometry
    }

    fn child_main_axis_position(
        &self,
        _constraints: &SliverConstraints,
        _child: &dyn RenderObject<SliverProtocol>,
    ) -> f32 {
        self.child_position.unwrap_or(0.0)
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        ctx.paint_child();
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Single, Self::ParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }

    fn attach(&mut self, handle: RepaintHandle) {
        if let Some(controller) = self.controller.as_ref() {
            let mark_handle = handle.clone();
            self.listener_id = Some(controller.add_listener(Arc::new(move || {
                let _ = mark_handle.mark_needs_layout();
            })));
        }
    }

    fn detach(&mut self) {
        // Deliberately does NOT stop/dispose `self.controller` — the same
        // FLUI divergence `RenderAnimatedSize::detach` documents: FLUI's
        // `detach` fires only on structural tree removal (not Flutter's
        // far-more-frequent offstage/onstage toggling), and controller
        // lifecycle belongs to the owning widget/`State`, not this render
        // object. Stopping here would also race a fresh `attach` on a
        // remove+insert reparent.
        if let Some(controller) = self.controller.as_ref()
            && let Some(id) = self.listener_id.take()
        {
            controller.remove_listener(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_animation::Scheduler;
    use flui_rendering::testing::sliver;

    fn controller(ms: u64) -> AnimationController {
        AnimationController::new(Duration::from_millis(ms), Arc::new(Scheduler::new()))
    }

    fn vertical_constraints(scroll_offset: f32, remaining_paint_extent: f32) -> SliverConstraints {
        sliver::vertical()
            .scroll_offset(scroll_offset)
            .remaining_paint_extent(remaining_paint_extent)
            .cross_axis_extent(300.0)
            .viewport_main_axis_extent(remaining_paint_extent)
            .remaining_cache_extent(remaining_paint_extent)
            .build()
    }

    // ---- PersistentHeaderCore pure formulas --------------------------------
    //
    // `layout_child`'s own change-detection guard and edge-triggered stretch
    // trigger signal (traps #1, #6, #7) need a live `SliverLayoutContext` to drive
    // the child's box layout, so those are proven end-to-end in
    // `render_object_harness.rs` (`harness_sliver_persistent_header_stretch_*`)
    // rather than re-derived by hand here.

    #[test]
    fn stretch_trigger_signal_is_data_plane_and_clone_shared() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<StretchTriggerSignal>();

        let signal = StretchTriggerSignal::new();
        let cloned = signal.clone();
        assert_eq!(signal.count(), 0);

        cloned.notify();

        assert_eq!(
            signal.count(),
            1,
            "stretch trigger signal clones share the same data-plane counter"
        );
    }

    #[test]
    fn stretch_offset_for_geometry_ignores_scroll_offset_unlike_layout_child() {
        // Trap #7 regression: update_geometry's stretch offset must NOT gate
        // on scroll_offset == 0.0 (layout_child's does).
        let core = PersistentHeaderCore::new(
            40.0,
            120.0,
            Some(OverScrollHeaderStretchConfiguration::new(50.0, None)),
        );
        let constraints = vertical_constraints(10.0, 400.0).with_overlap(-30.0);
        assert_eq!(
            core.stretch_offset_for_geometry(&constraints),
            30.0,
            "update_geometry's stretch offset must fire even with scroll_offset > 0.0"
        );
    }

    // ---- Scrolling: hand-computed formulas at several scroll offsets -------

    #[test]
    fn scrolling_header_shrinks_then_scrolls_off() {
        let header = RenderSliverScrollingPersistentHeader::new(40.0, 120.0);

        // At scroll_offset=0: fully expanded, paint_extent = max_extent.
        let c0 = vertical_constraints(0.0, 400.0);
        let (g0, pos0) = header.update_geometry(&c0, 120.0);
        assert_eq!(g0.paint_extent, 120.0);
        assert_eq!(pos0, 0.0);
        assert!(g0.has_visual_overflow);

        // Mid-shrink: scroll_offset=60 (still above min_extent=40 remaining).
        let c1 = vertical_constraints(60.0, 400.0);
        let (g1, _) = header.update_geometry(&c1, 60.0);
        assert_eq!(
            g1.paint_extent, 60.0,
            "paint_extent = max_extent - scroll_offset"
        );

        // Past max_extent: scrolled fully off, paint_extent clamps to 0.
        let c2 = vertical_constraints(200.0, 400.0);
        let (g2, _) = header.update_geometry(&c2, 40.0);
        assert_eq!(g2.paint_extent, 0.0, "clamped to 0 once scrolled fully off");
    }

    // ---- Pinned: config setters ---------------------------------------------
    //
    // `max_scroll_obstruction_extent` reporting (trap #5) and the pinned
    // child-position/paint-extent contract need a live layout pass — proven
    // in `render_object_harness.rs`'s two-sliver `viewport_multi` test.

    #[test]
    fn pinned_header_set_max_extent_reports_change_flag() {
        let mut header = RenderSliverPinnedPersistentHeader::new(40.0, 120.0);
        assert_eq!(header.min_extent(), 40.0);
        assert!(header.set_max_extent(150.0));
        assert!(!header.set_max_extent(150.0), "no-op set reports unchanged");
        assert_eq!(header.max_extent(), 150.0);
    }

    // ---- Floating: re-reveal sequence (traps #3, #4) live in the harness ---
    //
    // The re-reveal state machine reads/writes `self.effective_scroll_offset`
    // and needs a live `SliverLayoutContext` (`perform_layout` lays out the
    // child through it), so the multi-pass scroll sequence is proven
    // end-to-end in `render_object_harness.rs`
    // (`harness_sliver_persistent_header_floating_*`) rather than re-derived
    // by hand here. The tests below cover the pieces that genuinely don't
    // need a layout context: the sealed mode formulas and the
    // controller-driving methods.

    #[test]
    fn floating_pinned_child_position_is_always_zero_even_mid_reveal() {
        let core = PersistentHeaderCore::new(40.0, 120.0, None);
        let view = PersistentHeaderCoreView { core: &core };
        let constraints = vertical_constraints(60.0, 400.0);
        let (_, position) = FloatingPinnedMode::update_geometry(&view, &constraints, 30.0, 90.0);
        assert_eq!(
            position, 0.0,
            "FloatingPinned's child_main_axis_position is always 0.0, unlike plain Floating"
        );
    }

    #[test]
    fn floating_pinned_paint_extent_never_drops_below_min_extent_at_full_shrink() {
        let core = PersistentHeaderCore::new(40.0, 120.0, None);
        let view = PersistentHeaderCoreView { core: &core };
        // Fully shrunk: effective_scroll_offset == max_extent.
        let constraints = vertical_constraints(120.0, 400.0);
        let (geometry, _) = FloatingPinnedMode::update_geometry(&view, &constraints, 120.0, 40.0);
        assert_eq!(
            geometry.paint_extent, 40.0,
            "even at full shrink, FloatingPinned keeps at least min_extent visible"
        );
    }

    #[test]
    fn maybe_start_snap_animation_is_inert_without_snap_configuration() {
        let ctl = controller(100);
        let mut header: RenderSliverFloatingPersistentHeader =
            RenderSliverFloatingHeaderBase::new(40.0, 120.0, Some(ctl.clone()));
        header.effective_scroll_offset = Some(60.0);
        header.maybe_start_snap_animation(ScrollDirection::Reverse);
        assert!(
            !ctl.is_animating(),
            "no snap_configuration means maybe_start_snap_animation is a no-op"
        );
    }

    #[test]
    fn maybe_start_snap_animation_forward_ignored_when_already_fully_revealed() {
        let ctl = controller(100);
        let mut header: RenderSliverFloatingPersistentHeader =
            RenderSliverFloatingHeaderBase::new(40.0, 120.0, Some(ctl.clone()))
                .with_snap_configuration(FloatingHeaderSnapConfiguration::new(
                    ArcCurve::new(Curves::Linear),
                    Duration::from_millis(50),
                ));
        header.effective_scroll_offset = Some(0.0);
        header.maybe_start_snap_animation(ScrollDirection::Forward);
        assert!(
            !ctl.is_animating(),
            "already at effective_scroll_offset <= 0.0, forward snap has nothing to do"
        );
    }

    #[test]
    fn maybe_start_snap_animation_drives_controller_toward_target() {
        let ctl = controller(100);
        let mut header: RenderSliverFloatingPersistentHeader =
            RenderSliverFloatingHeaderBase::new(40.0, 120.0, Some(ctl.clone()))
                .with_snap_configuration(FloatingHeaderSnapConfiguration::new(
                    ArcCurve::new(Curves::Linear),
                    Duration::from_millis(50),
                ));
        header.effective_scroll_offset = Some(60.0);
        header.maybe_start_snap_animation(ScrollDirection::Reverse);
        assert!(
            ctl.is_animating(),
            "reverse snap toward max_extent must start the controller"
        );
        header.maybe_stop_snap_animation(ScrollDirection::Reverse);
        assert!(
            !ctl.is_animating(),
            "maybe_stop_snap_animation must stop it"
        );
    }

    #[test]
    fn update_scroll_start_direction_feeds_allow_floating_expansion_disjunct() {
        // Trap #4 regression: this setter has no internal caller, but must be
        // usable to seed the second disjunct directly.
        let mut header: RenderSliverFloatingPersistentHeader =
            RenderSliverFloatingHeaderBase::new(40.0, 120.0, None);
        assert_eq!(header.last_started_scroll_direction, None);
        header.update_scroll_start_direction(ScrollDirection::Forward);
        assert_eq!(
            header.last_started_scroll_direction,
            Some(ScrollDirection::Forward)
        );
    }
}
