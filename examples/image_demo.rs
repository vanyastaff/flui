//! `image_demo` — end-to-end visual check of [`RenderImage`].
//!
//! Decodes a real JPEG/PNG, builds a [`RenderImage`] for several
//! [`ImageFit`] modes, and software-composites the scaled/aligned result
//! into PNG files using the *same* fit + alignment math the GPU paint path
//! uses ([`RenderImage::compute_size`] + [`RenderImage::paint_rect_in`]).
//!
//! This produces viewable artifacts proving the layout + paint geometry is
//! correct on a real image, without needing a GPU surface.
//!
//! # Usage
//!
//! ```bash
//! # Default: reads %TEMP%/flui_test_cat.jpg, writes target/image_demo/*.png
//! cargo run --example image_demo
//!
//! # Explicit input / output dir:
//! cargo run --example image_demo -- path\to\photo.jpg out_dir
//! ```

use std::path::PathBuf;

use flui_rendering::constraints::BoxConstraints;
use flui_rendering::objects::{ImageAlignment, ImageFit, RenderImage};
use flui_types::geometry::px;
use flui_types::painting::Image as FluiImage;
use flui_types::{Rect, Size};

/// Output canvas dimensions (the "box" we lay the image into).
const BOX_W: u32 = 480;
const BOX_H: u32 = 320;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("flui_test_cat.jpg"));
    let out_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/image_demo"));

    std::fs::create_dir_all(&out_dir)?;

    println!("Decoding {} ...", input.display());
    let decoded = image::open(&input)?.to_rgba8();
    let (iw, ih) = decoded.dimensions();
    println!("Source image: {iw}x{ih} ({} bytes RGBA)", decoded.len());

    // Build a flui Image handle from the decoded RGBA8 bytes.
    let flui_image = FluiImage::from_rgba8(iw, ih, decoded.as_raw().clone());

    // Box constraints: tight to the output canvas (like a fixed-size widget).
    let constraints = BoxConstraints {
        min_width: px(0.0),
        max_width: px(BOX_W as f32),
        min_height: px(0.0),
        max_height: px(BOX_H as f32),
    };
    let box_size = Size::new(px(BOX_W as f32), px(BOX_H as f32));

    let modes = [
        ("fill", ImageFit::Fill),
        ("contain", ImageFit::Contain),
        ("cover", ImageFit::Cover),
        ("scaledown", ImageFit::ScaleDown),
        ("none", ImageFit::None),
    ];

    for (name, fit) in modes {
        let render = RenderImage::from_image(flui_image.clone(), fit, ImageAlignment::Center);

        let laid_out = render.compute_size(&constraints);
        let dst = render
            .paint_rect_in(box_size)
            .expect("non-degenerate image has a paint rect");

        println!(
            "{name:>9}: layout_size={:.0}x{:.0}  paint_rect=({:.0},{:.0} {:.0}x{:.0})",
            laid_out.width.get(),
            laid_out.height.get(),
            dst.origin().x.get(),
            dst.origin().y.get(),
            dst.size().width.get(),
            dst.size().height.get(),
        );

        let canvas = composite(&decoded, dst, BOX_W, BOX_H);
        let path = out_dir.join(format!("cat_{name}.png"));
        canvas.save(&path)?;
        println!("           wrote {}", path.display());
    }

    println!("\nDone. Open the PNGs in {} to inspect.", out_dir.display());
    Ok(())
}

/// Software-composites `src` into a `box_w x box_h` RGBA canvas at the
/// destination rect `dst` (nearest-neighbour sampling), over a checkerboard
/// background so letterboxing/cropping is visible.
fn composite(src: &image::RgbaImage, dst: Rect, box_w: u32, box_h: u32) -> image::RgbaImage {
    let (sw, sh) = src.dimensions();
    let mut out = image::RgbaImage::new(box_w, box_h);

    // Checkerboard background (so transparent / letterbox areas are obvious).
    for (x, y, px_out) in out.enumerate_pixels_mut() {
        let checit = ((x / 16) + (y / 16)) % 2 == 0;
        let v = if checit { 200u8 } else { 170u8 };
        *px_out = image::Rgba([v, v, v, 255]);
    }

    let dx0 = dst.origin().x.get();
    let dy0 = dst.origin().y.get();
    let dw = dst.size().width.get();
    let dh = dst.size().height.get();
    if dw <= 0.0 || dh <= 0.0 {
        return out;
    }

    // Iterate over the destination rect's pixel coverage, clipped to canvas.
    let x_start = dx0.floor().max(0.0) as i64;
    let y_start = dy0.floor().max(0.0) as i64;
    let x_end = (dx0 + dw).ceil().min(box_w as f32) as i64;
    let y_end = (dy0 + dh).ceil().min(box_h as f32) as i64;

    for oy in y_start..y_end {
        for ox in x_start..x_end {
            // Map output pixel center back to source coordinates.
            let u = ((ox as f32 + 0.5) - dx0) / dw; // 0..1 across dst
            let v = ((oy as f32 + 0.5) - dy0) / dh;
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let sx = (u * sw as f32).floor().clamp(0.0, (sw - 1) as f32) as u32;
            let sy = (v * sh as f32).floor().clamp(0.0, (sh - 1) as f32) as u32;
            let sample = *src.get_pixel(sx, sy);
            out.put_pixel(ox as u32, oy as u32, sample);
        }
    }

    out
}
