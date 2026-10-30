//! [`RenderSliverCarousel`]: lazily built items at one large extent, each masked to the
//! keyline it passes.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::rc::Rc;

use flui_foundation::Variable;
use flui_foundation::geometry::{Axis, Offset, Rect, Size};
use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
use flui_painting::paint::Clip;
use flui_rendering::{
    constraints::{SliverConstraints, SliverGeometry, child_paint_offset},
    context::{PaintCx, SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverMultiBoxAdaptorParentData,
    protocol::ChildLayout,
    traits::{RenderSliver, SemanticsClip},
};

use super::geometry::{CarouselGeometry, mask_rect};
use super::layout::CarouselLayout;
use crate::sliver::pinned::{PinnedChildren, PinnedPlace};

/// Where each laid-out item of a carousel was last masked, for the item's own content to
/// read: a parallax image shifts by the item's [`progress`](Self::progress), a caption fades
/// with its [`mask`](Self::mask).
///
/// The sliver writes it in layout and the values are read in paint or after, never written by
/// anyone else; a clone shares the same record. An item that is not laid out has no entry.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct CarouselItemFrames {
    frames: Rc<RefCell<HashMap<usize, ItemFrame>>>,
    initial: Rc<Cell<Option<usize>>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ItemFrame {
    mask: Rect,
    progress: f64,
}

impl fmt::Debug for CarouselItemFrames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CarouselItemFrames")
            .field("items", &self.frames.borrow().len())
            .field("initial", &self.initial.get())
            .finish()
    }
}

impl CarouselItemFrames {
    /// A record no sliver has written yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The shown part of item `index`, in the item's own coordinates.
    #[must_use]
    pub fn mask(&self, index: usize) -> Option<Rect> {
        self.frames.borrow().get(&index).map(|frame| frame.mask)
    }

    /// How far item `index` is from the keyline before it to the one after, in `[0, 1]`.
    #[must_use]
    pub fn progress(&self, index: usize) -> Option<f64> {
        self.frames.borrow().get(&index).map(|frame| frame.progress)
    }

    /// The next layout opens the carousel on item `index` (the last item past the end), once,
    /// instead of the offset the scroll position holds: the start a controller asks for
    /// before the carousel is laid out.
    pub fn open_at(&self, index: usize) {
        self.initial.set(Some(index));
    }

    fn replace(&self, frames: impl Iterator<Item = (usize, ItemFrame)>) {
        let mut record = self.frames.borrow_mut();
        record.clear();
        record.extend(frames);
    }
}

/// What a [`RenderSliverCarousel`] is configured with.
#[derive(Debug, Clone, PartialEq)]
pub struct CarouselConfig {
    /// How items are sized.
    pub layout: CarouselLayout,
    /// The gap between items, logical pixels; finite and not negative.
    pub spacing: f64,
}

impl CarouselConfig {
    /// `layout` with `spacing` between items; a spacing that is not finite or negative is zero.
    #[must_use]
    pub fn new(layout: CarouselLayout, spacing: f64) -> Self {
        let spacing = if spacing.is_finite() && spacing > 0.0 {
            spacing.min(super::layout::MAX_ITEM_EXTENT)
        } else {
            0.0
        };
        Self { layout, spacing }
    }
}

/// One laid-out item, as paint and hit testing need it.
#[derive(Debug, Clone, Copy)]
struct Shown {
    slot: usize,
    /// The child's paint offset in the sliver.
    offset: Offset,
    /// Its distance from the viewport's leading edge, along the scroll axis.
    main: f64,
    mask: Rect,
    mask_extent: f64,
}

impl Shown {
    /// The mask in the sliver's coordinates.
    fn clip(&self) -> Rect {
        Rect::from_ltrb(
            self.mask.left() + self.offset.dx,
            self.mask.top() + self.offset.dy,
            self.mask.right() + self.offset.dx,
            self.mask.bottom() + self.offset.dy,
        )
    }
}

