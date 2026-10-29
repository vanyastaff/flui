//! The atlas an emitter draws its particles from: its sprite rasterised once.

use std::f64::consts::{FRAC_PI_2, PI};

use flui_foundation::geometry::Rect;
use flui_painting::paint::Image;

use super::config::Sprite;

/// The side, in pixels, of a built-in sprite.
pub(super) const SIDE: u32 = 32;
/// Samples per pixel along each axis, for smooth edges.
const SUPERSAMPLE: u32 = 4;

/// An emitter's atlas image and the rectangle of its one sprite.
#[derive(Debug, Clone)]
pub(super) struct Atlas {
    pub(super) image: Image,
    pub(super) sprite: Rect<f64>,
    /// The sprite's longer side, which a particle's size scales to.
    pub(super) side: f64,
}

impl Atlas {
    pub(super) fn new(sprite: &Sprite) -> Self {
        let coverage: fn(f64, f64) -> bool = match sprite {
            Sprite::Image(image) => {
                let (width, height) = (f64::from(image.width()), f64::from(image.height()));
                return Self {
                    image: image.clone(),
                    sprite: Rect::from_ltwh(0.0, 0.0, width, height),
                    side: width.max(height).max(1.0),
                };
            }
            Sprite::Square => |_, _| true,
            Sprite::Circle => |x, y| x * x + y * y <= 1.0,
            Sprite::Star => in_star,
        };
        let mut data = Vec::with_capacity((SIDE * SIDE * 4) as usize);
        let samples = f64::from(SUPERSAMPLE * SUPERSAMPLE);
        for row in 0..SIDE {
            for column in 0..SIDE {
                let mut covered = 0u32;
                for sy in 0..SUPERSAMPLE {
                    for sx in 0..SUPERSAMPLE {
                        // The sample's position in [-1, 1] across the sprite.
                        let at = |pixel: u32, sub: u32| {
                            (f64::from(pixel * SUPERSAMPLE + sub) + 0.5)
                                / f64::from(SIDE * SUPERSAMPLE)
                                * 2.0
                                - 1.0
                        };
                        if coverage(at(column, sx), at(row, sy)) {
                            covered += 1;
                        }
                    }
                }
                // White, its alpha the covered share: the particle's color tints it.
                let alpha = (f64::from(covered) / samples * 255.0).round() as u8;
                data.extend_from_slice(&[255, 255, 255, alpha]);
            }
        }
        let side = f64::from(SIDE);
        Self {
            image: Image::from_rgba8(SIDE, SIDE, data),
            sprite: Rect::from_ltwh(0.0, 0.0, side, side),
            side,
        }
    }
}

/// Whether `(x, y)`, in `[-1, 1]` with y down, is inside a five-pointed star pointing up
/// whose points touch the unit circle.
fn in_star(x: f64, y: f64) -> bool {
    const INNER: f64 = 0.4;
    let radius = x.hypot(y);
    if radius > 1.0 {
        return false;
    }
    // The angle from the upward point, folded into one point's half: [0, π/5].
    let sector = PI / 5.0;
    let angle = (y.atan2(x) + FRAC_PI_2).rem_euclid(2.0 * sector);
    let folded = if angle > sector {
        2.0 * sector - angle
    } else {
        angle
    };
    // The edge from the point (radius 1, angle 0) to the notch (INNER, sector), in polar
    // form: inside when the sample is nearer the centre than that edge along its ray.
    let (tip_x, tip_y) = (0.0, 1.0);
    let (notch_x, notch_y) = (INNER * sector.sin(), INNER * sector.cos());
    let (ray_x, ray_y) = (folded.sin(), folded.cos());
    // Where the ray meets the edge: solve tip + s·(notch − tip) = r·ray.
    let (ex, ey) = (notch_x - tip_x, notch_y - tip_y);
    let denominator = ray_x * ey - ray_y * ex;
    if denominator.abs() < f64::EPSILON {
        return radius <= 1.0;
    }
    let reach = (tip_x * ey - tip_y * ex) / denominator;
    radius <= reach
}
