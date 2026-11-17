//! The steps and the proportion a Win32 window keeps while the user drags its
//! frame (`WindowCommand::SetResizeIncrements`,
//! `WindowCommand::SetAspectRatio`): Win32 has neither, so the window
//! procedure holds the rectangle `WM_SIZING` proposes to them, the edge the
//! user drags moving and the opposite edge staying.
//!
//! The pure half of the Win32 window, compiled and tested on every host: the
//! rectangle and the frame around the content are plain values the procedure
//! reads from the message and the window (`proc`).

#![forbid(unsafe_code)]

use flui_platform::AspectRatio;

use super::limits::PixelSize;

/// The edge or corner of the frame a sizing drag moves (`WMSZ_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizingEdge {
    Left,
    Right,
    Top,
    TopLeft,
    TopRight,
    Bottom,
    BottomLeft,
    BottomRight,
}

impl SizingEdge {
    /// The edge `WM_SIZING`'s `wParam` names; `None` for a value Win32 does
    /// not send.
    pub(crate) const fn from_wmsz(edge: usize) -> Option<Self> {
        Some(match edge {
            1 => Self::Left,
            2 => Self::Right,
            3 => Self::Top,
            4 => Self::TopLeft,
            5 => Self::TopRight,
            6 => Self::Bottom,
            7 => Self::BottomLeft,
            8 => Self::BottomRight,
            _ => return None,
        })
    }

    /// Whether the drag moves the left edge (else the right one stays put or
    /// moves).
    const fn moves_left(self) -> bool {
        matches!(self, Self::Left | Self::TopLeft | Self::BottomLeft)
    }

    /// Whether the drag moves the top edge.
    const fn moves_top(self) -> bool {
        matches!(self, Self::Top | Self::TopLeft | Self::TopRight)
    }

    /// Whether the drag moves only a horizontal edge, so the height follows
    /// from the width it leaves the content.
    const fn is_vertical_only(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }
}

/// A window rectangle in screen device pixels, as `WM_SIZING` carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SizingRect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

/// What the user's resize keeps, in device pixels at the window's scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResizeRule {
    /// The content grows and shrinks in steps of this many pixels, counted
    /// from [`base`](Self::base).
    pub(crate) increments: Option<PixelSize>,
    /// Where the steps count from: the content's minimum size, or zero.
    pub(crate) base: PixelSize,
    /// The content keeps this proportion.
    pub(crate) aspect: Option<AspectRatio>,
}

impl ResizeRule {
    /// `proposed` held to this rule, when the user drags `edge` of a window
    /// whose frame adds `frame` to its content: the content takes the
    /// proportion (the dragged dimension decides, the width at a corner),
    /// then the steps (rounded down toward the base; a size at or below the
    /// base stays), and the edges the drag moves give
    /// way, the opposite ones staying where they are. The arithmetic is
    /// wide, so no rectangle overflows it.
    pub(crate) fn hold(
        &self,
        edge: SizingEdge,
        proposed: SizingRect,
        frame: PixelSize,
    ) -> SizingRect {
        let outer_width = i64::from(proposed.right) - i64::from(proposed.left);
        let outer_height = i64::from(proposed.bottom) - i64::from(proposed.top);
        let mut width = (outer_width - i64::from(frame.width)).max(1);
        let mut height = (outer_height - i64::from(frame.height)).max(1);
        if let Some(aspect) = self.aspect {
            let (w, h) = (
                i64::from(aspect.width().get()),
                i64::from(aspect.height().get()),
            );
            if edge.is_vertical_only() {
                width = (height * w + h / 2) / h;
            } else {
                height = (width * h + w / 2) / w;
            }
            width = width.max(1);
            height = height.max(1);
        }
        if let Some(step) = self.increments {
            width = stepped(width, i64::from(self.base.width), i64::from(step.width));
            height = stepped(height, i64::from(self.base.height), i64::from(step.height));
        }
        let outer_width = width + i64::from(frame.width);
        let outer_height = height + i64::from(frame.height);
        let (left, right) = if edge.moves_left() {
            (
                i64::from(proposed.right) - outer_width,
                i64::from(proposed.right),
            )
        } else {
            (
                i64::from(proposed.left),
                i64::from(proposed.left) + outer_width,
            )
        };
        let (top, bottom) = if edge.moves_top() {
            (
                i64::from(proposed.bottom) - outer_height,
                i64::from(proposed.bottom),
            )
        } else {
            (
                i64::from(proposed.top),
                i64::from(proposed.top) + outer_height,
            )
        };
        SizingRect {
            left: saturate(left),
            top: saturate(top),
            right: saturate(right),
            bottom: saturate(bottom),
        }
    }
}

