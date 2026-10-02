//! Public capture witnesses for target-aware layer traversal.
use crate::headless::HeadlessRenderer;
use flui_foundation::geometry::{Matrix4, Offset, Rect, Size};
use flui_layer::{
    BackdropFilterLayer, FollowerLayer, Layer, LayerLink, LayerTree, LeaderLayer, OffsetLayer,
    PictureLayer, ShaderMaskLayer, TransformLayer,
};
use flui_painting::{
    BlendMode, Canvas, Paint, Shader,
    paint::{ImageFilter, TileMode},
    styling::Color,
};

const SIDE: u32 = 64;
fn picture(rect: Rect<f64>, color: Color) -> Layer {
    let mut canvas = Canvas::new();
    canvas.draw_rect(rect, &Paint::fill(color).with_anti_alias(false));
    Layer::from(PictureLayer::new(canvas.finish()))
}
fn pixel(bytes: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIDE + x) * 4) as usize;
    [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
}
fn expect_pixel(bytes: &[u8], at: (u32, u32), expected: [u8; 4]) {
    let actual = pixel(bytes, at.0, at.1);
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(&a, b)| a.abs_diff(b) <= 2),
        "{at:?}: {actual:?} versus {expected:?}"
    );
}
fn capture(renderer: &HeadlessRenderer, tree: &LayerTree) -> Vec<u8> {
    renderer
        .render_layer_tree(tree, (SIDE, SIDE))
        .expect("public layer capture")
}
// Named diagnostics complement the pixel oracles without replacing them.
fn dump_named(name: &str, rgba: &[u8]) {
    #[cfg(feature = "testing")]
    crate::readback_dump::dump_rgba_png(name, SIDE, SIDE, rgba);
    #[cfg(not(feature = "testing"))]
    let _ = (name, rgba);
}

fn mask_modes_and_tail(renderer: &HeadlessRenderer) {
    for (mode, shader, child, expected) in [
        (
            BlendMode::DstIn,
            Color::rgba(255, 255, 255, 128),
            true,
            [255, 127, 127, 255],
        ),
        (BlendMode::Src, Color::BLUE, false, [0, 0, 255, 255]),
        (BlendMode::Clear, Color::WHITE, true, [255; 4]),
    ] {
        let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
        let masked = tree.push_child(
            tree.root(),
            Layer::from(ShaderMaskLayer::new(
                Shader::solid(shader),
                mode,
                Rect::from_xywh(8.0, 8.0, 32.0, 32.0),
            )),
        );
        if child {
            tree.push_child(
                masked,
                picture(Rect::from_xywh(8.0, 8.0, 32.0, 32.0), Color::RED),
            );
        }
        tree.push_child(
            tree.root(),
            picture(Rect::from_xywh(48.0, 48.0, 8.0, 8.0), Color::GREEN),
        );
        let bytes = capture(renderer, &tree);
        expect_pixel(&bytes, (16, 16), expected);
        expect_pixel(&bytes, (4, 4), [255; 4]);
        expect_pixel(&bytes, (51, 51), [0, 255, 0, 255]);
    }
    let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
    let outer = tree.push_child(
        tree.root(),
        Layer::from(ShaderMaskLayer::new(
            Shader::solid(Color::WHITE),
            BlendMode::DstIn,
            Rect::from_xywh(8.0, 8.0, 32.0, 32.0),
        )),
    );
    let inner = tree.push_child(
        outer,
        Layer::from(ShaderMaskLayer::new(
            Shader::solid(Color::TRANSPARENT),
            BlendMode::DstIn,
            Rect::from_xywh(8.0, 8.0, 32.0, 32.0),
        )),
    );
    tree.push_child(
        inner,
        picture(Rect::from_xywh(8.0, 8.0, 32.0, 32.0), Color::RED),
    );
    expect_pixel(&capture(renderer, &tree), (16, 16), [255; 4]);
}
fn followers_resolve_and_hide(renderer: &HeadlessRenderer) {
    for linked in [true, false] {
        let link = LayerLink::new();
        let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
        if linked {
            let branch = tree.push_child(
                tree.root(),
                Layer::from(OffsetLayer::new(Offset::new(40.0, 4.0))),
            );
            tree.push_child(
                branch,
                Layer::from(LeaderLayer::with_offset(
                    link,
                    Size::new(8.0, 8.0),
                    Offset::new(2.0, 2.0),
                )),
            );
        }
        let branch = tree.push_child(
            tree.root(),
            Layer::from(OffsetLayer::new(Offset::new(4.0, 40.0))),
        );
        let follower = tree.push_child(
            branch,
            Layer::from(
                FollowerLayer::new(link)
                    .with_size(Size::new(8.0, 8.0))
                    .with_show_when_unlinked(false),
            ),
        );
        tree.push_child(
            follower,
            picture(Rect::from_xywh(0.0, 0.0, 8.0, 8.0), Color::RED),
        );
        tree.push_child(
            tree.root(),
            picture(Rect::from_xywh(52.0, 52.0, 8.0, 8.0), Color::GREEN),
        );
        let bytes = capture(renderer, &tree);
        expect_pixel(
            &bytes,
            (44, 8),
            if linked { [255, 0, 0, 255] } else { [255; 4] },
        );
        expect_pixel(&bytes, (6, 42), [255; 4]);
        expect_pixel(&bytes, (55, 55), [0, 255, 0, 255]);
    }
}
fn nested_backdrop_reads_child_and_keeps_tail(renderer: &HeadlessRenderer) {
    let mut tree = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN));
    let group = tree.push_child(
        tree.root(),
        Layer::from(ShaderMaskLayer::new(
            Shader::solid(Color::WHITE),
            BlendMode::DstIn,
            Rect::from_xywh(8.0, 8.0, 48.0, 48.0),
        )),
    );
    tree.push_child(
        group,
        picture(Rect::from_xywh(8.0, 8.0, 24.0, 48.0), Color::BLACK),
    );
    tree.push_child(
        group,
        picture(Rect::from_xywh(32.0, 8.0, 24.0, 48.0), Color::WHITE),
    );
    tree.push_child(
        group,
        Layer::from(BackdropFilterLayer::new(
            ImageFilter::blur(3.0),
            BlendMode::SrcOver,
            Rect::from_xywh(8.0, 8.0, 48.0, 48.0),
        )),
    );
    tree.push_child(
        group,
        picture(Rect::from_xywh(12.0, 12.0, 6.0, 6.0), Color::RED),
    );
    let bytes = capture(renderer, &tree);
    let fringe = pixel(&bytes, 31, 32);
    assert!(
        fringe[0] > 15
            && fringe[0] < 240
            && fringe[0].abs_diff(fringe[1]) <= 2
            && fringe[0].abs_diff(fringe[2]) <= 2,
        "isolated black/white backdrop produces gray, got {fringe:?}"
    );
    expect_pixel(&bytes, (14, 14), [255, 0, 0, 255]);
    expect_pixel(&bytes, (4, 32), [0, 255, 0, 255]);
}
fn anisotropic_backdrop_halo(renderer: &HeadlessRenderer) {
    for (sigma_x, sigma_y, halo, unchanged) in [
        (4.0, 0.0, (25, 31), (31, 25)),
        (0.0, 4.0, (31, 25), (25, 31)),
    ] {
        let mut tree = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::BLACK));
        tree.push_child(
            tree.root(),
            picture(Rect::from_xywh(30.0, 30.0, 4.0, 4.0), Color::WHITE),
        );
        tree.push_child(
            tree.root(),
            Layer::from(BackdropFilterLayer::new(
                ImageFilter::Blur { sigma_x, sigma_y },
                BlendMode::SrcOver,
                Rect::from_xywh(8.0, 8.0, 48.0, 48.0),
            )),
        );
        let bytes = capture(renderer, &tree);
        let blurred = pixel(&bytes, halo.0, halo.1);
        assert!(
            blurred[0] > 5,
            "directional halo missing for ({sigma_x},{sigma_y}): {blurred:?}"
        );
        expect_pixel(&bytes, unchanged, [0, 0, 0, 255]);
    }
    // A transparent child target has its white strip at attachment x=0. Its first four columns
    // are white. Src replaces that original paint with the blur, making the
    // missing samples beyond the attachment edge observable rather than hidden
    // behind the original opaque paint by SrcOver.
    let mut tree = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::BLACK));
    let child = tree.push_child(
        tree.root(),
        Layer::from(ShaderMaskLayer::new(
            Shader::solid(Color::WHITE),
            BlendMode::DstIn,
            Rect::from_xywh(0.0, 8.0, 48.0, 48.0),
        )),
    );
    tree.push_child(
        child,
        picture(Rect::from_xywh(0.0, 8.0, 4.0, 48.0), Color::WHITE),
    );
    tree.push_child(
        child,
        Layer::from(BackdropFilterLayer::new(
            ImageFilter::Blur {
                sigma_x: 4.0,
                sigma_y: 0.0,
            },
            BlendMode::Src,
            Rect::from_xywh(0.0, 8.0, 48.0, 48.0),
        )),
    );
    let bytes = capture(renderer, &tree);
    // The shipped Gaussian is normalized over ceil(sqrt(3)*sigma) integer
    // taps. At the first texel only offsets 0..3 read white; negative offsets
    // read transparent decal. Clamping would also count every negative tap.
    let weight = |i: i32| (-0.5 * f64::from(i * i) / 16.0).exp();
    let total: f64 = (-7..=7).map(weight).sum();
    let included: f64 = (0..=3).map(weight).sum();
    let expected = (255.0 * included / total).round() as u8;
    expect_pixel(&bytes, (0, 32), [expected, expected, expected, 255]);
    expect_pixel(&bytes, (52, 32), [0, 0, 0, 255]);
}

