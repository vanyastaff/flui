//! How `ColorTween` interpolates: Oklab with premultiplied alpha.
//!
//! Prints two transitions. Blue → yellow compares a gamma-sRGB channel mix, which dips
//! through a dark grey midpoint, with `ColorTween`'s Oklab mix, which keeps perceived
//! lightness steady. Red → transparent compares a straight-alpha Oklab mix, which blends
//! in the transparent end's black and darkens, with `ColorTween`'s premultiplied mix,
//! which stays red while it fades.
//!
//! Run with: `cargo run -p flui-animation --example oklab_gradient`

use flui_animation::{Animatable, ColorTween};
use flui_painting::styling::{Color, Oklab};

/// Rounds a channel value into `0..=255`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "rounded and clamped into 0..=255 first"
)]
fn channel(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// A gamma-sRGB channel mix, for comparison.
fn srgb_mix(from: Color, to: Color, t: f32) -> Color {
    let mix = |a: u8, b: u8| channel(f32::from(a) + (f32::from(b) - f32::from(a)) * t);
    Color::rgba(
        mix(from.r, to.r),
        mix(from.g, to.g),
        mix(from.b, to.b),
        mix(from.a, to.a),
    )
}

/// An Oklab mix with straight (unpremultiplied) alpha, for comparison.
fn straight_oklab_mix(from: Color, to: Color, t: f32) -> Color {
    let (a, b) = (from.to_oklab(), to.to_oklab());
    let mix = |x: f32, y: f32| x + (y - x) * t;
    let lab = Oklab {
        l: mix(a.l, b.l),
        a: mix(a.a, b.a),
        b: mix(a.b, b.b),
    };
    Color::from_oklab(lab, channel(mix(f32::from(from.a), f32::from(to.a))))
}

fn brightness(c: Color) -> u16 {
    u16::from(c.r) + u16::from(c.g) + u16::from(c.b)
}

fn main() {
    let (blue, yellow) = (Color::rgb(0, 0, 255), Color::rgb(255, 255, 0));
    let across = ColorTween::new(blue, yellow);
    println!("blue -> yellow:");
    println!("    t     sRGB (r,g,b)  sum    ColorTween (r,g,b)  sum");
    for step in 0u8..=10 {
        let t = f32::from(step) / 10.0;
        let (s, o) = (srgb_mix(blue, yellow, t), across.transform(f64::from(t)));
        println!(
            "  {t:4.1}   ({:3},{:3},{:3})  {:4}   ({:3},{:3},{:3})       {:4}",
            s.r,
            s.g,
            s.b,
            brightness(s),
            o.r,
            o.g,
            o.b,
            brightness(o),
        );
    }

    let red = Color::rgb(255, 0, 0);
    let fade = ColorTween::new(red, Color::TRANSPARENT);
    println!("\nred -> transparent:");
    println!("    t     straight Oklab (r,g,b,a)   ColorTween (r,g,b,a)");
    for step in 0u8..=10 {
        let t = f32::from(step) / 10.0;
        let s = straight_oklab_mix(red, Color::TRANSPARENT, t);
        let p = fade.transform(f64::from(t));
        println!(
            "  {t:4.1}   ({:3},{:3},{:3},{:3})        ({:3},{:3},{:3},{:3})",
            s.r, s.g, s.b, s.a, p.r, p.g, p.b, p.a,
        );
    }
}
