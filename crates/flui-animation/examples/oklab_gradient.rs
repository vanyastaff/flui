//! How `Tween<Color>` interpolates: Oklab with premultiplied alpha.
//!
//! Prints two transitions. Blue → yellow compares a gamma-sRGB channel mix, which dips
//! through a dark grey midpoint, with `Tween<Color>`'s Oklab mix, which keeps perceived
//! lightness steady. Red → transparent compares a straight-alpha Oklab mix, which blends
//! in the transparent end's black and darkens, with `Tween<Color>`'s premultiplied mix,
//! which stays red while it fades.
//!
//! Run with: `cargo run -p flui-animation --example oklab_gradient`

use flui_animation::{Animatable, Tween};
use flui_painting::styling::{Color, Oklab};

/// A gamma-sRGB channel mix, for comparison.
fn srgb_mix(from: Color, to: Color, t: f32) -> Color {
    let from = from.to_rgba_f32_array();
    let to = to.to_rgba_f32_array();
    Color::from_rgba_f32_array(std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t))
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
    Color::from_oklab(lab, 255).with_opacity(f64::from(mix(from.opacity(), to.opacity())))
}

fn brightness(c: Color) -> u16 {
    let [r, g, b, _] = c.to_rgba8();
    u16::from(r) + u16::from(g) + u16::from(b)
}

fn main() {
    let (blue, yellow) = (Color::rgb(0, 0, 255), Color::rgb(255, 255, 0));
    let across = Tween::new(blue, yellow);
    println!("blue -> yellow:");
    println!("    t     sRGB (r,g,b)  sum    Tween (r,g,b)  sum");
    for step in 0u8..=10 {
        let t = f32::from(step) / 10.0;
        let (s, o) = (srgb_mix(blue, yellow, t), across.transform(f64::from(t)));
        let ([sr, sg, sb, _], [or, og, ob, _]) = (s.to_rgba8(), o.to_rgba8());
        println!(
            "  {t:4.1}   ({:3},{:3},{:3})  {:4}   ({:3},{:3},{:3})       {:4}",
            sr,
            sg,
            sb,
            brightness(s),
            or,
            og,
            ob,
            brightness(o),
        );
    }

    let red = Color::rgb(255, 0, 0);
    let fade = Tween::new(red, Color::TRANSPARENT);
    println!("\nred -> transparent:");
    println!("    t     straight Oklab (r,g,b,a)   Tween (r,g,b,a)");
    for step in 0u8..=10 {
        let t = f32::from(step) / 10.0;
        let s = straight_oklab_mix(red, Color::TRANSPARENT, t);
        let p = fade.transform(f64::from(t));
        let ([sr, sg, sb, sa], [pr, pg, pb, pa]) = (s.to_rgba8(), p.to_rgba8());
        println!("  {t:4.1}   ({sr:3},{sg:3},{sb:3},{sa:3})        ({pr:3},{pg:3},{pb:3},{pa:3})");
    }
}