fn rotated_nonconstant_mask_shaders(renderer: &HeadlessRenderer) {
    let colors = vec![Color::RED, Color::BLUE];
    for shader in [
        Shader::RadialGradient {
            center: Offset::new(20.0, 24.0),
            radius: 32.0,
            colors: colors.clone(),
            stops: None,
            tile_mode: TileMode::Clamp,
            focal: None,
            focal_radius: None,
        },
        Shader::SweepGradient {
            center: Offset::new(24.0, 24.0),
            colors,
            stops: None,
            tile_mode: TileMode::Clamp,
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
        },
    ] {
        let build = |matrix| {
            let mut tree = LayerTree::new(Layer::from(TransformLayer::new(matrix)));
            let masked = tree.push_child(
                tree.root(),
                Layer::from(ShaderMaskLayer::new(
                    shader.clone(),
                    BlendMode::Src,
                    Rect::from_xywh(8.0, 8.0, 48.0, 48.0),
                )),
            );
            tree.push_child(
                masked,
                picture(Rect::from_xywh(8.0, 8.0, 48.0, 48.0), Color::WHITE),
            );
            tree
        };
        let original = capture(renderer, &build(Matrix4::IDENTITY));
        let matrix =
            Matrix4::translation(64.0, 0.0, 0.0) * Matrix4::rotation_z(std::f64::consts::FRAC_PI_2);
        let rotated = capture(renderer, &build(matrix));
        assert_ne!(
            pixel(&original, 16, 20),
            pixel(&original, 40, 36),
            "nonconstant shader witness"
        );
        for (x, y) in [(16, 20), (40, 36), (24, 40)] {
            expect_pixel(&rotated, (x, y), pixel(&original, y, 63 - x));
        }
    }
}

