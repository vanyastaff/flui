//! Two-circle witnesses use points on independently chosen circles, not a CPU quadratic solver.
use super::*;
use flui_painting::{
    Alignment,
    decoration::{DecorationPaintOptions, paint_box_decoration},
    styling::{BoxDecoration, Gradient, RadialGradient},
};

fn radial(
    center: Offset<f64>,
    radius: f64,
    focal: Option<Offset<f64>>,
    focal_radius: Option<f64>,
    tile_mode: TileMode,
) -> Shader {
    Shader::RadialGradient {
        center,
        radius,
        colors: vec![Color::RED, Color::BLUE],
        stops: None,
        focal,
        focal_radius,
        tile_mode,
    }
}
fn draw(renderer: &HeadlessRenderer, shader: Shader, mode: BlendMode) -> Vec<u8> {
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
        &Paint::fill(Color::WHITE)
            .with_shader(shader)
            .with_blend_mode(mode)
            .with_anti_alias(false),
    );
    let mut tree = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::WHITE));
    tree.push_child(tree.root(), Layer::from(PictureLayer::new(canvas.finish())));
    capture(renderer, &tree)
}
fn focal(renderer: &HeadlessRenderer, mode: BlendMode) {
    let bytes = draw(
        renderer,
        radial(
            Offset::new(24.5, 24.5),
            16.0,
            Some(Offset::new(16.5, 24.5)),
            None,
            TileMode::Clamp,
        ),
        mode,
    );
    // The first circle is a point, exactly at this pixel centre.
    expect_pixel(&bytes, (16, 24), [255, 0, 0, 255]);
    // At t=.5 the circle centre is20.5 and radius8: its rightmost point is28.5.
    expect_pixel(&bytes, (28, 24), [128, 0, 128, 255]);
}
pub(super) fn ordinary_focal(renderer: &HeadlessRenderer) {
    focal(renderer, BlendMode::SrcOver);
}
pub(super) fn advanced_focal(renderer: &HeadlessRenderer) {
    focal(renderer, BlendMode::Multiply);
}
fn initial_circle(renderer: &HeadlessRenderer, mode: BlendMode) {
    let bytes = draw(
        renderer,
        radial(
            Offset::new(24.5, 24.5),
            16.0,
            Some(Offset::new(16.5, 24.5)),
            Some(4.0),
            TileMode::Clamp,
        ),
        mode,
    );
    expect_pixel(&bytes, (20, 24), [255, 0, 0, 255]);
    // At t=.5: centre20.5, radius10, rightmost point30.5.
    expect_pixel(&bytes, (30, 24), [128, 0, 128, 255]);
}
pub(super) fn ordinary_initial_circle(renderer: &HeadlessRenderer) {
    initial_circle(renderer, BlendMode::SrcOver);
}
pub(super) fn advanced_initial_circle(renderer: &HeadlessRenderer) {
    initial_circle(renderer, BlendMode::Multiply);
}
pub(super) fn concentric_initial_radius(renderer: &HeadlessRenderer) {
    for mode in [BlendMode::SrcOver, BlendMode::Multiply] {
        let bytes = draw(
            renderer,
            radial(
                Offset::new(24.5, 24.5),
                16.0,
                None,
                Some(4.0),
                TileMode::Clamp,
            ),
            mode,
        );
        // Radius10 halfway between4 and16.
        expect_pixel(&bytes, (34, 24), [128, 0, 128, 255]);
    }
}
pub(super) fn linear_circle_equation(renderer: &HeadlessRenderer) {
    for mode in [BlendMode::SrcOver, BlendMode::Multiply] {
        let bytes = draw(
            renderer,
            radial(
                Offset::new(32.5, 24.5),
                16.0,
                Some(Offset::new(16.5, 24.5)),
                Some(0.0),
                TileMode::Clamp,
            ),
            mode,
        );
        // Centre20.5/radius4 at t=.25, rightmost point24.5.
        expect_pixel(&bytes, (24, 24), [191, 0, 64, 255]);
        // At the shared tangent point every interpolated circle contains P;
        // there is no unique parameter. The orthogonal point has no solution.
        expect_pixel(&bytes, (16, 24), [255; 4]);
        expect_pixel(&bytes, (16, 32), [255; 4]);
    }
}
pub(super) fn repeated_root_and_missing_cone(renderer: &HeadlessRenderer) {
    for mode in [BlendMode::SrcOver, BlendMode::Multiply] {
        let bytes = draw(
            renderer,
            radial(
                Offset::new(32.5, 24.5),
                8.0,
                Some(Offset::new(16.5, 24.5)),
                Some(8.0),
                TileMode::Clamp,
            ),
            mode,
        );
        // Tangency at t=.5 has a repeated root; y40.5 lies outside every circle.
        expect_pixel(&bytes, (24, 32), [128, 0, 128, 255]);
        expect_pixel(&bytes, (24, 40), [255; 4]);
        // Two valid roots at t=-.25 and .75: the larger circle parameter wins.
        expect_pixel(&bytes, (20, 24), [64, 0, 191, 255]);
    }
    let healthy = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN));
    expect_pixel(&capture(renderer, &healthy), (24, 40), [0, 255, 0, 255]);
}
pub(super) fn radial_tiling(renderer: &HeadlessRenderer) {
    for (tile, expected) in [
        (TileMode::Clamp, [0, 0, 255, 255]),
        (TileMode::Repeat, [191, 0, 64, 255]),
        (TileMode::Mirror, [191, 0, 64, 255]),
        (TileMode::Decal, [255; 4]),
    ] {
        for mode in [BlendMode::SrcOver, BlendMode::Multiply] {
            let bytes = draw(
                renderer,
                radial(Offset::new(16.5, 24.5), 16.0, None, None, tile),
                mode,
            );
            // Radius36 -> t2.25: repeat and mirror both give .25.
            expect_pixel(&bytes, (52, 24), expected);
            if tile == TileMode::Mirror {
                // Radius20 -> t1.25: mirror .75, unlike repeat .25.
                expect_pixel(&bytes, (36, 24), [64, 0, 191, 255]);
            }
        }
    }
}
pub(super) fn centered_defaults_and_zero_radius(renderer: &HeadlessRenderer) {
    for mode in [BlendMode::SrcOver, BlendMode::Multiply] {
        let bytes = draw(
            renderer,
            radial(Offset::new(24.5, 24.5), 16.0, None, None, TileMode::Clamp),
            mode,
        );
        expect_pixel(&bytes, (32, 24), [128, 0, 128, 255]);
        let zero = draw(
            renderer,
            radial(Offset::new(24.5, 24.5), 0.0, None, None, TileMode::Clamp),
            mode,
        );
        expect_pixel(&zero, (32, 24), [255, 0, 0, 255]);
    }
}
pub(super) fn decoration_preserves_both_circles(renderer: &HeadlessRenderer) {
    let gradient = RadialGradient::new(
        Alignment::new(0.03125, 0.03125),
        0.5,
        vec![Color::RED, Color::BLUE],
        None,
        TileMode::Clamp,
        Some(Alignment::new(-0.46875, 0.03125)),
        Some(0.125),
    );
    let decoration = BoxDecoration::with_gradient(Gradient::Radial(gradient));
    let mut canvas = Canvas::new();
    paint_box_decoration(
        &mut canvas,
        Rect::from_xywh(8.0, 8.0, 32.0, 32.0),
        &decoration,
        DecorationPaintOptions::with_anti_alias(false),
    );
    let bytes = capture(
        renderer,
        &LayerTree::new(Layer::from(PictureLayer::new(canvas.finish()))),
    );
    expect_pixel(&bytes, (20, 24), [255, 0, 0, 255]);
    expect_pixel(&bytes, (30, 24), [128, 0, 128, 255]);
}
pub(super) fn radial_refusal_and_next_frame(renderer: &HeadlessRenderer) {
    let center = Offset::new(24.5, 24.5);
    let inputs = [
        radial(
            center,
            16.0,
            Some(Offset::new(f64::NAN, 24.5)),
            None,
            TileMode::Clamp,
        ),
        radial(center, 16.0, None, Some(-1.0), TileMode::Clamp),
        radial(center, 16.0, None, Some(f64::INFINITY), TileMode::Clamp),
        radial(center, 16.0, None, Some(16.0), TileMode::Clamp),
        radial(center, -1.0, None, None, TileMode::Clamp),
        radial(center, 1e100, None, None, TileMode::Clamp),
    ];
    let healthy = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN));
    let mut failed = Vec::new();
    for (index, shader) in inputs.into_iter().enumerate() {
        for mode in [BlendMode::SrcOver, BlendMode::Multiply] {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut canvas = Canvas::new();
                canvas.draw_rect(
                    Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
                    &Paint::fill(Color::WHITE)
                        .with_shader(shader.clone())
                        .with_blend_mode(mode),
                );
                let tree = LayerTree::new(Layer::from(PictureLayer::new(canvas.finish())));
                assert!(matches!(
                    renderer.render_layer_tree(&tree, (SIDE, SIDE)),
                    Err(crate::EngineError::InvalidGeometry(_))
                ));
                expect_pixel(&capture(renderer, &healthy), (24, 24), [0, 255, 0, 255]);
            }));
            if outcome.is_err() {
                failed.push((index, mode));
            }
        }
    }
    assert!(failed.is_empty(), "radial refusal/recovery: {failed:?}");
}

