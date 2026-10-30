//! [`reading_order`]: the order a reader takes a document's texts in, from
//! where they are laid out.

use std::cmp::Ordering;

use flui_foundation::geometry::Rect;

use crate::properties::TextDirection;

/// The order a reader takes `boxes` in, as indices into `boxes`: line by line
/// from the top, and along each line in `direction`.
///
/// One function orders a selectable document for assistive technology and for
/// the pointer and keys that select in it, so the text a selection reports is
/// the text it highlights.
///
/// Lines are found by a heuristic that may change without notice: boxes are
/// taken from the top, and a box joins the latest line whose band (its first
/// box's top to bottom) holds the box's vertical centre or whose band's centre
/// the box spans, so a small text beside a tall one shares its line however
/// the two are aligned; any other box starts a line. Boxes
/// along a line are ordered by their leading edge. Ties keep the order of
/// `boxes`, so texts stacked on one another read in the order they are given.
/// A box with a non-finite edge has no place on the page: it reads after the
/// placed ones, in the order given.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::geometry::Rect;
/// use flui_semantics::{TextDirection, reading_order};
///
/// let right = Rect::from_ltwh(100.0, 0.0, 50.0, 20.0);
/// let left = Rect::from_ltwh(0.0, 2.0, 50.0, 16.0);
/// let below = Rect::from_ltwh(0.0, 30.0, 50.0, 20.0);
/// let boxes = [below, right, left];
/// assert_eq!(reading_order(&boxes, TextDirection::Ltr), [2, 1, 0]);
/// assert_eq!(reading_order(&boxes, TextDirection::Rtl), [1, 2, 0]);
/// ```
#[must_use]
pub fn reading_order(boxes: &[Rect], direction: TextDirection) -> Vec<usize> {
    let (mut placed, unplaced): (Vec<usize>, Vec<usize>) =
        (0..boxes.len()).partition(|&index| boxes[index].is_finite());
    placed.sort_by(|&a, &b| boxes[a].top().total_cmp(&boxes[b].top()).then(a.cmp(&b)));

    // Each line keeps its band and its boxes. Boxes come by top edge, so a
    // line whose band ends above a box's top can take no later box either:
    // only the lines still open are searched, and a column of texts is
    // ordered in linear time.
    let mut lines: Vec<((f64, f64), Vec<usize>)> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for index in placed {
        let rect = &boxes[index];
        open.retain(|&line| lines[line].0.1 >= rect.top());
        let centre = f64::midpoint(rect.top(), rect.bottom());
        let joined = open.iter().rev().copied().find(|&line| {
            let (top, bottom) = lines[line].0;
            let band_centre = f64::midpoint(top, bottom);
            (top <= centre && centre <= bottom)
                || (rect.top() <= band_centre && band_centre <= rect.bottom())
        });
        if let Some(line) = joined {
            lines[line].1.push(index);
        } else {
            open.push(lines.len());
            lines.push(((rect.top(), rect.bottom()), vec![index]));
        }
    }

    let along = |a: &usize, b: &usize| -> Ordering {
        let (a_rect, b_rect) = (&boxes[*a], &boxes[*b]);
        match direction {
            TextDirection::Ltr => a_rect.left().total_cmp(&b_rect.left()),
            TextDirection::Rtl => b_rect.right().total_cmp(&a_rect.right()),
        }
        .then(a.cmp(b))
    };
    let mut order = Vec::with_capacity(boxes.len());
    for (_, mut line) in lines {
        line.sort_by(along);
        order.extend(line);
    }
    order.extend(unplaced);
    order
}