fn fractional_dpr_clip_and_mask(renderer: &HeadlessRenderer) {
    for dpr in [1.0, 1.5, 2.0] {
        let matrix = Matrix4::scaling(dpr, dpr, 1.0) * Matrix4::translation(0.25, 0.5, 0.0);
        let mut tree = LayerTree::new(Layer::from(TransformLayer::new(matrix)));
        let clip = tree.push_child(
            tree.root(),
            Layer::from(flui_layer::ClipRectLayer::hard_edge(Rect::from_xywh(
                8.25, 8.25, 16.0, 16.0,
            ))),
        );
        let mask = tree.push_child(
            clip,
            Layer::from(ShaderMaskLayer::new(
                Shader::solid(Color::rgba(255, 255, 255, 128)),
                BlendMode::DstIn,
                Rect::from_xywh(0.0, 0.0, 32.0, 32.0),
            )),
        );
        tree.push_child(
            mask,
            picture(Rect::from_xywh(0.0, 0.0, 32.0, 32.0), Color::BLUE),
        );
        let bytes = capture(renderer, &tree);
        expect_pixel(
            &bytes,
            ((16.5 * dpr) as u32, (16.5 * dpr) as u32),
            [127, 127, 255, 255],
        );
        expect_pixel(&bytes, ((4.5 * dpr) as u32, (16.5 * dpr) as u32), [255; 4]);
        expect_pixel(&bytes, ((28.5 * dpr) as u32, (16.5 * dpr) as u32), [255; 4]);
    }
}
fn partial_mask_matches_public_capture(renderer: &HeadlessRenderer) {
    use crate::{damage::FramePlan, raster::RasterBackend};
    let mut tree = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN));
    let mask = tree.push_child(
        tree.root(),
        Layer::from(ShaderMaskLayer::new(
            Shader::solid(Color::rgba(255, 255, 255, 128)),
            BlendMode::DstIn,
            Rect::from_xywh(16.0, 16.0, 32.0, 32.0),
        )),
    );
    tree.push_child(
        mask,
        picture(Rect::from_xywh(16.0, 16.0, 32.0, 32.0), Color::RED),
    );
    let full = capture(renderer, &tree);
    let scene = flui_layer::Scene::new(tree);
    let mut retained = renderer
        .retained_capture((SIDE, SIDE))
        .expect("retained capture");
    retained.mark_full_repaint();
    retained.render_scene(&scene).expect("direct first frame");
    retained.mark_dirty(Rect::from_xywh(0.0, 0.0, 1.0, 1.0));
    retained.render_scene(&scene).expect("seed retained target");
    assert_eq!(retained.last_plan(), Some(FramePlan::RetainedFull));
    let before = retained.read_rgba().expect("seed readback");
    retained.mark_dirty(Rect::from_xywh(20.0, 20.0, 4.0, 4.0));
    retained.render_scene(&scene).expect("partial mask replay");
    assert!(matches!(
        retained.last_plan(),
        Some(FramePlan::RetainedPartial(_))
    ));
    let partial = retained.read_rgba().expect("partial readback");
    expect_pixel(&partial, (40, 40), pixel(&before, 40, 40));
    assert!(
        partial.iter().zip(&full).all(|(&a, &b)| a.abs_diff(b) <= 2),
        "retained partial equals public full capture"
    );
}

fn nested_mask_failure_recovers(renderer: &HeadlessRenderer) {
    let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
    let mask = tree.push_child(
        tree.root(),
        Layer::from(ShaderMaskLayer::new(
            Shader::solid(Color::WHITE),
            BlendMode::DstIn,
            Rect::from_xywh(8.0, 8.0, 32.0, 32.0),
        )),
    );
    let mut canvas = Canvas::new();
    canvas.draw_texture(
        flui_painting::paint::TextureId::new(99999),
        Rect::from_xywh(8.0, 8.0, 32.0, 32.0),
        None,
        flui_painting::paint::FilterQuality::None,
        1.0,
    );
    tree.push_child(mask, Layer::from(PictureLayer::new(canvas.finish())));
    let error = renderer
        .render_layer_tree(&tree, (SIDE, SIDE))
        .expect_err("missing masked texture is not silently ignored");
    assert!(
        matches!(
            error,
            crate::EngineError::ExternalTexture(crate::ExternalTextureError::UnknownTexture {
                id: 99999
            })
        ),
        "first record error remains authoritative: {error:?}"
    );
    let next = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN));
    expect_pixel(&capture(renderer, &next), (16, 16), [0, 255, 0, 255]);
}