pub(super) fn affine_cropped_radial(renderer: &HeadlessRenderer) {
    for mode in [BlendMode::SrcOver, BlendMode::Multiply] {
        let mut tree = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::WHITE));
        let mask = tree.push_child(
            tree.root(),
            Layer::from(ShaderMaskLayer::new(
                Shader::solid(Color::WHITE),
                BlendMode::DstIn,
                Rect::from_xywh(4.0, 4.0, 60.0, 60.0),
            )),
        );
        let transformed = tree.push_child(
            mask,
            Layer::from(TransformLayer::new(
                Matrix4::translation(0.5, -16.5, 0.0) * Matrix4::scaling(2.0, 2.0, 1.0),
            )),
        );
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
            &Paint::fill(Color::WHITE)
                .with_shader(radial(
                    Offset::new(24.5, 24.5),
                    16.0,
                    Some(Offset::new(16.5, 24.5)),
                    None,
                    TileMode::Clamp,
                ))
                .with_blend_mode(mode),
        );
        tree.push_child(transformed, Layer::from(PictureLayer::new(canvas.finish())));
        let bytes = capture(renderer, &tree);
        expect_pixel(&bytes, (33, 32), [255, 0, 0, 255]);
        expect_pixel(&bytes, (57, 32), [128, 0, 128, 255]);
    }
}
