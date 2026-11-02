//! `RenderToolbarItems` — a toolbar's commands in one row, the trailing
//! ones that don't fit behind an overflow button.
//!
//! # Layout
//!
//! Every child but the last is an item; the last is the overflow button.
//! All children are laid out once a frame, loosely, at their own width under
//! the incoming maximum. The row takes the longest leading run of items
//! that fits the maximum width; when not every item fits, the button's width
//! is held back so the run and the button fit together, and the run keeps
//! at least its first item, clipped when even that and the button are too
//! wide, so a lone command stays reachable.
//!
//! Collapsed, the run and then the button (when items are left over) stand
//! in a row in reading order, as tall as the tallest of them. Expanded, the
//! button and then the left-over items stand in a column as wide as the
//! widest, each laid out again at that width so its whole row answers a
//! touch. With nothing left over there is no button and expanding changes
//! nothing.
//!
//! # Hidden children
//!
//! A child the layout leaves out is not painted, not hit-tested and not
//! visited for semantics, so assistive technology finds exactly the
//! commands that show. That is what a `RenderWrap`, an offstage child or a
//! layout delegate cannot do: what shows is known only once the children are
//! measured.
//!
//! # Report
//!
//! Each layout writes how many items the row holds to the
//! [`ToolbarItemsSplit`] it was given, so the widget above can offer the
//! left-over items its own way (a design package's popup) once the frame is
//! laid out.

use std::cell::Cell;
use std::fmt;
use std::ops::Range;
use std::rc::Rc;

use flui_foundation::Variable;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_painting::{paint::Clip, typography::TextDirection};
use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext, PaintCx},
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

/// How many items the last layout of a [`RenderToolbarItems`] put in its
/// row; the rest are behind its overflow button.
///
/// The render object writes it in layout and it is read after, never
/// written by anyone else; a clone shares the same record.
#[derive(Clone, Default)]
pub struct ToolbarItemsSplit {
    /// The row length and the item count of the last layout.
    record: Rc<Cell<Option<(usize, usize)>>>,
}

impl ToolbarItemsSplit {
    /// A record no layout has written yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many leading items the row holds, `None` before the first layout.
    #[must_use]
    pub fn row_len(&self) -> Option<usize> {
        self.record.get().map(|(row_len, _)| row_len)
    }

    /// The items behind the overflow button, by index: empty before the
    /// first layout and while every item fits.
    #[must_use]
    pub fn hidden(&self) -> Range<usize> {
        self.record
            .get()
            .map_or(0..0, |(row_len, items)| row_len..items)
    }

    /// Whether `self` and `other` are one record.
    #[must_use]
    pub fn is_same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.record, &other.record)
    }
}

impl fmt::Debug for ToolbarItemsSplit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolbarItemsSplit")
            .field("row_len", &self.row_len())
            .field("items", &self.record.get().map(|(_, items)| items))
            .finish()
    }
}

/// How many leading items of `widths` fit in `max` beside nothing, or
/// beside a `button` wide when not all of them do; at least one while there
/// is one.
fn row_len(widths: &[f64], button: f64, max: f64) -> usize {
    let total = widths.iter().sum::<f64>();
    if total <= max + flui_foundation::EPSILON {
        return widths.len();
    }
    let room = max - button;
    let mut used = 0.0;
    let mut len = 0;
    for width in widths {
        if used + width > room + flui_foundation::EPSILON {
            break;
        }
        used += width;
        len += 1;
    }
    len.max(1).min(widths.len())
}

/// What one layout decided: its size, where each shown child goes, how many
/// items the row holds and whether the content overflows the size.
struct Arrangement {
    size: Size,
    placed: Vec<(usize, Offset)>,
    row_len: usize,
    clipped: bool,
}

/// A toolbar's items in one row, those that don't fit behind an overflow
/// button that shows them in a column instead: see the module docs.
#[derive(Debug, Clone)]
pub struct RenderToolbarItems {
    expanded: bool,
    text_direction: TextDirection,
    split: ToolbarItemsSplit,
    /// The children the last layout counted.
    child_count: usize,
    /// How many items the last layout put in the row.
    row_len: usize,
    /// Whether the last layout's content is bigger than its size.
    clipped: bool,
}

impl RenderToolbarItems {
    /// A collapsed, left-to-right row reporting to `split`.
    #[must_use]
    pub fn new(split: ToolbarItemsSplit) -> Self {
        Self {
            expanded: false,
            text_direction: TextDirection::Ltr,
            split,
            child_count: 0,
            row_len: 0,
            clipped: false,
        }
    }

    /// Builder: shows the left-over items in place of the row.
    #[must_use]
    pub fn with_expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Builder: the reading direction the row follows.
    #[must_use]
    pub fn with_text_direction(mut self, text_direction: TextDirection) -> Self {
        self.text_direction = text_direction;
        self
    }