/// `size` rounded down to a whole number of `step`s above `base`; a size at
/// or below the base, or a step that is no step, stays.
fn stepped(size: i64, base: i64, step: i64) -> i64 {
    if step <= 0 || size <= base {
        return size;
    }
    base + (size - base) / step * step
}

/// `value` in the range a `RECT` edge holds.
fn saturate(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> SizingRect {
        SizingRect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn ratio(width: u32, height: u32) -> AspectRatio {
        AspectRatio::new(
            NonZeroU32::new(width).expect("nonzero"),
            NonZeroU32::new(height).expect("nonzero"),
        )
    }

    const FRAME: PixelSize = PixelSize::new(16, 39);

    #[test]
    fn a_sizing_drag_keeps_the_steps_and_the_proportion_and_moves_only_its_edges() {
        let steps = ResizeRule {
            increments: Some(PixelSize::new(10, 20)),
            base: PixelSize::new(100, 100),
            aspect: None,
        };
        let wide = ResizeRule {
            increments: None,
            base: PixelSize::new(0, 0),
            aspect: Some(ratio(16, 9)),
        };
        let both = ResizeRule {
            increments: Some(PixelSize::new(16, 9)),
            ..wide
        };
        // (rule, edge, proposed outer rectangle, held outer rectangle)
        let cases = [
            // Content 127x161 from base 100: down to 120 and 160.
            (
                steps,
                SizingEdge::BottomRight,
                rect(0, 0, 143, 200),
                rect(0, 0, 136, 199),
            ),
            // The left edge moves; the right one stays.
            (
                steps,
                SizingEdge::TopLeft,
                rect(-27, -1, 116, 199),
                rect(-20, 0, 116, 199),
            ),
            // At or below the base, nothing to step.
            (
                steps,
                SizingEdge::Right,
                rect(0, 0, 90, 90),
                rect(0, 0, 90, 90),
            ),
            // A right-edge drag to 320 wide: the height follows, 180.
            (
                wide,
                SizingEdge::Right,
                rect(0, 0, 336, 100),
                rect(0, 0, 336, 219),
            ),
            // A bottom-edge drag to 90 high: the width follows, 160.
            (
                wide,
                SizingEdge::Bottom,
                rect(0, 0, 500, 129),
                rect(0, 0, 176, 129),
            ),
            // A top-left drag: the width decides, the top and left give way.
            (
                wide,
                SizingEdge::TopLeft,
                rect(-304, 0, 16, 1000),
                rect(-304, 790, 16, 1000),
            ),
            // The proportion, then the steps: 330 wide is 186 high, which
            // steps down to 320 by 180.
            (
                both,
                SizingEdge::BottomRight,
                rect(0, 0, 346, 100),
                rect(0, 0, 336, 219),
            ),
        ];
        for (rule, edge, proposed, held) in cases {
            assert_eq!(
                rule.hold(edge, proposed, FRAME),
                held,
                "{rule:?} dragging {edge:?} to {proposed:?}"
            );
        }
    }

    #[test]
    fn a_rectangle_at_the_edge_of_the_numbers_saturates_instead_of_overflowing() {
        let rule = ResizeRule {
            increments: None,
            base: PixelSize::new(0, 0),
            aspect: Some(ratio(1, 1_000)),
        };
        let held = rule.hold(
            SizingEdge::BottomRight,
            rect(0, 0, i32::MAX, 10),
            PixelSize::new(0, 0),
        );
        assert_eq!(held.bottom, i32::MAX);
        assert_eq!(held.right, i32::MAX);
    }

    #[test]
    fn an_edge_win32_does_not_send_names_no_drag() {
        assert_eq!(SizingEdge::from_wmsz(0), None);
        assert_eq!(SizingEdge::from_wmsz(9), None);
        assert_eq!(SizingEdge::from_wmsz(8), Some(SizingEdge::BottomRight));
    }
}