fn backdrop_dpr_and_offset_preserve_pixels(renderer: &HeadlessRenderer) {
    let build = |device_space: bool| {
        let mut tree = LayerTree::new(Layer::from(TransformLayer::new(if device_space {
            Matrix4::IDENTITY
        } else {
            Matrix4::scaling(2.0, 2.0, 1.0) * Matrix4::translation(3.0, 5.0, 0.0)
        })));
        // Root black covers the frame independently of the transformed subgroup.
        tree.push_child(
            tree.root(),
            picture(Rect::from_xywh(-64.0, -64.0, 128.0, 128.0), Color::BLACK),
        );
        tree.push_child(
            tree.root(),
            picture(
                if device_space {
                    Rect::from_xywh(34.0, 38.0, 4.0, 4.0)
                } else {
                    Rect::from_xywh(14.0, 14.0, 2.0, 2.0)
                },
                Color::WHITE,
            ),
        );
        tree.push_child(
            tree.root(),
            Layer::from(BackdropFilterLayer::new(
                ImageFilter::blur(if device_space { 4.0 } else { 2.0 }),
                BlendMode::SrcOver,
                if device_space {
                    Rect::from_xywh(22.0, 26.0, 32.0, 32.0)
                } else {
                    Rect::from_xywh(8.0, 8.0, 16.0, 16.0)
                },
            )),
        );
        tree
    };
    let actual = capture(renderer, &build(false));
    let expected = capture(renderer, &build(true));
    assert!(
        pixel(&actual, 30, 39)[0] > 2,
        "DPR blur produces a physical halo"
    );
    assert!(
        actual
            .iter()
            .zip(&expected)
            .all(|(&a, &b)| a.abs_diff(b) <= 2),
        "DPR2 plus offset matches independently baked device geometry and sigma"
    );
}
fn competing_effect_errors_and_limits_recover(renderer: &HeadlessRenderer) {
    let valid = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN));
    let linear = |tile_mode, colors, stops| {
        Shader::linear_gradient(
            Offset::ZERO,
            Offset::new(8.0, 0.0),
            colors,
            stops,
            tile_mode,
        )
    };
    let radial = |focal, focal_radius| {
        Shader::radial_gradient(
            Offset::new(4.0, 4.0),
            4.0,
            vec![Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
            focal,
            focal_radius,
        )
    };
    let cases = [
        ("unsupported backdrop", None),
        (
            "image mask",
            Some(Shader::Image(flui_painting::paint::ImageShader::new(
                TileMode::Clamp,
                TileMode::Clamp,
            ))),
        ),
        (
            "repeat linear mask",
            Some(linear(
                TileMode::Repeat,
                vec![Color::RED, Color::BLUE],
                None,
            )),
        ),
        (
            "mirror sweep mask",
            Some(Shader::sweep_gradient(
                Offset::ZERO,
                vec![Color::RED, Color::BLUE],
                None,
                TileMode::Mirror,
                0.0,
                std::f64::consts::TAU,
            )),
        ),
        (
            "focal radial mask",
            Some(radial(Some(Offset::new(2.0, 4.0)), None)),
        ),
        ("focal radius mask", Some(radial(None, Some(1.0)))),
        (
            "NaN radial center",
            Some(Shader::radial_gradient(
                Offset::new(f64::NAN, 4.0),
                4.0,
                vec![Color::RED, Color::BLUE],
                None,
                TileMode::Clamp,
                None,
                None,
            )),
        ),
        (
            "unrepresentable radial radius",
            Some(Shader::radial_gradient(
                Offset::new(4.0, 4.0),
                1e100,
                vec![Color::RED, Color::BLUE],
                None,
                TileMode::Clamp,
                None,
                None,
            )),
        ),
        (
            "empty gradient mask",
            Some(linear(TileMode::Clamp, vec![], None)),
        ),
        (
            "nonfinite mask stops",
            Some(linear(
                TileMode::Clamp,
                vec![Color::RED, Color::BLUE],
                Some(vec![0.0, f64::NAN]),
            )),
        ),
        (
            "decreasing mask stops",
            Some(linear(
                TileMode::Clamp,
                vec![Color::RED, Color::BLUE],
                Some(vec![1.0, 0.0]),
            )),
        ),
    ];
    for (case, shader) in cases {
        let shader_error = shader.is_some();
        for missing_first in [false, true] {
            let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
            let mut canvas = Canvas::new();
            canvas.draw_texture(
                flui_painting::paint::TextureId::new(99999),
                Rect::from_xywh(8.0, 8.0, 8.0, 8.0),
                None,
                flui_painting::paint::FilterQuality::None,
                1.0,
            );
            let missing = Layer::from(PictureLayer::new(canvas.finish()));
            let unsupported = if let Some(shader) = &shader {
                Layer::from(ShaderMaskLayer::new(
                    shader.clone(),
                    BlendMode::DstIn,
                    Rect::from_xywh(8.0, 8.0, 8.0, 8.0),
                ))
            } else {
                Layer::from(BackdropFilterLayer::new(
                    ImageFilter::Dilate { radius: 2.0 },
                    BlendMode::SrcOver,
                    Rect::from_xywh(8.0, 8.0, 8.0, 8.0),
                ))
            };
            let pair = if missing_first {
                [missing, unsupported]
            } else {
                [unsupported, missing]
            };
            for layer in pair {
                tree.push_child(tree.root(), layer);
            }
            let error = renderer
                .render_layer_tree(&tree, (SIDE, SIDE))
                .expect_err("competing errors refuse frame");
            let correct = if missing_first {
                matches!(
                    error,
                    crate::EngineError::ExternalTexture(
                        crate::ExternalTextureError::UnknownTexture { id: 99999 }
                    )
                )
            } else if shader_error {
                matches!(error, crate::EngineError::UnsupportedMaskShader)
            } else {
                matches!(error, crate::EngineError::UnsupportedBackdropFilter { .. })
            };
            assert!(
                correct,
                "chronological first error, case={case}, missing_first={missing_first}: {error:?}"
            );
            expect_pixel(&capture(renderer, &valid), (16, 16), [0, 255, 0, 255]);
        }
    }
    // Existing normalization remains usable: one-color gradients, omitted
    // trailing stops, clamp-normalized duplicates and zero focal radius.
    for shader in [
        linear(TileMode::Clamp, vec![Color::RED], None),
        linear(
            TileMode::Clamp,
            vec![Color::RED, Color::RED],
            Some(vec![0.0]),
        ),
        linear(
            TileMode::Clamp,
            vec![Color::RED; 3],
            Some(vec![-2.0, -1.0, 2.0]),
        ),
        Shader::radial_gradient(
            Offset::new(12.0, 12.0),
            4.0,
            vec![Color::RED],
            None,
            TileMode::Clamp,
            None,
            Some(0.0),
        ),
    ] {
        let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
        tree.push_child(
            tree.root(),
            Layer::from(ShaderMaskLayer::new(
                shader,
                BlendMode::Src,
                Rect::from_xywh(8.0, 8.0, 8.0, 8.0),
            )),
        );
        expect_pixel(&capture(renderer, &tree), (12, 12), [255, 0, 0, 255]);
    }
    let mut enormous = LayerTree::new(Layer::from(TransformLayer::new(Matrix4::scaling(
        1e40, 1e40, 1.0,
    ))));
    enormous.push_child(
        enormous.root(),
        Layer::from(ShaderMaskLayer::new(
            Shader::linear_gradient(
                Offset::ZERO,
                Offset::new(4e-39, 0.0),
                vec![Color::RED, Color::BLUE],
                None,
                TileMode::Clamp,
            ),
            BlendMode::Src,
            Rect::from_xywh(0.0, 0.0, 4e-39, 4e-39),
        )),
    );
    assert!(
        matches!(
            renderer.render_layer_tree(&enormous, (SIDE, SIDE)),
            Err(crate::EngineError::InvalidGeometry(
                crate::GeometryError::Unrepresentable { .. }
            ))
        ),
        "finite device bounds do not admit an infinite packed gradient transform"
    );
    expect_pixel(&capture(renderer, &valid), (16, 16), [0, 255, 0, 255]);
    let mut deep = LayerTree::new(Layer::from(OffsetLayer::zero()));
    let mut parent = deep.root();
    for _ in 0..65 {
        parent = deep.push_child(
            parent,
            Layer::from(ShaderMaskLayer::new(
                Shader::solid(Color::WHITE),
                BlendMode::DstIn,
                Rect::from_xywh(8.0, 8.0, 8.0, 8.0),
            )),
        );
    }
    deep.push_child(
        parent,
        picture(Rect::from_xywh(8.0, 8.0, 8.0, 8.0), Color::RED),
    );
    assert!(matches!(
        renderer.render_layer_tree(&deep, (SIDE, SIDE)),
        Err(crate::EngineError::PreparedResourceLimit {
            resource: "effect nesting",
            requested: 65,
            limit: 64
        })
    ));
    expect_pixel(&capture(renderer, &valid), (16, 16), [0, 255, 0, 255]);
    let mut huge = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::BLACK));
    huge.push_child(
        huge.root(),
        Layer::from(BackdropFilterLayer::new(
            ImageFilter::blur(1_000_000.0),
            BlendMode::SrcOver,
            Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
        )),
    );
    assert!(
        matches!(
            renderer.render_layer_tree(&huge, (SIDE, SIDE)),
            Err(crate::EngineError::PreparedResourceLimit {
                resource: "cumulative effect sampling work",
                ..
            })
        ),
        "huge sigma is rejected before filter encoding"
    );
    expect_pixel(&capture(renderer, &valid), (16, 16), [0, 255, 0, 255]);
}
fn arbitrary_affine_terminal_shader_matches_direct_paint(renderer: &HeadlessRenderer) {
    let colors = vec![Color::RED, Color::BLUE];
    let shaders = [
        Shader::LinearGradient {
            from: Offset::new(8.0, 8.0),
            to: Offset::new(40.0, 40.0),
            colors: colors.clone(),
            stops: None,
            tile_mode: TileMode::Clamp,
        },
        Shader::RadialGradient {
            center: Offset::new(20.0, 24.0),
            radius: 32.0,
            colors: colors.clone(),
            stops: None,
            tile_mode: TileMode::Clamp,
            focal: None,
            focal_radius: None,
        },
        Shader::SweepGradient {
            center: Offset::new(24.0, 24.0),
            colors,
            stops: None,
            tile_mode: TileMode::Clamp,
            start_angle: 0.0,
            end_angle: std::f64::consts::TAU,
        },
    ];
    for (kind, shader) in shaders.into_iter().enumerate() {
        let matrix = Matrix4::translation(32.0, 32.0, 0.0)
            * Matrix4::rotation_z(std::f64::consts::FRAC_PI_4)
            * Matrix4::scaling(1.2, 0.8, 1.0)
            * Matrix4::translation(-24.0, -24.0, 0.0);
        let mut direct = LayerTree::new(Layer::from(TransformLayer::new(matrix)));
        let mut canvas = Canvas::new();
        let mut paint = Paint::fill(Color::WHITE).with_anti_alias(false);
        paint.shader = Some(shader.clone());
        canvas.draw_rect(Rect::from_xywh(8.0, 8.0, 32.0, 32.0), &paint);
        direct.push_child(
            direct.root(),
            Layer::from(PictureLayer::new(canvas.finish())),
        );
        let mut masked = LayerTree::new(Layer::from(TransformLayer::new(matrix)));
        masked.push_child(
            masked.root(),
            Layer::from(ShaderMaskLayer::new(
                shader,
                BlendMode::Src,
                Rect::from_xywh(8.0, 8.0, 32.0, 32.0),
            )),
        );
        let expected = capture(renderer, &direct);
        let actual = capture(renderer, &masked);
        dump_named(&format!("layer-affine-{kind}-direct"), &expected);
        dump_named(&format!("layer-affine-{kind}-mask"), &actual);
        assert_ne!(
            pixel(&expected, 28, 28),
            pixel(&expected, 38, 34),
            "nonconstant affine witness"
        );
        for at in [(28, 28), (38, 34), (30, 36)] {
            expect_pixel(&actual, at, pixel(&expected, at.0, at.1));
        }
    }
}

