use flui_painting::{GlyphContent, GlyphImage};

fn main() {
    let _image = GlyphImage {
        left: 0,
        top: 0,
        width: 2,
        height: 2,
        content: GlyphContent::Mask,
        data: vec![255],
    };
}