/// A carousel sliver: items laid out at the layout's large extent along a horizontal or
/// vertical scroll axis, each clipped to the size of the keyline it passes.
///
/// The element tree builds the items: the sliver lays out those whose centre is between the
/// first and last keylines, one more on each side, requests the absent ones and retains that
/// window. A tap lands on an item only inside its mask, and the item's semantics are clipped
/// to the mask, so neither finds an item where it is not drawn. The largest masks paint in
/// front.
///
/// The keylines need a bounded viewport: under an unbounded main axis (a shrink-wrapping
/// viewport) the sliver publishes zero geometry and logs a warning once, instead of deriving
/// an infinite extent.
#[derive(Debug, Clone)]
pub struct RenderSliverCarousel {
    config: CarouselConfig,
    item_count: usize,
    frames: CarouselItemFrames,
    /// The geometry of the last pass, kept while viewport, configuration and count stay.
    geometry: Option<(f64, CarouselGeometry)>,
    logical_to_slot: BTreeMap<usize, usize>,
    /// Back to front.
    shown: Vec<Shown>,
    axis: Axis,
    warned_unbounded: bool,
    /// The accessible set the items announce, when it differs from the item count.
    collection: Option<flui_rendering::semantics::SemanticsCollection>,
    /// The pinned residents outside the window (`set_pinned_indices`).
    pinned: PinnedChildren,
}

impl RenderSliverCarousel {
    /// A carousel of `item_count` items configured by `config`, recording each item's mask in
    /// `frames`.
    #[must_use]
    pub fn new(config: CarouselConfig, item_count: usize, frames: CarouselItemFrames) -> Self {
        Self {
            config,
            item_count,
            frames,
            geometry: None,
            logical_to_slot: BTreeMap::new(),
            shown: Vec::new(),
            axis: Axis::Horizontal,
            warned_unbounded: false,
            collection: None,
            pinned: PinnedChildren::default(),
        }
    }

    /// The configuration.
    #[must_use]
    pub fn config(&self) -> &CarouselConfig {
        &self.config
    }