fn partial_backdrop_reconstructs_halo_before_later_sibling(renderer: &HeadlessRenderer) {
    use crate::{damage::FramePlan, raster::RasterBackend};
    let build = |color| {
        let mut tree = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::BLACK));
        tree.push_child(
            tree.root(),
            picture(Rect::from_xywh(26.0, 30.0, 4.0, 4.0), color),
        );
        for x in [28.0, 38.0] {
            tree.push_child(
                tree.root(),
                Layer::from(BackdropFilterLayer::new(
                    ImageFilter::blur(3.0),
                    BlendMode::SrcOver,
                    Rect::from_xywh(x, 20.0, 12.0, 24.0),
                )),
            );
        }
        // This final red sibling is outside the original 4x4 damage but inside
        // a filter's sampling halo and output. Retained red must not be sampled
        // as if it had already painted before either backdrop operation.
        tree.push_child(
            tree.root(),
            picture(Rect::from_xywh(32.0, 28.0, 4.0, 8.0), Color::RED),
        );
        tree
    };
    let before = flui_layer::Scene::new(build(Color::WHITE));
    let after_tree = build(Color::BLUE);
    let full = capture(renderer, &after_tree);
    let after = flui_layer::Scene::new(after_tree);
    let mut retained = renderer
        .retained_capture((SIDE, SIDE))
        .expect("retained backdrop capture");
    retained.mark_full_repaint();
    retained
        .render_scene(&before)
        .expect("first backdrop frame");
    retained.mark_dirty(Rect::from_xywh(0.0, 0.0, 1.0, 1.0));
    retained
        .render_scene(&before)
        .expect("seed backdrop retained target");
    assert_eq!(retained.last_plan(), Some(FramePlan::RetainedFull));
    let baseline = retained.read_rgba().expect("before halo readback");
    assert_ne!(
        pixel(&baseline, 30, 31),
        pixel(&full, 30, 31),
        "changed preceding paint affects backdrop output"
    );
    retained.mark_dirty(Rect::from_xywh(26.0, 30.0, 4.0, 4.0));
    retained
        .render_scene(&after)
        .expect("partial backdrop dependency replay");
    let Some(FramePlan::RetainedPartial(_)) = retained.last_plan() else {
        panic!(
            "backdrop fixture must remain partial, got {:?}",
            retained.last_plan()
        )
    };
    let partial = retained.read_rgba().expect("partial halo readback");
    dump_named("layer-backdrop-full", &full);
    dump_named("layer-backdrop-partial", &partial);
    expect_pixel(&partial, (33, 31), [255, 0, 0, 255]);
    let stale = partial
        .as_chunks::<4>()
        .0
        .iter()
        .zip(full.as_chunks::<4>().0)
        .enumerate()
        .find(|(_, (a, b))| a.iter().zip(*b).any(|(&a, &b)| a.abs_diff(b) > 2));
    assert!(
        stale.is_none(),
        "expanded partial backdrop differs from full at {stale:?}"
    );
}

