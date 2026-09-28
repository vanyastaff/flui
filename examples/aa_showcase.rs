//! AA Showcase — visual check for the engine's anti-aliasing paths.
//!
//! Renders the primitives whose AA this engine computes, at angles that make the
//! quality visible:
//!   * rounded rects rotated 0° / 15° / 30° / 45° — the SDF-instanced affine path
//!     (`rect_instanced.wgsl`). The L2 (`length(dpdx, dpdy)`) gradient gives a
//!     ~1-device-px edge band at every angle; the old L1/`fwidth` band was up to
//!     √2 (~1.41px) wider on the 45° edges.
//!   * a circle and a rotated oval — `circle_instanced.wgsl`.
//!   * a pie arc — `arc_instanced.wgsl` (radial + angular SDF edges).
//!   * a self-intersecting 5-point star, filled — the SSAA-tile path
//!     (`draw_path` → supersampled offscreen → box downsample), which is what the
//!     pool-bucketing + `crop_uv` work touches.
//!   * a rounded-rect ring (`draw_drrect`).
//!
//! White-on-dark so the boundary band is easy to inspect (zoom in on the 45° rect
//! and the star tips). Run with: `cargo run --example aa_showcase`.

use flui_app::{AppConfig, run_direct};

fn main() -> anyhow::Result<()> {
    run_direct(
        AppConfig::new()
            .with_title("FLUI — AA Showcase (L2 SDF + SSAA paths)")
            .with_size(960, 640),
        |builder, width, height| {
            use flui_foundation::geometry::{Point, RRect, Rect};
            use flui_painting::Canvas;
            use flui_painting::{
                paint::{Paint, path::Path},
                styling::Color,
            };

            let mut canvas = Canvas::new();

            // Dark slate background so the AA edge band is visible against fills.
            canvas.draw_rect(
                Rect::from_ltrb(0.0, 0.0, width, height),
                &Paint::fill(Color::rgb(24, 24, 37)),
            );

            let white = Paint::fill(Color::WHITE);

            // ── Row 1: rounded rects rotated 0/15/30/45° (SDF-instanced affine) ──
            // The 45° card is the clearest L2-vs-L1 tell: its edges should read as a
            // single crisp ~1px ramp, not a fuzzy ~1.4px band.
            let card_half_w = 60.0_f64;
            let card_half_h = 38.0_f64;
            let row1_y = 130.0_f64;
            for (slot, angle_deg) in [0.0_f64, 15.0, 30.0, 45.0].into_iter().enumerate() {
                let center_x = 140.0 + slot as f64 * 220.0;
                canvas.save();
                canvas.translate(center_x, row1_y);
                canvas.rotate(angle_deg.to_radians());
                let local = Rect::from_ltrb(-card_half_w, -card_half_h, card_half_w, card_half_h);
                canvas.draw_rrect(RRect::from_rect_circular(local, 16.0), &white);
                canvas.restore();
            }

            // ── Row 2: circle, rotated oval, pie arc (circle/arc instanced) ──────
            let row2_y = 340.0_f64;
            canvas.draw_circle(Point::new(140.0, row2_y), 52.0, &white);

            // Oval rotated 30° to exercise the affine ellipse path.
            canvas.save();
            canvas.translate(380.0, row2_y);
            canvas.rotate(30.0_f64.to_radians());
            canvas.draw_oval(Rect::from_ltrb(-70.0, -40.0, 70.0, 40.0), &white);
            canvas.restore();

            // Pie arc: 270° sweep, filled to centre.
            canvas.draw_arc(
                Rect::from_ltrb(560.0, row2_y - 56.0, 672.0, row2_y + 56.0),
                -45.0_f64.to_radians(),
                270.0_f64.to_radians(),
                true,
                &white,
            );

            // Rounded-rect ring (drrect) — outer minus inner.
            let ring_center_x = 840.0_f64;
            let outer = RRect::from_rect_circular(
                Rect::from_ltrb(
                    ring_center_x - 56.0,
                    row2_y - 56.0,
                    ring_center_x + 56.0,
                    row2_y + 56.0,
                ),
                20.0,
            );
            let inner = RRect::from_rect_circular(
                Rect::from_ltrb(
                    ring_center_x - 32.0,
                    row2_y - 32.0,
                    ring_center_x + 32.0,
                    row2_y + 32.0,
                ),
                12.0,
            );
            canvas.draw_drrect(outer, inner, &white);

            // ── Row 3: self-intersecting 5-point star (SSAA-tile fill path) ──────
            let star_center = Point::new(width / 2.0, 520.0);
            let outer_radius = 80.0_f64;
            let inner_radius = 32.0_f64;
            let mut star = Path::new();
            for tip in 0..10 {
                let radius = if tip % 2 == 0 {
                    outer_radius
                } else {
                    inner_radius
                };
                // Start at the top tip (-90°) and step every 36°.
                let angle = (-90.0_f64 + tip as f64 * 36.0).to_radians();
                let point = Point::new(
                    star_center.x + radius * angle.cos(),
                    star_center.y + radius * angle.sin(),
                );
                if tip == 0 {
                    star.move_to(point);
                } else {
                    star.line_to(point);
                }
            }
            star.close();
            canvas.draw_path(&star, &white);

            builder.add_picture(canvas.finish());
        },
    )
}
