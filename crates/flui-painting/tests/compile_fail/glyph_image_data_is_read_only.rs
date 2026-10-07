use flui_painting::{GlyphContent, GlyphImage};

fn main() {
    let mut image = GlyphImage::try_new(0, 0, 1, 1, GlyphContent::Mask, vec![255])
        .expect("valid mask");
    let image = &mut image;
    image.data()[0] = 0;
}