    /// Whether the left-over items show in place of the row.
    #[must_use]
    pub fn expanded(&self) -> bool {
        self.expanded
    }

    /// The reading direction the row follows.
    #[must_use]
    pub fn text_direction(&self) -> TextDirection {
        self.text_direction
    }

    /// How many items the last layout put in the row.
    #[must_use]
    pub fn row_len(&self) -> usize {
        self.row_len
    }

    /// Shows the left-over items in place of the row, or the row again.
    pub fn set_expanded(&mut self, expanded: bool) -> flui_rendering::RenderUpdateImpact {
        if self.expanded == expanded {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.expanded = expanded;
        flui_rendering::RenderUpdateImpact::LAYOUT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Sets the reading direction the row follows.
    pub fn set_text_direction(
        &mut self,
        text_direction: TextDirection,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.text_direction == text_direction {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.text_direction = text_direction;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Reports the next layouts to `split`.
    pub fn set_split(&mut self, split: ToolbarItemsSplit) -> flui_rendering::RenderUpdateImpact {
        if self.split.is_same(&split) {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        split.record.set(self.split.record.get());
        self.split = split;
        flui_rendering::RenderUpdateImpact::NONE
    }

    /// The items, without the button.
    fn item_count(child_count: usize) -> usize {
        child_count.saturating_sub(1)
    }

    /// Whether the last layout left items over.
    fn overflowing(&self) -> bool {
        self.row_len < Self::item_count(self.child_count)
    }

    /// Whether the last layout shows the column of left-over items.
    fn shows_column(&self) -> bool {
        self.expanded && self.overflowing()
    }

    /// Whether child `slot` shows after the last layout.
    fn shows(&self, slot: usize) -> bool {
        let items = Self::item_count(self.child_count);
        if slot >= self.child_count {
            return false;
        }
        if slot == items {
            return self.overflowing();
        }
        if self.shows_column() {
            slot >= self.row_len
        } else {
            slot < self.row_len
        }
    }

    /// Lays `count` children out through `layout` in `constraints` and says
    /// where the shown ones go: the one algorithm of live and dry layout.
    fn arrange(
        &self,
        constraints: BoxConstraints,
        count: usize,
        mut layout: impl FnMut(usize, BoxConstraints) -> flui_rendering::RenderResult<Size>,
    ) -> flui_rendering::RenderResult<Arrangement> {
        if count == 0 {
            return Ok(Arrangement {
                size: constraints.smallest(),
                placed: Vec::new(),
                row_len: 0,
                clipped: false,
            });
        }
        let items = Self::item_count(count);
        let loose = BoxConstraints::new(0.0, constraints.max_width, 0.0, constraints.max_height);
        let mut sizes = Vec::with_capacity(count);
        for i in 0..count {
            sizes.push(layout(i, loose)?);
        }
        let widths = sizes[..items]
            .iter()
            .map(|size| size.width)
            .collect::<Vec<f64>>();
        let row_len = row_len(&widths, sizes[items].width, constraints.max_width);
        let overflowing = row_len < items;

        if self.expanded && overflowing {
            let shown = std::iter::once(items)
                .chain(row_len..items)
                .collect::<Vec<usize>>();
            let widest = shown
                .iter()
                .map(|&i| sizes[i].width)
                .fold(0.0_f64, f64::max);
            let width = constraints.constrain_width(widest);
            let across = BoxConstraints::new(width, width, 0.0, constraints.max_height);
            let mut placed = Vec::with_capacity(shown.len());
            let mut y = 0.0;
            for i in shown {
                let size = layout(i, across)?;
                placed.push((i, Offset::new(0.0, y)));
                y += size.height;
            }
            let size = constraints.constrain(Size::new(width, y));
            return Ok(Arrangement {
                size,
                placed,
                row_len,
                clipped: y > size.height || widest > size.width,
            });
        }

        let mut shown = (0..row_len).collect::<Vec<usize>>();
        if overflowing {
            shown.push(items);
        }
        let content = shown.iter().map(|&i| sizes[i].width).sum::<f64>();
        let height = shown
            .iter()
            .map(|&i| sizes[i].height)
            .fold(0.0_f64, f64::max);
        let size = constraints.constrain(Size::new(content, height));
        let mut placed = Vec::with_capacity(shown.len());
        let mut x = 0.0;
        for i in shown {
            let child = sizes[i];
            let dx = match self.text_direction {
                TextDirection::Ltr => x,
                TextDirection::Rtl => size.width - x - child.width,
            };
            placed.push((i, Offset::new(dx, (size.height - child.height) / 2.0)));
            x += child.width;
        }
        Ok(Arrangement {
            size,
            placed,
            row_len,
            clipped: content > size.width || height > size.height,
        })
    }

    /// The row length a layout at `width` would choose, from the children's
    /// maximum intrinsic widths at `height`.
    fn intrinsic_row_len(
        ctx: &mut BoxIntrinsicsCtx<'_>,
        width: f64,
        height: f64,
    ) -> flui_rendering::RenderResult<(usize, Vec<f64>)> {
        let count = ctx.child_count();
        let items = Self::item_count(count);
        let mut widths = Vec::with_capacity(items);
        for i in 0..items {
            widths.push(ctx.child_max_intrinsic_width(i, height)?);
        }
        let button = if count > 0 {
            ctx.child_max_intrinsic_width(items, height)?
        } else {
            0.0
        };
        Ok((row_len(&widths, button, width), widths))
    }

    fn intrinsic_height(
        &self,
        ctx: &mut BoxIntrinsicsCtx<'_>,
        width: f64,
        min: bool,
    ) -> flui_rendering::RenderResult<f64> {
        let count = ctx.child_count();
        if count == 0 {
            return Ok(0.0);
        }
        let items = Self::item_count(count);
        let (len, _) = Self::intrinsic_row_len(ctx, width, f64::INFINITY)?;
        let height_of = |ctx: &mut BoxIntrinsicsCtx<'_>, i: usize| {
            if min {
                ctx.child_min_intrinsic_height(i, width)
            } else {
                ctx.child_max_intrinsic_height(i, width)
            }
        };
        let overflowing = len < items;
        if self.expanded && overflowing {
            let mut sum = height_of(ctx, items)?;
            for i in len..items {
                sum += height_of(ctx, i)?;
            }
            return Ok(sum);
        }
        let mut tallest = 0.0_f64;
        for i in 0..len {
            tallest = tallest.max(height_of(ctx, i)?);
        }
        if overflowing {
            tallest = tallest.max(height_of(ctx, items)?);
        }
        Ok(tallest)
    }
}

impl flui_foundation::inspect::Inspect for RenderToolbarItems {
    fn kind(&self) -> &'static str {
        "RenderToolbarItems"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("expanded", &self.expanded);
        out.field("text_direction", &self.text_direction);
        out.field("row_len", &(self.row_len as u64));
    }
}

impl RenderBox for RenderToolbarItems {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        let count = ctx.child_count();
        let arrangement = self.arrange(constraints, count, |i, c| ctx.layout_child(i, c))?;
        for &(i, offset) in &arrangement.placed {
            ctx.position_child(i, offset);
        }
        self.child_count = count;
        self.row_len = arrangement.row_len;
        self.clipped = arrangement.clipped;
        self.split
            .record
            .set(Some((arrangement.row_len, Self::item_count(count))));
        Ok(arrangement.size)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        let count = ctx.child_count();
        self.arrange(constraints, count, |i, c| ctx.child_dry_layout(i, c))
            .map(|arrangement| arrangement.size)
    }

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        let count = ctx.child_count();
        let items = Self::item_count(count);
        match items {
            0 => Ok(0.0),
            1 => ctx.child_min_intrinsic_width(0, height),
            _ => Ok(ctx.child_min_intrinsic_width(0, height)?
                + ctx.child_min_intrinsic_width(items, height)?),
        }
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        let items = Self::item_count(ctx.child_count());
        (0..items)
            .map(|i| ctx.child_max_intrinsic_width(i, height))
            .sum()
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.intrinsic_height(ctx, width, true)
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        self.intrinsic_height(ctx, width, false)
    }

    /// A toolbar's buttons share no text line to align by.
    fn compute_distance_to_actual_baseline(
        &self,
        _baseline: TextBaseline,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        Ok(None)
    }

    /// Only the children the last layout shows are in the semantics tree.
    fn visits_child_for_semantics(&self, child_slot: usize, _size: Size) -> bool {
        self.shows(child_slot)
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Variable>) {
        let paint_shown = |ctx: &mut PaintCx<'_, Variable>| {
            for i in 0..self.child_count {
                if self.shows(i) {
                    ctx.paint_child(i);
                }
            }
        };
        if self.clipped {
            let bounds = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(bounds, Clip::HardEdge, paint_shown);
        } else {
            paint_shown(ctx);
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        for i in (0..self.child_count).rev() {
            if self.shows(i) && ctx.hit_test_child_at_layout_offset(i) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::row_len;

    #[test]
    fn the_row_holds_what_fits_beside_the_button() {
        let cases: [(&[f64], f64, f64, usize); 6] = [
            (&[], 10.0, 100.0, 0),
            (&[30.0, 30.0, 30.0], 10.0, 90.0, 3),
            (&[30.0, 30.0, 30.0], 10.0, 89.0, 2),
            (&[30.0, 30.0, 30.0], 40.0, 89.0, 1),
            (&[80.0, 30.0], 40.0, 60.0, 1),
            (&[30.0, 30.0, 30.0], 10.0, 0.0, 1),
        ];
        for (widths, button, max, expected) in cases {
            assert_eq!(
                row_len(widths, button, max),
                expected,
                "{widths:?} beside {button} in {max}"
            );
        }
    }
}