fn gradient_scale_and_large_origin_preserve_device_pixels(renderer: &HeadlessRenderer) {
    let shader = |kind: u8, origin: f64, extent: f64, constant: bool| {
        let colors = if constant {
            vec![Color::BLUE, Color::BLUE]
        } else {
            vec![Color::RED, Color::BLUE]
        };
        match kind {
            0 => Shader::LinearGradient {
                from: Offset::new(origin, 0.0),
                to: Offset::new(origin + extent, extent),
                colors,
                stops: None,
                tile_mode: TileMode::Clamp,
            },
            1 => Shader::RadialGradient {
                center: Offset::new(origin + extent * 0.25, extent * 0.375),
                radius: extent,
                colors,
                stops: None,
                tile_mode: TileMode::Clamp,
                focal: None,
                focal_radius: None,
            },
            _ => Shader::SweepGradient {
                center: Offset::new(origin + extent * 0.25, extent * 0.375),
                colors,
                stops: None,
                tile_mode: TileMode::Clamp,
                start_angle: 0.0,
                end_angle: std::f64::consts::TAU,
            },
        }
    };
    for kind in 0..3 {
        let rounded = |local: bool| {
            let extent = if local { 1000.0 } else { 10.0 };
            let radius = if local { 200.0 } else { 2.0 };
            let mut tree = LayerTree::new(Layer::from(TransformLayer::new(if local {
                Matrix4::scaling(0.01, 0.01, 1.0)
            } else {
                Matrix4::IDENTITY
            })));
            let mut canvas = Canvas::new();
            let mut paint = Paint::fill(Color::BLUE);
            paint.shader = Some(shader(kind, 0.0, extent, true));
            canvas.draw_rrect(
                flui_foundation::geometry::RRect::from_rect_circular(
                    Rect::from_xywh(0.0, 0.0, extent, extent),
                    radius,
                ),
                &paint,
            );
            tree.push_child(tree.root(), Layer::from(PictureLayer::new(canvas.finish())));
            capture(renderer, &tree)
        };
        let baked = rounded(false);
        let scaled = rounded(true);
        dump_named(&format!("layer-rounded-{kind}-baked"), &baked);
        dump_named(&format!("layer-rounded-{kind}-scaled"), &scaled);
        let corner = pixel(&baked, 0, 0);
        assert!(
            corner[0] < 240 && corner[0] > 5,
            "kind {kind}: physical rounded corner has partial AA coverage: {corner:?}"
        );
        expect_pixel(&scaled, (0, 0), corner);
        expect_pixel(&scaled, (5, 5), [0, 0, 255, 255]);

        let translated = |origin: f64| {
            let mut tree = LayerTree::new(Layer::from(TransformLayer::new(Matrix4::translation(
                -origin, 0.0, 0.0,
            ))));
            let mut canvas = Canvas::new();
            let mut paint = Paint::fill(Color::WHITE).with_anti_alias(false);
            paint.shader = Some(shader(kind, origin, 32.0, false));
            canvas.draw_rect(Rect::from_xywh(origin, 0.0, 32.0, 32.0), &paint);
            tree.push_child(tree.root(), Layer::from(PictureLayer::new(canvas.finish())));
            capture(renderer, &tree)
        };
        let baked = translated(0.0);
        let rebased = translated(1_000_000_000.0);
        dump_named(&format!("layer-origin-{kind}-baked"), &baked);
        dump_named(&format!("layer-origin-{kind}-rebased"), &rebased);
        assert_ne!(
            pixel(&baked, 8, 8),
            pixel(&baked, 20, 12),
            "kind {kind}: nonconstant large-origin shader witness"
        );
        for at in [(8, 8), (20, 12), (12, 24)] {
            expect_pixel(&rebased, at, pixel(&baked, at.0, at.1));
        }
        expect_pixel(&rebased, (40, 16), [255; 4]);
    }
}