    /// Replaces the configuration.
    pub fn set_config(&mut self, config: CarouselConfig) -> flui_rendering::RenderUpdateImpact {
        if self.config == config {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.config = config;
        self.geometry = None;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// The record each pass writes the items' masks into.
    #[must_use]
    pub fn frames(&self) -> &CarouselItemFrames {
        &self.frames
    }

    /// Records the items' masks in `frames` from the next pass on.
    pub fn set_frames(&mut self, frames: CarouselItemFrames) -> flui_rendering::RenderUpdateImpact {
        if Rc::ptr_eq(&self.frames.frames, &frames.frames) {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.frames = frames;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Overrides the accessible set size rather than deriving it from the item count. `None`
    /// declares an unknown total.
    pub fn set_semantic_set_size(
        &mut self,
        count: Option<usize>,
    ) -> flui_rendering::RenderUpdateImpact {
        use flui_rendering::semantics::{SemanticsCollection, SemanticsCollectionKind};
        let collection = SemanticsCollection::new(SemanticsCollectionKind::List, count);
        if self.collection == Some(collection) {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.collection = Some(collection);
        flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// The logical indices of pinned residents outside the window. Each is
    /// laid out where the keylines place it and announced, but neither painted
    /// nor hit-tested (ADR-0265).
    pub fn set_pinned_indices(
        &mut self,
        pinned: &std::collections::BTreeSet<usize>,
    ) -> flui_rendering::RenderUpdateImpact {
        self.pinned.set(pinned)
    }

    /// The number of items.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// Sets the number of items.
    pub fn set_item_count(&mut self, item_count: usize) -> flui_rendering::RenderUpdateImpact {
        if self.item_count == item_count {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.item_count = item_count;
        self.geometry = None;
        flui_rendering::RenderUpdateImpact::LAYOUT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// The geometry for a `viewport` long main axis, reusing the last one when it fits.
    fn geometry_for(&mut self, viewport: f64) -> Option<CarouselGeometry> {
        if let Some((cached, geometry)) = &self.geometry
            && *cached == viewport
            && geometry.count() == self.item_count
        {
            return Some(geometry.clone());
        }
        let geometry = CarouselGeometry::new(
            &self.config.layout,
            viewport,
            self.config.spacing,
            self.item_count,
        )?;
        self.geometry = Some((viewport, geometry.clone()));
        Some(geometry)
    }

    fn nothing(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, SliverMultiBoxAdaptorParentData>,
    ) -> SliverGeometry {
        self.shown.clear();
        self.pinned.clear_slots();
        self.frames.replace(std::iter::empty());
        ctx.emit_retain_band(0, 0);
        SliverGeometry::new(0.0, 0.0, 0.0)
    }
}

/// The viewport extent the keylines are measured against, or `None` when the main axis is
/// unbounded.
fn bounded_viewport(constraints: &SliverConstraints) -> Option<f64> {
    let viewport = constraints.viewport_main_axis_extent;
    (viewport.is_finite()
        && viewport > 0.0
        && constraints.remaining_paint_extent.is_finite()
        && constraints.scroll_offset.is_finite())
    .then_some(viewport)
}

impl Diagnosticable for RenderSliverCarousel {
    fn debug_fill_properties(&self, properties: &mut DiagnosticsBuilder) {
        properties.add_int("item_count", self.item_count as i64, None);
        properties.add_double("spacing", self.config.spacing, Some("px"));
        properties.add_int("shown", self.shown.len() as i64, None);
    }
}

impl RenderSliver for RenderSliverCarousel {
    type Arity = Variable;
    type ParentData = SliverMultiBoxAdaptorParentData;

    fn describe_semantics_configuration(
        &self,
        config: &mut flui_rendering::semantics::SemanticsConfiguration,
    ) {
        use flui_rendering::semantics::{SemanticsCollection, SemanticsCollectionKind};
        config.set_lazy_collection(self.collection.unwrap_or_else(|| {
            SemanticsCollection::new(SemanticsCollectionKind::List, Some(self.item_count))
        }));
    }

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> flui_rendering::RenderResult<SliverGeometry> {
        let constraints = *ctx.constraints();
        self.axis = constraints.axis_direction.axis();
        let Some(viewport) = bounded_viewport(&constraints) else {
            if !self.warned_unbounded {
                self.warned_unbounded = true;
                tracing::warn!(
                    viewport = constraints.viewport_main_axis_extent,
                    remaining_paint_extent = constraints.remaining_paint_extent,
                    scroll_offset = constraints.scroll_offset,
                    "a carousel needs a bounded scroll axis to place its keylines; it shows \
                     nothing under an unbounded one (a shrink-wrapping viewport)"
                );
            }
            return Ok(self.nothing(ctx));
        };
        if self.item_count == 0 {
            return Ok(self.nothing(ctx));
        }
        let Some(geometry) = self.geometry_for(viewport) else {
            return Ok(self.nothing(ctx));
        };
        if let Some(index) = self.frames.initial.take() {
            let correction = geometry.rest_offset(index) - constraints.scroll_offset;
            if correction.abs() > flui_foundation::EPSILON {
                return Ok(SliverGeometry::scroll_offset_correction(correction));
            }
        }
        let offset = constraints.scroll_offset.max(0.0);
        let keylines = geometry.keylines_at(offset);
        let Some((first, mut last)) = geometry.window(&keylines, offset) else {
            return Ok(self.nothing(ctx));
        };

        self.logical_to_slot.clear();
        for slot in 0..ctx.child_count() {
            if let Some(data) = ctx.child_parent_data(slot) {
                self.logical_to_slot.insert(data.index, slot);
            }
        }
        let extent = geometry.item_extent();
        let child_constraints = constraints.as_box_constraints(extent, extent, None);
        let mut placed = Vec::new();
        for index in first..=last {
            if let Some(&slot) = self.logical_to_slot.get(&index) {
                ctx.layout_box_child(slot, child_constraints)?;
                let placement = geometry.place_with(&keylines, index, offset);
                let layout_offset = offset + placement.center - extent / 2.0;
                if let Some(data) = ctx.child_parent_data_mut(slot) {
                    data.layout_offset = layout_offset;
                }
                placed.push((index, slot, layout_offset, placement));
                continue;
            }
            match ctx.request_child_build(index) {
                ChildLayout::NoChild => {
                    self.item_count = index;
                    last = index.saturating_sub(1);
                    break;
                }
                ChildLayout::Unwired => break,
                _ => {}
            }
        }
        ctx.emit_retain_band(first, last + 1);

        let scroll_extent = geometry.max_offset() + viewport;
        let paint_extent = self.calculate_paint_offset(&constraints, 0.0, scroll_extent);
        let cache_extent = self.calculate_cache_offset(&constraints, 0.0, scroll_extent);
        let sliver = SliverGeometry::new(scroll_extent, paint_extent, 0.0)
            .with_max_paint_extent(scroll_extent)
            .with_cache_extent(cache_extent)
            .with_visual_overflow();

        let cross = constraints.cross_axis_extent;
        let horizontal = self.axis == Axis::Horizontal;
        let mut shown = Vec::with_capacity(placed.len());
        let mut frames = Vec::with_capacity(placed.len());
        for (index, slot, layout_offset, placement) in placed {
            let paint_offset = child_paint_offset(&constraints, &sliver, layout_offset, extent);
            ctx.position_child(slot, paint_offset);
            let mask = mask_rect(extent, placement.mask_extent, cross, horizontal);
            frames.push((
                index,
                ItemFrame {
                    mask,
                    progress: placement.progress,
                },
            ));
            shown.push(Shown {
                slot,
                offset: paint_offset,
                main: layout_offset - offset,
                mask,
                mask_extent: placement.mask_extent,
            });
        }
        shown.sort_by(|a, b| a.mask_extent.total_cmp(&b.mask_extent));
        self.shown = shown;
        self.frames.replace(frames.into_iter());

        // A pinned item is placed where it rests on the large keyline: the
        // keylines at the current offset park every item outside them at an
        // edge, which is no place to reveal it from.
        let item_count = self.item_count;
        self.pinned.lay_out(first..last + 1, ctx, |index| {
            (index < item_count).then(|| {
                let rest = geometry.rest_offset(index);
                let placement = geometry.place(index, rest);
                let layout_offset = rest + placement.center - extent / 2.0;
                PinnedPlace {
                    constraints: child_constraints,
                    layout_offset,
                    paint_offset: child_paint_offset(&constraints, &sliver, layout_offset, extent),
                }
            })
        })?;
        Ok(sliver)
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        for item in &self.shown {
            if item.mask_extent <= 0.0 {
                continue;
            }
            ctx.with_clip_rect(item.clip(), Clip::HardEdge, |ctx| {
                ctx.paint_child(item.slot);
            });
        }
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        let extent = self
            .geometry
            .as_ref()
            .map_or(0.0, |(_, geometry)| geometry.item_extent());
        for item in self.shown.iter().rev() {
            if item.mask_extent <= 0.0 {
                continue;
            }
            let start = item.main + (extent - item.mask_extent) / 2.0;
            if ctx.is_within_main_axis_range(start, start + item.mask_extent)
                && ctx.hit_test_child_at_layout_offset(item.slot)
            {
                return true;
            }
        }
        false
    }

    /// A shown item with a visible mask, or a pinned child: paint and hit
    /// testing go through the shown items alone, so a pinned child is
    /// announced without being drawn or reached.
    fn visits_child_for_semantics(&self, child_slot: usize, _size: Size) -> bool {
        self.pinned.is_pinned_slot(child_slot)
            || self
                .shown
                .iter()
                .any(|item| item.slot == child_slot && item.mask_extent > 0.0)
    }

    fn describe_approximate_paint_clip(&self, child_slot: usize, _size: Size) -> Option<Rect> {
        self.shown
            .iter()
            .find(|item| item.slot == child_slot)
            .map(Shown::clip)
    }

    fn describe_semantics_clip(&self, child_slot: usize, _size: Size) -> Option<SemanticsClip> {
        self.shown
            .iter()
            .find(|item| item.slot == child_slot)
            .map(|item| SemanticsClip::Bounds(item.clip()))
    }
}
