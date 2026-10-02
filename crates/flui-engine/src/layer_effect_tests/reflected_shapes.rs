//! Reflections keep dimensions positive and preserve each rounded corner.
use super::*;
use flui_foundation::geometry::{RRect, Radius};

fn shape_scene(matrix: Matrix4, bounds: Rect<f64>, radii: Option<[f64; 4]>) -> LayerTree {
    let mut canvas = Canvas::new();
    let paint = Paint::fill(Color::RED);
    if let Some(radii) = radii {
        let [tl, tr, br, bl] = radii.map(|radius| Radius::new(radius, radius));
        canvas.draw_rrect(RRect::new(bounds, tl, tr, br, bl), &paint);
    } else {
        canvas.draw_rect(bounds, &paint.with_anti_alias(false));
    }
    let mut tree = LayerTree::new(Layer::from(TransformLayer::new(matrix)));
    tree.push_child(tree.root(), Layer::from(PictureLayer::new(canvas.finish())));
    tree
}
fn matches_baked(
    renderer: &HeadlessRenderer,
    source: LayerTree,
    reference: LayerTree,
    centre: (u32, u32),
) {
    let expected = capture(renderer, &reference);
    let actual = capture(renderer, &source);
    expect_pixel(&expected, centre, [255, 0, 0, 255]);
    assert!(
        actual
            .iter()
            .zip(&expected)
            .all(|(&a, &b)| a.abs_diff(b) <= 2),
        "affine reflected/scaled shape must match independently baked bounds and corners"
    );
}
pub(super) fn reflected_shapes_and_scaled_radii_match_baked(renderer: &HeadlessRenderer) {
    let flip = Matrix4::translation(64.0, 0.0, 0.0) * Matrix4::scaling(-1.0, 1.0, 1.0);
    let bounds = Rect::from_xywh(10.0, 14.0, 30.0, 24.0);
    let baked = Rect::from_xywh(24.0, 14.0, 30.0, 24.0);
    matches_baked(
        renderer,
        shape_scene(flip, bounds, None),
        shape_scene(Matrix4::IDENTITY, baked, None),
        (35, 25),
    );
    let corners = [10.0, 2.0, 6.0, 0.0];
    let swapped = [2.0, 10.0, 0.0, 6.0];
    let reference = shape_scene(Matrix4::IDENTITY, baked, Some(swapped));
    expect_pixel(&capture(renderer, &reference), (52, 16), [255; 4]);
    matches_baked(
        renderer,
        shape_scene(flip, bounds, Some(corners)),
        reference,
        (35, 25),
    );
    let scaled = Matrix4::scaling(2.0, 2.0, 1.0);
    let local = Rect::from_xywh(8.0, 7.0, 16.0, 18.0);
    let reference = shape_scene(
        Matrix4::IDENTITY,
        Rect::from_xywh(16.0, 14.0, 32.0, 36.0),
        Some([12.0, 4.0, 8.0, 0.0]),
    );
    expect_pixel(&capture(renderer, &reference), (17, 15), [255; 4]);
    matches_baked(
        renderer,
        shape_scene(scaled, local, Some([6.0, 2.0, 4.0, 0.0])),
        reference,
        (30, 30),
    );
    // Affine reflections rebase the large local origin before packing f32.
    let far = Matrix4::translation(1_000_000_064.0, 0.0, 0.0) * Matrix4::scaling(-1.0, 1.0, 1.0);
    matches_baked(
        renderer,
        shape_scene(
            far,
            Rect::from_xywh(1_000_000_010.0, 14.0, 30.0, 24.0),
            None,
        ),
        shape_scene(Matrix4::IDENTITY, baked, None),
        (35, 25),
    );
}