fn nested_half_alpha_masks_each_apply_once(renderer: &HeadlessRenderer) {
    let bounds = Rect::from_xywh(8.0, 8.0, 32.0, 32.0);
    let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
    let mut parent = tree.root();
    for _ in 0..2 {
        parent = tree.push_child(
            parent,
            Layer::from(ShaderMaskLayer::new(
                Shader::solid(Color::rgba(255, 255, 255, 128)),
                BlendMode::DstIn,
                bounds,
            )),
        );
    }
    tree.push_child(parent, picture(bounds, Color::RED));
    let bytes = capture(renderer, &tree);
    dump_named("layer-nested-half-masks", &bytes);
    // Both independent masks attenuate the finished child once: (128/255)^2.
    // Skipping either one instead produces pink with green/blue near 127.
    expect_pixel(&bytes, (16, 16), [255, 191, 191, 255]);
    expect_pixel(&bytes, (4, 16), [255; 4]);
}
fn reused_capture_tracks_moving_and_removed_leader(renderer: &HeadlessRenderer) {
    let link = LayerLink::new();
    let build = |leader_x: Option<f64>| {
        let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
        if let Some(x) = leader_x {
            let branch = tree.push_child(
                tree.root(),
                Layer::from(OffsetLayer::new(Offset::new(x, 4.0))),
            );
            tree.push_child(
                branch,
                Layer::from(LeaderLayer::with_offset(
                    link,
                    Size::new(8.0, 8.0),
                    Offset::new(2.0, 2.0),
                )),
            );
        }
        let branch = tree.push_child(
            tree.root(),
            Layer::from(OffsetLayer::new(Offset::new(4.0, 40.0))),
        );
        let follower = tree.push_child(
            branch,
            Layer::from(
                FollowerLayer::new(link)
                    .with_size(Size::new(8.0, 8.0))
                    .with_show_when_unlinked(false),
            ),
        );
        tree.push_child(
            follower,
            picture(Rect::from_xywh(0.0, 0.0, 8.0, 8.0), Color::RED),
        );
        tree.push_child(
            tree.root(),
            picture(Rect::from_xywh(52.0, 52.0, 8.0, 8.0), Color::GREEN),
        );
        tree
    };
    // Same renderer cache and link identity, independent successive scenes.
    let first = capture(renderer, &build(Some(40.0)));
    dump_named("layer-leader-before", &first);
    expect_pixel(&first, (44, 8), [255, 0, 0, 255]);
    expect_pixel(&first, (16, 8), [255; 4]);
    let moved = capture(renderer, &build(Some(12.0)));
    dump_named("layer-leader-moved", &moved);
    expect_pixel(&moved, (44, 8), [255; 4]);
    expect_pixel(&moved, (16, 8), [255, 0, 0, 255]);
    let removed = capture(renderer, &build(None));
    dump_named("layer-leader-removed", &removed);
    expect_pixel(&removed, (44, 8), [255; 4]);
    expect_pixel(&removed, (16, 8), [255; 4]);
    expect_pixel(&removed, (6, 42), [255; 4]);
    expect_pixel(&removed, (55, 55), [0, 255, 0, 255]);
}

fn ordinary_and_advanced_gradient_numeric_refusals_recover(renderer: &HeadlessRenderer) {
    let colors = vec![Color::RED, Color::BLUE];
    let linear = |from, stops| Shader::LinearGradient {
        from,
        to: Offset::new(32.0, 32.0),
        colors: colors.clone(),
        stops,
        tile_mode: TileMode::Clamp,
    };
    let radial = |center, radius| Shader::RadialGradient {
        center,
        radius,
        colors: colors.clone(),
        stops: None,
        tile_mode: TileMode::Clamp,
        focal: None,
        focal_radius: None,
    };
    let sweep = |start_angle, end_angle| Shader::SweepGradient {
        center: Offset::new(16.0, 16.0),
        colors: colors.clone(),
        stops: None,
        tile_mode: TileMode::Clamp,
        start_angle,
        end_angle,
    };
    let invalid = [
        (
            "NaN linear endpoint",
            linear(Offset::new(f64::NAN, 0.0), None),
        ),
        (
            "unrepresentable radius",
            radial(Offset::new(16.0, 16.0), 1e100),
        ),
        ("negative radius", radial(Offset::new(16.0, 16.0), -1.0)),
        (
            "NaN radial centre",
            radial(Offset::new(16.0, f64::NAN), 16.0),
        ),
        ("NaN sweep angle", sweep(f64::NAN, std::f64::consts::TAU)),
        ("unrepresentable sweep span", sweep(-3e38, 3e38)),
        ("packed sweep span collapses", sweep(1e9, 1e9 + 1.0)),
        (
            "linear dot product overflows",
            Shader::LinearGradient {
                from: Offset::new(-1e30, 0.0),
                to: Offset::new(1e30, 0.0),
                colors: colors.clone(),
                stops: None,
                tile_mode: TileMode::Clamp,
            },
        ),
        (
            "radial squared distance overflows",
            radial(Offset::new(1e30, 0.0), 1e30),
        ),
        ("NaN stop", linear(Offset::ZERO, Some(vec![0.0, f64::NAN]))),
        (
            "decreasing effective stops",
            linear(Offset::ZERO, Some(vec![0.8, 0.2])),
        ),
    ];
    let valid = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::GREEN));
    let mut failed = Vec::new();
    for (name, shader) in invalid {
        for mode in [BlendMode::SrcOver, BlendMode::Multiply] {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut canvas = Canvas::new();
                let mut paint = Paint::fill(Color::WHITE)
                    .with_anti_alias(false)
                    .with_blend_mode(mode);
                paint.shader = Some(shader.clone());
                canvas.draw_rect(Rect::from_xywh(0.0, 0.0, 32.0, 32.0), &paint);
                let tree = LayerTree::new(Layer::from(PictureLayer::new(canvas.finish())));
                let result = renderer.render_layer_tree(&tree, (SIDE, SIDE));
                assert!(
                    matches!(result, Err(crate::EngineError::InvalidGeometry(_))),
                    "{name}, {mode:?}: invalid shader returns geometry error, got {:?}",
                    result.as_ref().map(Vec::len)
                );
                expect_pixel(&capture(renderer, &valid), (16, 16), [0, 255, 0, 255]);
            }));
            if outcome.is_err() {
                failed.push(format!("{name}, {mode:?}"));
            }
        }
    }
    assert!(
        failed.is_empty(),
        "numeric refusal/recovery rows: {failed:?}"
    );
}

