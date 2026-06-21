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
//! and the star tips). Run with: `cargo run --example aa_showcase`
//! (or `just example aa_showcase`).

use flui_app::{AppConfig, run_direct};

fn main() -> anyhow::Result<()> {
    run_direct(
        AppConfig::new()
            .with_title("FLUI — AA Showcase (L2 SDF + SSAA paths)")
            .with_size(960, 640),
        |builder, width, height| {
            use flui_painting::Canvas;
            use flui_types::{
                Point, RRect, Rect,
                geometry::{Pixels, px},
                painting::{Paint, path::Path},
                styling::Color,
            };

            let mut canvas = Canvas::new();

            // Dark slate background so the AA edge band is visible against fills.
            canvas.draw_rect(
                Rect::from_ltrb(px(0.0), px(0.0), px(width), px(height)),
                &Paint::fill(Color::rgb(24, 24, 37)),
            );

            let white = Paint::fill(Color::WHITE);

            // ── Row 1: rounded rects rotated 0/15/30/45° (SDF-instanced affine) ──
            // The 45° card is the clearest L2-vs-L1 tell: its edges should read as a
            // single crisp ~1px ramp, not a fuzzy ~1.4px band.
            let card_half_w = 60.0_f32;
            let card_half_h = 38.0_f32;
            let row1_y = 130.0_f32;
            for (slot, angle_deg) in [0.0_f32, 15.0, 30.0, 45.0].into_iter().enumerate() {
                let center_x = 140.0 + slot as f32 * 220.0;
                canvas.save();
                canvas.translate(center_x, row1_y);
                canvas.rotate(angle_deg.to_radians());
                let local = Rect::from_ltrb(
                    px(-card_half_w),
                    px(-card_half_h),
                    px(card_half_w),
                    px(card_half_h),
                );
                canvas.draw_rrect(RRect::from_rect_circular(local, Pixels(16.0)), &white);
                canvas.restore();
            }

            // ── Row 2: circle, rotated oval, pie arc (circle/arc instanced) ──────
            let row2_y = 340.0_f32;
            canvas.draw_circle(Point::new(px(140.0), px(row2_y)), px(52.0), &white);

            // Oval rotated 30° to exercise the affine ellipse path.
            canvas.save();
            canvas.translate(380.0, row2_y);
            canvas.rotate(30.0_f32.to_radians());
            canvas.draw_oval(
                Rect::from_ltrb(px(-70.0), px(-40.0), px(70.0), px(40.0)),
                &white,
            );
            canvas.restore();

            // Pie arc: 270° sweep, filled to centre.
            canvas.draw_arc(
                Rect::from_ltrb(px(560.0), px(row2_y - 56.0), px(672.0), px(row2_y + 56.0)),
                -45.0_f32.to_radians(),
                270.0_f32.to_radians(),
                true,
                &white,
            );

            // Rounded-rect ring (drrect) — outer minus inner.
            let ring_center_x = 840.0_f32;
            let outer = RRect::from_rect_circular(
                Rect::from_ltrb(
                    px(ring_center_x - 56.0),
                    px(row2_y - 56.0),
                    px(ring_center_x + 56.0),
                    px(row2_y + 56.0),
                ),
                Pixels(20.0),
            );
            let inner = RRect::from_rect_circular(
                Rect::from_ltrb(
                    px(ring_center_x - 32.0),
                    px(row2_y - 32.0),
                    px(ring_center_x + 32.0),
                    px(row2_y + 32.0),
                ),
                Pixels(12.0),
            );
            canvas.draw_drrect(outer, inner, &white);

            // ── Row 3: self-intersecting 5-point star (SSAA-tile fill path) ──────
            let star_center = Point::new(px(width / 2.0), px(520.0));
            let outer_radius = 80.0_f32;
            let inner_radius = 32.0_f32;
            let mut star = Path::new();
            for tip in 0..10 {
                let radius = if tip % 2 == 0 {
                    outer_radius
                } else {
                    inner_radius
                };
                // Start at the top tip (-90°) and step every 36°.
                let angle = (-90.0_f32 + tip as f32 * 36.0).to_radians();
                let point = Point::new(
                    px(star_center.x.0 + radius * angle.cos()),
                    px(star_center.y.0 + radius * angle.sin()),
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
