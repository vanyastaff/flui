//! Backend-agnostic superellipse (iOS squircle) path generation.
//!
//! Pure geometry — no wgpu, no lyon; depends only on `flui_foundation::geometry`.
//!
//! The CPU statement of the shape the GPU evaluates as a signed distance
//! field. Nothing in a shipped build calls it — see the module declaration in
//! `lib.rs` for why it is `cfg(test)` and what it is for.

use flui_foundation::geometry::{Point, RSuperellipse};
use flui_painting::paint::Path;

/// Generate a superellipse (iOS squircle) path from an [`RSuperellipse`].
///
/// Uses the parametric superellipse equation with `n = 4` (iOS squircle):
/// ```text
/// x(t) = a * sign(cos(t)) * |cos(t)|^(2/n)
/// y(t) = b * sign(sin(t)) * |sin(t)|^(2/n)
/// ```
///
/// Each corner is generated independently using its own radii, with straight
/// edges connecting the corners. 16 sample points per corner quarter-arc
/// produce a visually smooth curve.
///
/// # Caching
///
/// None: it regenerates the path on every call. It is called once per test,
/// so a cache would buy nothing and would have to be kept correct.
pub(crate) fn generate_superellipse_path(superellipse: &RSuperellipse) -> Path {
    let rect = superellipse.outer_rect();
    let tl = superellipse.tl_radius();
    let tr = superellipse.tr_radius();
    let br = superellipse.br_radius();
    let bl = superellipse.bl_radius();

    let mut path = Path::new();

    // iOS squircle exponent
    let n: f64 = 4.0;
    let two_over_n = 2.0 / n;

    // Number of sample points per corner quarter-arc
    let segments_per_corner: usize = 16;

    let left = rect.left();
    let top = rect.top();
    let right = rect.right();
    let bottom = rect.bottom();

    // Compute the superellipse point for a corner quadrant.
    // `cx`, `cy`: corner center; `rx`, `ry`: per-corner radii;
    // `t`: parametric angle; `sx`/`sy`: quadrant signs.
    let se_point = |cx: f64, cy: f64, rx: f64, ry: f64, t: f64, sx: f64, sy: f64| -> Point<f64> {
        let cos_t = t.cos();
        let sin_t = t.sin();
        let x = cx + sx * rx * cos_t.abs().powf(two_over_n);
        let y = cy + sy * ry * sin_t.abs().powf(two_over_n);
        Point::new(x, y)
    };

    // Top-left corner: center at (left + tl.x, top + tl.y)
    // Sweep from PI/2 → 0, direction sx = -1, sy = -1 (upper-left quadrant)
    {
        let cx = left + tl.x;
        let cy = top + tl.y;
        let rx = tl.x;
        let ry = tl.y;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f64::consts::FRAC_PI_2 * (1.0 - i as f64 / segments_per_corner as f64);
                let p = se_point(cx, cy, rx, ry, t, -1.0, -1.0);
                if i == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
        } else {
            path.move_to(Point::new(left, top));
        }
    }

    // Top-right corner: center at (right - tr.x, top + tr.y)
    // Direction sx = +1, sy = -1 (upper-right quadrant)
    {
        let cx = right - tr.x;
        let cy = top + tr.y;
        let rx = tr.x;
        let ry = tr.y;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f64::consts::FRAC_PI_2 * (i as f64 / segments_per_corner as f64);
                let p = se_point(cx, cy, rx, ry, t, 1.0, -1.0);
                path.line_to(p);
            }
        } else {
            path.line_to(Point::new(right, top));
        }
    }

    // Bottom-right corner: center at (right - br.x, bottom - br.y)
    // Direction sx = +1, sy = +1 (lower-right quadrant)
    {
        let cx = right - br.x;
        let cy = bottom - br.y;
        let rx = br.x;
        let ry = br.y;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f64::consts::FRAC_PI_2 * (1.0 - i as f64 / segments_per_corner as f64);
                let p = se_point(cx, cy, rx, ry, t, 1.0, 1.0);
                path.line_to(p);
            }
        } else {
            path.line_to(Point::new(right, bottom));
        }
    }

    // Bottom-left corner: center at (left + bl.x, bottom - bl.y)
    // Direction sx = -1, sy = +1 (lower-left quadrant)
    {
        let cx = left + bl.x;
        let cy = bottom - bl.y;
        let rx = bl.x;
        let ry = bl.y;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f64::consts::FRAC_PI_2 * (i as f64 / segments_per_corner as f64);
                let p = se_point(cx, cy, rx, ry, t, -1.0, 1.0);
                path.line_to(p);
            }
        } else {
            path.line_to(Point::new(left, bottom));
        }
    }

    path.close();
    path
}