// An acquired resize image and the painter viewport can momentarily differ.
// Use patterned attachment pixels, not a uniform clear, to detect accidental
// NDC stretching of a backdrop copied from nonzero attachment coordinates.
#[cfg(feature = "testing")]
fn backdrop_resize_attachment_coordinates(_renderer: &HeadlessRenderer) {
    use std::sync::Arc;
    let (device, queue) = crate::test_support::test_device_and_queue("backdrop resize attachment");
    for (extent, mode) in [
        (32_u32, BlendMode::Src),
        (96, BlendMode::Src),
        (32, BlendMode::Multiply),
        (96, BlendMode::Multiply),
        (32, BlendMode::Plus),
        (96, BlendMode::Plus),
    ] {
        let target = || {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("backdrop resize target"),
                size: wgpu::Extent3d {
                    width: extent,
                    height: extent,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let mut seed = Vec::new();
        for _vertical in 0..extent {
            for horizontal in 0..extent {
                seed.extend_from_slice(if (11..15).contains(&horizontal) {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                });
            }
        }
        let mut painter = crate::WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Rgba8Unorm,
            (64, 64),
        );
        let render =
            |painter: &mut crate::WgpuPainter, texture: &wgpu::Texture, bounds: Rect<f64>| {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &seed,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(extent * 4),
                        rows_per_image: Some(extent),
                    },
                    wgpu::Extent3d {
                        width: extent,
                        height: extent,
                        depth_or_array_layers: 1,
                    },
                );
                painter.begin_frame().expect("begin resized backdrop");
                painter
                    .record_backdrop_filter(
                        bounds,
                        &ImageFilter::Blur {
                            sigma_x: 2.0,
                            sigma_y: 0.0,
                        },
                        mode,
                    )
                    .expect("record resized backdrop");
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                painter
                    .render(
                        crate::render_target::RenderTarget::sampleable(&view, texture),
                        &mut encoder,
                    )
                    .expect("render acquired resize attachment");
                painter
                    .submit_encoder(encoder)
                    .expect("submit resized backdrop");
                painter.finish_frame();
                crate::test_support::readback_bytes(&device, &queue, texture, extent, extent)
            };
        let output = Rect::from_xywh(8.0, 8.0, 32.0, 32.0);
        let actual = render(&mut painter, &target(), output);
        let mut reference = crate::WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            wgpu::TextureFormat::Rgba8Unorm,
            (extent, extent),
        );
        let expected = render(&mut reference, &target(), output);
        let mismatch = actual.iter().zip(&expected).position(|(a, b)| a != b);
        assert!(
            mismatch.is_none(),
            "attachment {extent}, {mode:?}: first mismatched channel {mismatch:?}"
        );
        assert_ne!(actual, seed, "blur fixture must change stripe pixels");
        if extent == 32 {
            assert_eq!(
                render(
                    &mut painter,
                    &target(),
                    Rect::from_xywh(40.0, 8.0, 8.0, 8.0)
                ),
                seed,
                "outside smaller backing is a no-op"
            );
        }
        if extent == 96 && mode == BlendMode::Src {
            let edge = render(
                &mut painter,
                &target(),
                Rect::from_xywh(56.0, 8.0, 16.0, 16.0),
            );
            let at = |x: usize| (12 * extent as usize + x) * 4;
            assert_ne!(
                &edge[at(63)..at(63) + 4],
                &seed[at(63)..at(63) + 4],
                "viewport edge supplies transparent blur halo"
            );
            assert_eq!(
                &edge[at(64)..at(64) + 4],
                &seed[at(64)..at(64) + 4],
                "larger backing beyond viewport is untouched"
            );
        }
        painter.begin_frame().expect("begin view-only refusal");
        painter
            .record_backdrop_filter(
                output,
                &ImageFilter::Blur {
                    sigma_x: 2.0,
                    sigma_y: 0.0,
                },
                mode,
            )
            .expect("record refusal");
        let texture = target();
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        assert!(matches!(
            painter.render_to_view(&view, &mut encoder),
            Err(crate::EngineError::CompositeBackdropUnavailable)
        ));
        painter.finish_frame();
        assert_eq!(
            render(&mut painter, &target(), output),
            expected,
            "same painter recovers after missing backing"
        );
    }
}

#[test]
fn layer_effects_capture_as_specified() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    type Case = (&'static str, fn(&HeadlessRenderer));
    let rows: &[Case] = &[
        #[cfg(feature = "testing")]
        (
            "backdrop resize attachment coordinates",
            backdrop_resize_attachment_coordinates,
        ),
        ("fractional DPR clip and mask", fractional_dpr_clip_and_mask),
        (
            "partial mask equals public capture",
            partial_mask_matches_public_capture,
        ),
        ("nested mask failure recovery", nested_mask_failure_recovers),
        (
            "backdrop DPR and offset",
            backdrop_dpr_and_offset_preserve_pixels,
        ),
        (
            "competing effect errors and limits",
            competing_effect_errors_and_limits_recover,
        ),
        (
            "arbitrary affine terminal shader",
            arbitrary_affine_terminal_shader_matches_direct_paint,
        ),
        (
            "partial backdrop halo and later sibling",
            partial_backdrop_reconstructs_halo_before_later_sibling,
        ),
        (
            "gradient scale and large origin",
            gradient_scale_and_large_origin_preserve_device_pixels,
        ),
        (
            "nested half-alpha masks each once",
            nested_half_alpha_masks_each_apply_once,
        ),
        (
            "moving and removed leader across cached captures",
            reused_capture_tracks_moving_and_removed_leader,
        ),
        (
            "ordinary and advanced gradient numeric recovery",
            ordinary_and_advanced_gradient_numeric_refusals_recover,
        ),
        ("mask modes and tail", mask_modes_and_tail),
        ("followers linked and hidden", followers_resolve_and_hide),
        (
            "nested backdrop target and tail",
            nested_backdrop_reads_child_and_keeps_tail,
        ),
        ("anisotropic backdrop halo", anisotropic_backdrop_halo),
        (
            "rotated nonconstant mask shaders",
            rotated_nonconstant_mask_shaders,
        ),
    ];
    let failed: Vec<_> = rows
        .iter()
        .filter(|(_, row)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| row(&renderer))).is_err()
        })
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "failed layer effect rows: {failed:?}");
}
