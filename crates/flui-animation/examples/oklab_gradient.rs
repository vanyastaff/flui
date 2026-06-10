//! Perceptual (Oklab) vs componentwise-sRGB color interpolation.
//!
//! Prints the blue→yellow transition both ways. The sRGB path dips through a
//! dark gray midpoint (the classic "muddy gradient"); the Oklab path keeps
//! perceived lightness steady — this is what `OklabColorTween` gives every
//! color animation for free.
//!
//! Run with: `cargo run -p flui-animation --example oklab_gradient`

use flui_animation::{Animatable, ColorTween, OklabColorTween};
use flui_types::Color;

fn brightness(c: Color) -> u16 {
    u16::from(c.r) + u16::from(c.g) + u16::from(c.b)
}

fn main() {
    println!("FLUI Oklab color tween example\n");

    let blue = Color::rgb(0, 0, 255);
    let yellow = Color::rgb(255, 255, 0);

    let srgb = ColorTween::new(blue, yellow);
    let oklab = OklabColorTween::new(blue, yellow);

    println!("blue -> yellow, 11 steps:");
    println!("    t     sRGB (r,g,b)  sum    Oklab (r,g,b)  sum");
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        let s = srgb.transform(t);
        let o = oklab.transform(t);
        println!(
            "  {t:4.1}   ({:3},{:3},{:3})  {:4}   ({:3},{:3},{:3})  {:4}",
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

    let s_mid = srgb.transform(0.5);
    let o_mid = oklab.transform(0.5);
    println!(
        "\nMidpoint brightness: sRGB {} vs Oklab {} — the perceptual path never goes muddy.",
        brightness(s_mid),
        brightness(o_mid)
    );
}
