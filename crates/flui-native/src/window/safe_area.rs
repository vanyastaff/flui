//! The safe area of a window that covers part of a viewport whose edges the
//! platform reports unsafe: a browser's `env(safe-area-inset-*)`, which names
//! the bands of the layout viewport that a notch, rounded corners or a home
//! indicator take, while the window is a canvas laid out anywhere in it.

use flui_foundation::geometry::{EdgeInsets, Size};

/// A window's box in the viewport's coordinates: its origin and size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placement {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// How much of the window at `window` each unsafe band of a `viewport` with
/// the `unsafe_edges` covers, in the window's logical pixels, a window drawn
/// at `logical` while it measures `window.width` by `window.height` in the
/// viewport (a CSS transform scales the one and not the other).
///
/// A band reaches into the window from the viewport's edge by its width less
/// the window's distance from that edge, never past the window's far side; a
/// window clear of every band has none. Anything that is no number (a box, a
/// band or a size), a window with no area and a band that is negative give
/// no inset on that edge rather than a wrong one.
#[must_use]
pub(crate) fn within(
    viewport: Size<f64>,
    unsafe_edges: EdgeInsets,
    window: Placement,
    logical: Size<f64>,
) -> EdgeInsets {
    let usable = |value: f64| value.is_finite();
    if ![
        viewport.width,
        viewport.height,
        window.x,
        window.y,
        window.width,
        window.height,
        logical.width,
        logical.height,
    ]
    .into_iter()
    .all(usable)
        || window.width <= 0.0
        || window.height <= 0.0
    {
        return EdgeInsets::ZERO;
    }
    // How far a band `band` wide reaches past `distance` into a side `extent`
    // long, scaled into logical pixels.
    let reach = |band: f64, distance: f64, extent: f64, scale: f64| {
        if !band.is_finite() || band <= 0.0 {
            return 0.0;
        }
        let covered = (band - distance).clamp(0.0, extent) * scale;
        if covered.is_finite() { covered } else { 0.0 }
    };
    let horizontal = logical.width / window.width;
    let vertical = logical.height / window.height;
    EdgeInsets::new(
        reach(unsafe_edges.top, window.y, window.height, vertical),
        reach(
            unsafe_edges.right,
            viewport.width - (window.x + window.width),
            window.width,
            horizontal,
        ),
        reach(
            unsafe_edges.bottom,
            viewport.height - (window.y + window.height),
            window.height,
            vertical,
        ),
        reach(unsafe_edges.left, window.x, window.width, horizontal),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A phone in portrait: a notch band at the top, a home indicator at the
    /// bottom.
    const PORTRAIT: Size<f64> = Size::new(390.0, 844.0);
    const NOTCH: EdgeInsets = EdgeInsets::new(47.0, 0.0, 34.0, 0.0);

    fn at(x: f64, y: f64, width: f64, height: f64) -> Placement {
        Placement {
            x,
            y,
            width,
            height,
        }
    }

    /// Each case: the viewport, its unsafe edges, the window's box, its
    /// logical size, and the safe area the window has.
    #[test]
    fn a_window_takes_the_part_of_each_band_over_it() {
        let landscape = Size::new(844.0, 390.0);
        let sides = EdgeInsets::new(0.0, 47.0, 21.0, 47.0);
        let cases = [
            (
                "a window over the whole viewport takes every band",
                PORTRAIT,
                NOTCH,
                at(0.0, 0.0, 390.0, 844.0),
                PORTRAIT,
                NOTCH,
            ),
            (
                "a window clear of the bands takes none",
                PORTRAIT,
                NOTCH,
                at(20.0, 100.0, 300.0, 400.0),
                Size::new(300.0, 400.0),
                EdgeInsets::ZERO,
            ),
            (
                "a window below the top edge takes what is left of its band",
                PORTRAIT,
                NOTCH,
                at(0.0, 40.0, 390.0, 804.0),
                Size::new(390.0, 804.0),
                EdgeInsets::new(7.0, 0.0, 34.0, 0.0),
            ),
            (
                "a band wider than the window covers the window and no more",
                PORTRAIT,
                EdgeInsets::new(120.0, 0.0, 0.0, 0.0),
                at(0.0, 0.0, 390.0, 100.0),
                Size::new(390.0, 100.0),
                EdgeInsets::new(100.0, 0.0, 0.0, 0.0),
            ),
            (
                "landscape bands reach in from the sides",
                landscape,
                sides,
                at(0.0, 0.0, 844.0, 390.0),
                landscape,
                sides,
            ),
            (
                "a window drawn at half its logical size takes the bands in logical pixels",
                PORTRAIT,
                NOTCH,
                at(0.0, 0.0, 390.0, 844.0),
                Size::new(780.0, 1688.0),
                EdgeInsets::new(94.0, 0.0, 68.0, 0.0),
            ),
            (
                "a negative band is none",
                PORTRAIT,
                EdgeInsets::new(-5.0, 0.0, 34.0, 0.0),
                at(0.0, 0.0, 390.0, 844.0),
                PORTRAIT,
                EdgeInsets::new(0.0, 0.0, 34.0, 0.0),
            ),
            (
                "a band that is no number is none",
                PORTRAIT,
                EdgeInsets::new(f64::NAN, 0.0, f64::INFINITY, 0.0),
                at(0.0, 0.0, 390.0, 844.0),
                PORTRAIT,
                EdgeInsets::ZERO,
            ),
            (
                "a window with no area has none",
                PORTRAIT,
                NOTCH,
                at(0.0, 0.0, 0.0, 844.0),
                PORTRAIT,
                EdgeInsets::ZERO,
            ),
            (
                "a box that is no number has none",
                PORTRAIT,
                NOTCH,
                at(f64::NAN, 0.0, 390.0, 844.0),
                PORTRAIT,
                EdgeInsets::ZERO,
            ),
        ];
        for (case, viewport, edges, window, logical, expected) in cases {
            assert_eq!(within(viewport, edges, window, logical), expected, "{case}");
        }
    }
}
