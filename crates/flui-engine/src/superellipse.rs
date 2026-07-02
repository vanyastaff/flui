//! Backend-agnostic superellipse (iOS squircle) path generation.
//!
//! Pure geometry — no wgpu, no lyon; depends only on `flui_types`.
//! Moved here from `crate::wgpu::layer_render` so `CommandRenderer`'s
//! default `superellipse_path` impl can call it without the abstract
//! trait reaching into the concrete wgpu module.

use flui_types::{
    geometry::{Pixels, Point, RSuperellipse, px},
    painting::Path,
};

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
/// This function performs no caching — it regenerates the path on every call.
/// The production wgpu backend overrides `CommandRenderer::superellipse_path`
/// to consult its `Painter`-owned `SuperellipsePathCache` instead.
/// `DebugBackend` / `MockRenderer` use this uncached path directly.
pub(crate) fn generate_superellipse_path(superellipse: &RSuperellipse) -> Path {
    let rect = superellipse.outer_rect();
    let tl = superellipse.tl_radius();
    let tr = superellipse.tr_radius();
    let br = superellipse.br_radius();
    let bl = superellipse.bl_radius();

    let mut path = Path::new();

    // iOS squircle exponent
    let n: f32 = 4.0;
    let two_over_n = 2.0 / n;

    // Number of sample points per corner quarter-arc
    let segments_per_corner: usize = 16;

    let left = rect.left().0;
    let top = rect.top().0;
    let right = rect.right().0;
    let bottom = rect.bottom().0;

    // Compute the superellipse point for a corner quadrant.
    // `cx`, `cy`: corner center; `rx`, `ry`: per-corner radii;
    // `t`: parametric angle; `sx`/`sy`: quadrant signs.
    let se_point =
        |cx: f32, cy: f32, rx: f32, ry: f32, t: f32, sx: f32, sy: f32| -> Point<Pixels> {
            let cos_t = t.cos();
            let sin_t = t.sin();
            let x = cx + sx * rx * cos_t.abs().powf(two_over_n);
            let y = cy + sy * ry * sin_t.abs().powf(two_over_n);
            Point::new(px(x), px(y))
        };

    // Top-left corner: center at (left + tl.x, top + tl.y)
    // Sweep from PI/2 → 0, direction sx = -1, sy = -1 (upper-left quadrant)
    {
        let cx = left + tl.x.0;
        let cy = top + tl.y.0;
        let rx = tl.x.0;
        let ry = tl.y.0;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f32::consts::FRAC_PI_2 * (1.0 - i as f32 / segments_per_corner as f32);
                let p = se_point(cx, cy, rx, ry, t, -1.0, -1.0);
                if i == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
        } else {
            path.move_to(Point::new(px(left), px(top)));
        }
    }

    // Top-right corner: center at (right - tr.x, top + tr.y)
    // Direction sx = +1, sy = -1 (upper-right quadrant)
    {
        let cx = right - tr.x.0;
        let cy = top + tr.y.0;
        let rx = tr.x.0;
        let ry = tr.y.0;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f32::consts::FRAC_PI_2 * (i as f32 / segments_per_corner as f32);
                let p = se_point(cx, cy, rx, ry, t, 1.0, -1.0);
                path.line_to(p);
            }
        } else {
            path.line_to(Point::new(px(right), px(top)));
        }
    }

    // Bottom-right corner: center at (right - br.x, bottom - br.y)
    // Direction sx = +1, sy = +1 (lower-right quadrant)
    {
        let cx = right - br.x.0;
        let cy = bottom - br.y.0;
        let rx = br.x.0;
        let ry = br.y.0;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f32::consts::FRAC_PI_2 * (1.0 - i as f32 / segments_per_corner as f32);
                let p = se_point(cx, cy, rx, ry, t, 1.0, 1.0);
                path.line_to(p);
            }
        } else {
            path.line_to(Point::new(px(right), px(bottom)));
        }
    }

    // Bottom-left corner: center at (left + bl.x, bottom - bl.y)
    // Direction sx = -1, sy = +1 (lower-left quadrant)
    {
        let cx = left + bl.x.0;
        let cy = bottom - bl.y.0;
        let rx = bl.x.0;
        let ry = bl.y.0;
        if rx > 0.0 && ry > 0.0 {
            for i in 0..=segments_per_corner {
                let t = std::f32::consts::FRAC_PI_2 * (i as f32 / segments_per_corner as f32);
                let p = se_point(cx, cy, rx, ry, t, -1.0, 1.0);
                path.line_to(p);
            }
        } else {
            path.line_to(Point::new(px(left), px(bottom)));
        }
    }

    path.close();
    path
}
