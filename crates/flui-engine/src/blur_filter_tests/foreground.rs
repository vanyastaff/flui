//! Translation/crop oracle for foreground filter footprints.
//!
//! Both renders use the same device grid; the larger render merely translates
//! the scene by 32 integer pixels. One channel unit allows independent UNORM8
//! pass rounding. It cannot hide a missing halo, whose contrast is asserted
//! separately. Composition is additionally compared with independent nested
//! filters, so two crop renders cannot agree on the same truncated chain.

use std::sync::Arc;

use flui_foundation::geometry::{Matrix4, Offset, Point, RRect, Rect};
use flui_layer::{LayerTree, SceneBuilder};
use flui_painting::{
    Canvas, Paint,
    paint::{BlendMode, Clip, Image, ImageFilter, Path, Shader, TileMode},
    styling::Color,
};

use crate::{
    command_ir::{ImageFilterPass, ImageFilterSpec, MorphOp},
    painter::WgpuPainter,
};

#[derive(Clone, Copy)]
enum Filter {
    Blur(f32, f32),
    DoubleBlur,
    Dilate,
    Mixed,
    Erode,
    MorphRadius(f32),
    ChainSigma(f32),
    PublicSigma(f64),
}

impl Filter {
    fn public(self) -> ImageFilter {
        match self {
            Self::Blur(x, y) => ImageFilter::blur_directional(f64::from(x), f64::from(y)),
            Self::DoubleBlur => ImageFilter::Compose(vec![ImageFilter::blur(4.0); 2]),
            Self::Dilate => ImageFilter::dilate(4.0),
            Self::Erode => ImageFilter::erode(2.0),
            Self::MorphRadius(radius) => ImageFilter::dilate(f64::from(radius)),
            Self::PublicSigma(sigma) => ImageFilter::blur(sigma),
            Self::ChainSigma(sigma) => ImageFilter::Compose(vec![
                ImageFilter::blur(1.0),
                ImageFilter::blur(f64::from(sigma)),
            ]),
            Self::Mixed => ImageFilter::Compose(vec![
                ImageFilter::blur_directional(3.0, 2.0),
                ImageFilter::dilate(2.0),
                ImageFilter::erode(1.0),
                ImageFilter::blur_directional(2.0, 3.0),
            ]),
        }
    }

    fn direct(self) -> ImageFilterSpec {
        match self {
            Self::Blur(sigma_x, sigma_y) => ImageFilterSpec::Blur { sigma_x, sigma_y },
            Self::DoubleBlur => ImageFilterSpec::Chain(smallvec::smallvec![
                ImageFilterPass::Blur {
                    sigma_x: 4.0,
                    sigma_y: 4.0
                },
                ImageFilterPass::Blur {
                    sigma_x: 4.0,
                    sigma_y: 4.0
                },
            ]),
            Self::Dilate => ImageFilterSpec::Morph {
                radius: 4.0,
                op: MorphOp::Dilate,
            },
            Self::Erode => ImageFilterSpec::Morph {
                radius: 2.0,
                op: MorphOp::Erode,
            },
            Self::MorphRadius(radius) => ImageFilterSpec::Morph {
                radius,
                op: MorphOp::Dilate,
            },
            Self::PublicSigma(sigma) => ImageFilterSpec::Blur {
                sigma_x: sigma as f32,
                sigma_y: sigma as f32,
            },
            Self::ChainSigma(sigma) => ImageFilterSpec::Chain(smallvec::smallvec![
                ImageFilterPass::Blur {
                    sigma_x: 1.0,
                    sigma_y: 1.0
                },
                ImageFilterPass::Blur {
                    sigma_x: sigma,
                    sigma_y: sigma
                }
            ]),
            Self::Mixed => ImageFilterSpec::Chain(smallvec::smallvec![
                ImageFilterPass::Blur {
                    sigma_x: 3.0,
                    sigma_y: 2.0
                },
                ImageFilterPass::Morph {
                    radius: 2.0,
                    op: MorphOp::Dilate
                },
                ImageFilterPass::Morph {
                    radius: 1.0,
                    op: MorphOp::Erode
                },
                ImageFilterPass::Blur {
                    sigma_x: 2.0,
                    sigma_y: 3.0
                },
            ]),
        }
    }
}

#[derive(Clone, Copy)]
enum Primitive {
    Rect,
    Circle,
    Path,
    Image,
    Gradient,
    Glyph,
    Shadow,
    Arc,
    Rounded,
    RectGlyph,
    RadialGradient,
    SweepGradient,
    ExternalImage,
}

#[derive(Clone, Copy)]
enum Scope {
    Plain,
    OuterClipOpacity,
    NestedOpacity,
    InnerClip,
    NestedFilter,
    RoundedClip,
    PathClip,
}

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    rect: Rect<f64>,
    filter: Filter,
    scale: (f32, f32),
    scope: Scope,
    primitive: Primitive,
    visible: bool,
}

fn cases() -> Vec<Case> {
    let mut rows = Vec::new();
    for (name, x, y) in [
        ("left", -6.0, 10.0),
        ("right", 34.0, 10.0),
        ("top", 10.0, -6.0),
        ("bottom", 10.0, 34.0),
        ("top_left", -4.0, -4.0),
        ("top_right", 32.0, -4.0),
        ("bottom_left", -4.0, 32.0),
        ("bottom_right", 32.0, 32.0),
    ] {
        rows.push(Case {
            name,
            rect: Rect::from_xywh(x, y, 4.0, 4.0),
            filter: Filter::Blur(4.0, 4.0),
            scale: (1.0, 1.0),
            scope: Scope::Plain,
            primitive: Primitive::Rect,
            visible: true,
        });
    }
    let edge = Case {
        name: "fractional",
        rect: Rect::from_xywh(-5.25, 9.75, 4.0, 4.0),
        filter: Filter::Blur(4.0, 4.0),
        scale: (1.0, 1.0),
        scope: Scope::Plain,
        primitive: Primitive::Rect,
        visible: true,
    };
    rows.extend([
        edge,
        Case {
            name: "anisotropic",
            filter: Filter::Blur(4.0, 2.0),
            ..edge
        },
        Case {
            name: "scaled",
            scale: (2.0, 1.5),
            filter: Filter::Blur(4.0, 2.0),
            ..edge
        },
        Case {
            name: "outer_clip_opacity",
            scope: Scope::OuterClipOpacity,
            ..edge
        },
        Case {
            name: "inner_clip",
            scope: Scope::InnerClip,
            ..edge
        },
        Case {
            name: "nested_filter",
            scope: Scope::NestedFilter,
            ..edge
        },
        Case {
            name: "double_blur",
            filter: Filter::DoubleBlur,
            ..edge
        },
        Case {
            name: "dilate",
            filter: Filter::Dilate,
            ..edge
        },
        Case {
            name: "mixed_chain",
            filter: Filter::Mixed,
            ..edge
        },
        Case {
            name: "erode",
            rect: Rect::from_xywh(-4.0, 8.0, 12.0, 12.0),
            filter: Filter::Erode,
            ..edge
        },
        Case {
            name: "x_only",
            filter: Filter::Blur(4.0, 0.0),
            ..edge
        },
        Case {
            name: "y_only",
            rect: Rect::from_xywh(9.75, -5.25, 4.0, 4.0),
            filter: Filter::Blur(0.0, 4.0),
            ..edge
        },
        Case {
            name: "zero",
            rect: Rect::from_xywh(8.0, 8.0, 4.0, 4.0),
            filter: Filter::Blur(0.0, 0.0),
            ..edge
        },
        Case {
            name: "outside_radius",
            rect: Rect::from_xywh(-20.0, 10.0, 4.0, 4.0),
            visible: false,
            ..edge
        },
        Case {
            name: "circle",
            primitive: Primitive::Circle,
            ..edge
        },
        Case {
            name: "path",
            primitive: Primitive::Path,
            ..edge
        },
        Case {
            name: "image",
            primitive: Primitive::Image,
            ..edge
        },
        Case {
            name: "gradient",
            primitive: Primitive::Gradient,
            ..edge
        },
        // A viewport-sized allocation can handle arbitrarily large source bounds.
        Case {
            name: "huge_source",
            rect: Rect::from_xywh(-1e8, -1e8, 2e8, 2e8),
            ..edge
        },
    ]);
    rows.extend([
        Case {
            name: "glyph",
            rect: Rect::from_xywh(-7.0, 6.0, 4.0, 4.0),
            primitive: Primitive::Glyph,
            scope: Scope::OuterClipOpacity,
            ..edge
        },
        Case {
            name: "shadow",
            primitive: Primitive::Shadow,
            scope: Scope::OuterClipOpacity,
            ..edge
        },
        Case {
            name: "arc",
            primitive: Primitive::Arc,
            scope: Scope::OuterClipOpacity,
            ..edge
        },
        Case {
            name: "rounded",
            primitive: Primitive::Rounded,
            ..edge
        },
        Case {
            name: "rounded_clip",
            scope: Scope::RoundedClip,
            ..edge
        },
        Case {
            name: "path_clip",
            scope: Scope::PathClip,
            ..edge
        },
    ]);
    rows.push(Case {
        name: "distant_large_coordinates",
        rect: Rect::from_xywh(1e8, 1e8, 4.0, 4.0),
        visible: false,
        ..edge
    });
    rows.push(Case {
        name: "rect_and_overflowing_glyph",
        primitive: Primitive::RectGlyph,
        rect: Rect::from_xywh(-7.0, 6.0, 4.0, 4.0),
        ..edge
    });
    rows.push(Case {
        name: "tiny_circle_large_scale",
        primitive: Primitive::Circle,
        rect: Rect::from_xywh(-6e-8, 1e-7, 4e-8, 4e-8),
        scale: (1e8, 1e8),
        ..edge
    });
    rows.push(Case {
        name: "zero_morph",
        filter: Filter::MorphRadius(0.0),
        rect: Rect::from_xywh(8.0, 8.0, 4.0, 4.0),
        ..edge
    });
    for (name, primitive) in [
        ("radial_gradient", Primitive::RadialGradient),
        ("sweep_gradient", Primitive::SweepGradient),
        ("portable_gradient_plus", Primitive::Gradient),
        ("advanced_gradient_multiply", Primitive::Gradient),
        ("external_image", Primitive::ExternalImage),
    ] {
        rows.push(Case {
            name,
            primitive,
            scope: Scope::NestedOpacity,
            ..edge
        });
    }
    rows
}

fn source_path(rect: Rect<f64>) -> Path {
    let mut path = Path::new();
    path.move_to(Point::new(rect.left(), rect.top()));
    path.line_to(Point::new(rect.right(), rect.top()));
    path.line_to(Point::new(rect.right(), rect.bottom()));
    path.line_to(Point::new(rect.left(), rect.bottom()));
    path.close();
    path
}

fn source_image() -> Image {
    Image::from_rgba8(2, 2, [0, 0, 0, 255].repeat(4))
}

fn source_paint(c: Case) -> Paint {
    let shader = match c.primitive {
        Primitive::Gradient => Some(Shader::linear_gradient(
            Offset::new(c.rect.left(), c.rect.top()),
            Offset::new(c.rect.right(), c.rect.bottom()),
            vec![Color::BLACK, Color::rgb(80, 20, 0)],
            None,
            TileMode::Clamp,
        )),
        Primitive::RadialGradient => Some(Shader::radial_gradient(
            Offset::new(c.rect.center().x, c.rect.center().y),
            c.rect.width() * 0.5,
            vec![Color::BLACK, Color::rgb(80, 20, 0)],
            None,
            TileMode::Clamp,
            None,
            None,
        )),
        Primitive::SweepGradient => Some(Shader::sweep_gradient(
            Offset::new(c.rect.center().x, c.rect.center().y),
            vec![Color::BLACK, Color::rgb(80, 20, 0)],
            None,
            TileMode::Clamp,
            0.0,
            std::f64::consts::TAU,
        )),
        _ => None,
    };
    let paint = shader.map_or_else(
        || Paint::fill(Color::BLACK),
        |shader| Paint::fill(Color::BLACK).with_shader(shader),
    );
    match c.name {
        "portable_gradient_plus" => paint.with_blend_mode(BlendMode::Plus),
        "advanced_gradient_multiply" => paint.with_blend_mode(BlendMode::Multiply),
        _ => paint,
    }
}

fn source_paragraph() -> Arc<flui_painting::ShapedParagraph> {
    use flui_painting::{
        TextPainter,
        typography::{TextDirection, TextSpan, TextStyle},
    };
    let mut text = TextPainter::new()
        .with_text(TextSpan::new("M").with_style(TextStyle::new().with_font_size(12.0)))
        .with_text_direction(TextDirection::Ltr);
    text.layout(
        &mut flui_painting::TextContext::new(&flui_painting::FontCollection::new()),
        0.0,
        64.0,
    );
    let mut canvas = Canvas::new();
    text.paint(&mut canvas, Offset::ZERO);
    canvas
        .finish()
        .iter()
        .find_map(|command| match &command.op {
            flui_painting::DrawOp::Paragraph { paragraph, .. } => Some(Arc::clone(paragraph)),
            _ => None,
        })
        .expect("laid out text records a paragraph")
}
fn record_source(canvas: &mut Canvas, c: Case) {
    let paint = source_paint(c);
    match c.primitive {
        Primitive::Rect
        | Primitive::Gradient
        | Primitive::RadialGradient
        | Primitive::SweepGradient => canvas.draw_rect(c.rect, &paint),
        Primitive::Circle => canvas.draw_circle(c.rect.center(), c.rect.width() * 0.5, &paint),
        Primitive::Path => canvas.draw_path(&source_path(c.rect), &paint),
        Primitive::Image => canvas.draw_image(source_image(), c.rect, None),
        Primitive::ExternalImage => canvas.draw_texture(
            flui_painting::paint::TextureId::new(131),
            c.rect,
            None,
            flui_painting::paint::FilterQuality::None,
            1.0,
        ),
        Primitive::Glyph => canvas.draw_paragraph(
            &source_paragraph(),
            Offset::new(c.rect.left(), c.rect.top()),
            Color::BLACK,
        ),
        Primitive::RectGlyph => {
            canvas.draw_rect(Rect::from_xywh(20.0, 10.0, 4.0, 4.0), &paint);
            canvas.draw_paragraph(
                &source_paragraph(),
                Offset::new(c.rect.left(), c.rect.top()),
                Color::BLACK,
            );
        }
        Primitive::Shadow => canvas.draw_shadow(&Path::rectangle(c.rect), Color::BLACK, 2.0),
        Primitive::Arc => canvas.draw_arc(c.rect, 0.0, std::f64::consts::TAU, true, &paint),
        Primitive::Rounded => canvas.draw_rrect(RRect::from_rect_circular(c.rect, 1.0), &paint),
    }
}

fn draw_source(painter: &mut WgpuPainter, c: Case) {
    let paint = source_paint(c);
    match c.primitive {
        Primitive::Rect
        | Primitive::Gradient
        | Primitive::RadialGradient
        | Primitive::SweepGradient => painter.draw_rect(c.rect, &paint),
        Primitive::Circle => {
            painter.draw_circle(c.rect.center(), (c.rect.width() * 0.5) as f32, &paint);
        }
        Primitive::Path => painter.draw_path(&source_path(c.rect), &paint),
        Primitive::Image => painter.draw_image(&source_image(), c.rect, BlendMode::SrcOver),
        Primitive::ExternalImage => {
            let id = flui_painting::paint::TextureId::new(131);
            if painter.external_texture_registry().get(id).is_none() {
                let texture = painter.device().create_texture(&wgpu::TextureDescriptor {
                    label: Some("foreground external source"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                painter.queue().write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &[0, 0, 0, 255],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4),
                        rows_per_image: Some(1),
                    },
                    wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                painter
                    .external_texture_registry_mut()
                    .register(
                        id,
                        texture,
                        crate::ExternalTextureDescriptor {
                            sampling: crate::ExternalSampling::Nearest,
                            alpha: crate::ExternalAlpha::Straight,
                            color: crate::ExternalColorEncoding::EncodedSrgb,
                        },
                    )
                    .expect("valid external source admitted");
            }
            painter.draw_texture(
                id,
                c.rect,
                None,
                flui_painting::paint::FilterQuality::None,
                1.0,
            );
        }
        Primitive::Glyph => painter.draw_paragraph(
            source_paragraph(),
            Point::new(c.rect.left(), c.rect.top()),
            Color::BLACK,
        ),
        Primitive::RectGlyph => {
            painter.draw_rect(Rect::from_xywh(20.0, 10.0, 4.0, 4.0), &paint);
            painter.draw_paragraph(
                source_paragraph(),
                Point::new(c.rect.left(), c.rect.top()),
                Color::BLACK,
            );
        }
        Primitive::Shadow => painter.draw_shadow(&Path::rectangle(c.rect), Color::BLACK, 2.0),
        Primitive::Arc => painter.draw_arc(c.rect, 0.0, std::f32::consts::TAU, true, &paint),
        Primitive::Rounded => painter.draw_rrect(RRect::from_rect_circular(c.rect, 1.0), &paint),
    }
}

fn scene(c: Case, pad: f64, filtered: bool) -> LayerTree {
    let mut builder = SceneBuilder::new();
    builder.push_offset(Offset::new(pad, pad));
    builder.push_transform(Matrix4::scaling(
        f64::from(c.scale.0),
        f64::from(c.scale.1),
        1.0,
    ));
    if matches!(c.scope, Scope::OuterClipOpacity) {
        builder.push_clip_rect(Rect::from_xywh(-8.0, 0.0, 48.0, 32.0), Clip::HardEdge);
    }
    if filtered {
        builder.push_image_filter(c.filter.public());
    }
    if matches!(c.scope, Scope::RoundedClip) {
        builder.push_clip_rrect(
            RRect::from_rect_circular(Rect::from_xywh(-8.0, 0.0, 16.0, 32.0), 3.0),
            Clip::AntiAlias,
        );
    }
    if matches!(c.scope, Scope::PathClip) {
        builder.push_clip_path(
            source_path(Rect::from_xywh(-8.0, 0.0, 16.0, 32.0)),
            Clip::HardEdge,
        );
    }
    if matches!(c.scope, Scope::InnerClip) {
        builder.push_clip_rect(Rect::from_xywh(-8.0, 0.0, 16.0, 32.0), Clip::HardEdge);
    }
    match c.scope {
        Scope::OuterClipOpacity | Scope::NestedOpacity => {
            builder.push_opacity(0.5);
        }
        Scope::NestedFilter => {
            builder.push_image_filter(ImageFilter::blur(1.0));
        }
        Scope::Plain | Scope::InnerClip | Scope::RoundedClip | Scope::PathClip => {}
    }
    let mut canvas = Canvas::new();
    record_source(&mut canvas, c);
    builder.add_picture(canvas.finish());
    builder.build()
}

fn render_direct(
    painter: &mut WgpuPainter,
    c: Case,
    pad: f64,
    size: u32,
    filtered: bool,
) -> Vec<u8> {
    painter.resize(size, size);
    painter.begin_frame().expect("crop frame begins");
    painter.translate(Offset::new(pad, pad));
    painter.scale(c.scale.0, c.scale.1);
    if matches!(c.scope, Scope::OuterClipOpacity) {
        painter.save();
        painter.clip_rect(Rect::from_xywh(-8.0, 0.0, 48.0, 32.0), Clip::HardEdge);
    }
    if filtered {
        painter.save_layer_with_image_filter(c.filter.direct());
    }
    if matches!(c.scope, Scope::RoundedClip | Scope::PathClip) {
        painter.save();
        if matches!(c.scope, Scope::RoundedClip) {
            painter.clip_rrect(
                RRect::from_rect_circular(Rect::from_xywh(-8.0, 0.0, 16.0, 32.0), 3.0),
                Clip::AntiAlias,
            );
        } else {
            painter.clip_path(&source_path(Rect::from_xywh(-8.0, 0.0, 16.0, 32.0)));
        }
    }
    if matches!(c.scope, Scope::InnerClip) {
        painter.save();
        painter.clip_rect(Rect::from_xywh(-8.0, 0.0, 16.0, 32.0), Clip::HardEdge);
    }
    match c.scope {
        Scope::OuterClipOpacity | Scope::NestedOpacity => {
            painter.save_layer(None, &Paint::fill(Color::WHITE).with_opacity(0.5));
        }
        Scope::NestedFilter => painter.save_layer_with_image_filter(ImageFilterSpec::Blur {
            sigma_x: 1.0,
            sigma_y: 1.0,
        }),
        Scope::Plain | Scope::InnerClip | Scope::RoundedClip | Scope::PathClip => {}
    }
    draw_source(painter, c);
    if matches!(
        c.scope,
        Scope::OuterClipOpacity | Scope::NestedOpacity | Scope::NestedFilter
    ) {
        painter.restore_layer();
    }
    if matches!(c.scope, Scope::InnerClip) {
        painter.restore();
    }
    if matches!(c.scope, Scope::RoundedClip | Scope::PathClip) {
        painter.restore();
    }
    if filtered {
        painter.restore_layer();
    }
    if matches!(c.scope, Scope::OuterClipOpacity) {
        painter.restore();
    }
    let (texture, view) = crate::test_support::create_sampleable_target(
        painter.device(),
        "foreground crop target",
        size,
        size,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    crate::test_support::clear_target(painter.device(), painter.queue(), &view, wgpu::Color::WHITE);
    let mut encoder = painter
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .render_to_texture(&texture, &mut encoder)
        .expect("foreground crop encodes");
    painter
        .submit_encoder(encoder)
        .expect("foreground crop submits");
    let pixels = crate::test_support::readback_bytes(
        painter.device(),
        painter.queue(),
        &texture,
        size,
        size,
    );
    painter.finish_frame();
    pixels
}

fn assert_pixels_close(name: &str, a: &[u8], b: &[u8]) {
    assert_eq!(a.len(), b.len(), "{name}: image dimensions");
    let max = a
        .iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .expect("image is nonempty");
    assert!(
        max <= 1,
        "{name}: maximum channel difference {max} exceeds one UNORM8 unit"
    );
}

fn crop(large: &[u8]) -> Vec<u8> {
    (32..64)
        .flat_map(|y| large[(y * 96 + 32) * 4..(y * 96 + 64) * 4].iter().copied())
        .collect()
}

fn render_recorded(renderer: &crate::HeadlessRenderer, c: Case, pad: f64, size: u32) -> Vec<u8> {
    let tree = scene(c, pad, true);
    if matches!(c.primitive, Primitive::ExternalImage) {
        let mut capture = renderer
            .retained_capture((size, size))
            .expect("external crop capture");
        capture.set_solid_texture(flui_painting::paint::TextureId::new(131), [0, 0, 0, 255]);
        capture
            .render_unmanaged(&flui_layer::Scene::new(tree))
            .expect("recorded external source rasterizes");
        capture.read_rgba().expect("recorded external crop pixels")
    } else {
        renderer
            .render_layer_tree(&tree, (size, size))
            .expect("recorded crop scene")
    }
}

#[test]
fn foreground_filter_viewport_crop_contract() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (device, queue) = crate::test_support::test_device_and_queue("Foreground Crop Device");
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
    );
    let mut failures = Vec::new();
    for c in cases() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let small = render_direct(&mut painter, c, 0.0, 32, true);
            let large = render_direct(&mut painter, c, 32.0, 96, true);
            let replay_small = render_recorded(&renderer, c, 0.0, 32);
            let replay_large = render_recorded(&renderer, c, 32.0, 96);
            assert_pixels_close(c.name, &small, &crop(&large));
            assert_pixels_close(c.name, &replay_small, &crop(&replay_large));
            assert_pixels_close(c.name, &small, &replay_small);
            assert_pixels_close(c.name, &large, &replay_large);
            if c.visible {
                assert!(
                    small.as_chunks::<4>().0.iter().any(|p| p[0] < 254),
                    "{}: expected source contribution disappeared",
                    c.name
                );
            } else {
                assert!(
                    small.iter().all(|v| *v == 255),
                    "outside-radius source appeared in the viewport"
                );
                assert!(
                    replay_small.iter().all(|v| *v == 255),
                    "recorded outside-radius source appeared"
                );
            }
            if matches!(c.primitive, Primitive::RectGlyph) {
                for pixels in [&small, &replay_small] {
                    assert!(
                        (0..32).any(|y| (0..8).any(|x| pixels[(y * 32 + x) * 4] < 254)),
                        "mixed source must retain glyph contribution apart from the distant rect"
                    );
                }
            }
            if c.name == "tiny_circle_large_scale" {
                for pixels in [&small, &replay_small] {
                    assert!(
                        pixels[(24 * 32 + 24) * 4] < 254,
                        "source support must include the shader's clamped circle radius"
                    );
                }
            }
            if matches!(c.name, "zero" | "zero_morph") {
                let unfiltered = render_direct(&mut painter, c, 0.0, 32, false);
                let replay_unfiltered = renderer
                    .render_layer_tree(&scene(c, 0.0, false), (32, 32))
                    .expect("unfiltered recorded scene");
                assert_eq!(small, unfiltered, "zero sigma direct identity");
                assert_eq!(
                    replay_small, replay_unfiltered,
                    "zero sigma recorded identity"
                );
                assert_eq!(
                    &small[(9 * 32 + 9) * 4..(9 * 32 + 9) * 4 + 4],
                    &[0, 0, 0, 255]
                );
            }
        }));
        if result.is_err() {
            failures.push(c.name);
        }
    }
    assert!(
        failures.is_empty(),
        "foreground filter crop rows failed: {failures:?}"
    );
}

fn independent_chain_scene(filters: &[ImageFilter], nested: bool) -> LayerTree {
    let mut builder = SceneBuilder::new();
    if nested {
        for filter in filters.iter().rev() {
            builder.push_image_filter(filter.clone());
        }
    } else {
        builder.push_image_filter(ImageFilter::Compose(filters.to_vec()));
    }
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_xywh(40.0, 32.0, 2.0, 24.0),
        &Paint::fill(Color::BLACK),
    );
    builder.add_picture(canvas.finish());
    builder.build()
}

#[test]
fn foreground_filter_chains_match_independent_nested_layers() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let rows = [
        (
            "two blurs",
            vec![ImageFilter::blur(2.0), ImageFilter::blur(2.0)],
        ),
        (
            "different axis blurs",
            vec![
                ImageFilter::blur_directional(3.0, 1.0),
                ImageFilter::blur_directional(1.0, 3.0),
            ],
        ),
        (
            "blur then dilate",
            vec![ImageFilter::blur(2.0), ImageFilter::dilate(2.0)],
        ),
        (
            "dilate then blur",
            vec![ImageFilter::dilate(2.0), ImageFilter::blur(2.0)],
        ),
        (
            "erode after blur",
            vec![ImageFilter::blur(3.0), ImageFilter::erode(1.0)],
        ),
    ];
    let mut failures = Vec::new();
    for (name, filters) in rows {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let composed = renderer
                .render_layer_tree(&independent_chain_scene(&filters, false), (96, 96))
                .expect("composed scene");
            let nested = renderer
                .render_layer_tree(&independent_chain_scene(&filters, true), (96, 96))
                .expect("independent nested scene");
            assert_pixels_close(name, &composed, &nested);
            assert!(
                nested.as_chunks::<4>().0.iter().any(|p| p[0] < 240),
                "{name}: both routes lost source"
            );
        }));
        if result.is_err() {
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "independent chain rows failed: {failures:?}"
    );
}

#[test]
fn foreground_filter_invalid_parameters_refuse_and_recover() {
    let Some(renderer) = crate::test_support::renderer_or_skip() else {
        return;
    };
    let (device, queue) = crate::test_support::test_device_and_queue("Foreground Admission Device");
    let mut painter = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
    );
    let mut failures = Vec::new();
    let invalid = [
        ("negative sigma x", Filter::Blur(-1.0, 2.0)),
        ("negative sigma y", Filter::Blur(2.0, -1.0)),
        ("nan sigma x", Filter::Blur(f32::NAN, 2.0)),
        ("nan sigma y", Filter::Blur(2.0, f32::NAN)),
        ("infinite sigma x", Filter::Blur(f32::INFINITY, 2.0)),
        ("infinite sigma y", Filter::Blur(2.0, f32::INFINITY)),
        (
            "negative infinite sigma",
            Filter::Blur(f32::NEG_INFINITY, 0.0),
        ),
        (
            "underflowing squared sigma",
            Filter::Blur(f32::MIN_POSITIVE, 0.0),
        ),
        ("negative morph radius", Filter::MorphRadius(-1.0)),
        ("nan morph radius", Filter::MorphRadius(f32::NAN)),
        ("infinite morph radius", Filter::MorphRadius(f32::INFINITY)),
        ("invalid chain pass", Filter::ChainSigma(f32::NAN)),
        (
            "unrepresentable filter footprint",
            Filter::Blur(f32::MAX, 0.0),
        ),
        (
            "public sigma narrowing underflow",
            Filter::PublicSigma(1e-100),
        ),
    ];
    for (name, filter) in invalid {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let c = Case {
                name,
                rect: Rect::from_xywh(8.0, 8.0, 8.0, 8.0),
                filter,
                scale: (1.0, 1.0),
                scope: Scope::Plain,
                primitive: Primitive::Rect,
                visible: true,
            };
            let recorded_error = renderer
                .render_layer_tree(&scene(c, 0.0, true), (32, 32))
                .expect_err("invalid filter must be refused through replay");
            assert!(
                matches!(recorded_error, crate::EngineError::InvalidGeometry(_)),
                "{name}: unexpected replay error {recorded_error}"
            );
            if !matches!(filter, Filter::PublicSigma(_)) {
                painter.begin_frame().expect("invalid direct frame begins");
                painter.save_layer_with_image_filter(filter.direct());
                draw_source(&mut painter, c);
                painter.restore_layer();
                let (texture, _) = crate::test_support::create_sampleable_target(
                    &device,
                    "invalid filter target",
                    32,
                    32,
                    wgpu::TextureFormat::Rgba8Unorm,
                );
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                let error = painter
                    .render_to_texture(&texture, &mut encoder)
                    .expect_err("invalid direct filter must be refused");
                painter.finish_frame();
                assert!(
                    matches!(error, crate::EngineError::InvalidGeometry(_)),
                    "{name}: unexpected direct error {error}"
                );
            }
            let healthy = Case {
                name: "recovered",
                filter: Filter::Blur(0.0, 0.0),
                ..c
            };
            let direct = render_direct(&mut painter, healthy, 0.0, 32, true);
            let replay = renderer
                .render_layer_tree(&scene(healthy, 0.0, true), (32, 32))
                .expect("recorded frame recovers after invalid filter");
            assert_eq!(direct, replay, "{name}: recovery routes differ");
            assert_eq!(
                &direct[(10 * 32 + 10) * 4..(10 * 32 + 10) * 4 + 4],
                &[0, 0, 0, 255],
                "{name}: healthy next frame did not draw"
            );
        }));
        if result.is_err() {
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "filter admission rows failed: {failures:?}"
    );
}

#[test]
fn foreground_filter_prepared_quota_refusal_keeps_next_frame_deliverable() {
    use crate::device_domain::{DeviceDomain, PreparedCost, PreparedIrLimits};
    let (device, queue) = crate::test_support::test_device_and_queue("Foreground Quota Device");
    let domain = DeviceDomain::with_limits(
        Arc::clone(&device),
        Arc::clone(&queue),
        PreparedIrLimits {
            cost: PreparedCost {
                gpu_bytes: 256 * 1024,
                ..PreparedIrLimits::default().cost
            },
            ..Default::default()
        },
    );
    let mut painter = WgpuPainter::with_domain(domain, wgpu::TextureFormat::Rgba8Unorm, (32, 32));
    let mut failures = Vec::new();
    for (name, sigma, scope) in [
        ("before attachment switch", 100.0, Scope::Plain),
        (
            "nested group after attachment switch",
            30.0,
            Scope::NestedOpacity,
        ),
    ] {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let c = Case {
                name,
                rect: Rect::from_xywh(-1000.0, -1000.0, 2032.0, 2032.0),
                filter: Filter::Blur(sigma, sigma),
                scale: (1.0, 1.0),
                scope,
                primitive: Primitive::Rect,
                visible: true,
            };
            painter.begin_frame().expect("quota frame begins");
            painter.save_layer_with_image_filter(c.filter.direct());
            if matches!(scope, Scope::NestedOpacity) {
                painter.save_layer(None, &Paint::fill(Color::WHITE).with_opacity(0.5));
            }
            draw_source(&mut painter, c);
            if matches!(scope, Scope::NestedOpacity) {
                painter.restore_layer();
            }
            painter.restore_layer();
            let (texture, _) = crate::test_support::create_sampleable_target(
                &device,
                "quota filter target",
                32,
                32,
                wgpu::TextureFormat::Rgba8Unorm,
            );
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let result = painter.render_to_texture(&texture, &mut encoder);
            painter.finish_frame();
            assert!(
                matches!(
                    result,
                    Err(crate::EngineError::PreparedResourceLimit {
                        resource: "GPU payload bytes",
                        ..
                    })
                ),
                "{name}: expanded filter resources must be admitted before allocation: {result:?}"
            );
            let healthy = Case {
                name: "after quota refusal",
                rect: Rect::from_xywh(8.0, 8.0, 8.0, 8.0),
                filter: Filter::Blur(0.0, 0.0),
                scope: Scope::Plain,
                ..c
            };
            let pixels = render_direct(&mut painter, healthy, 0.0, 32, true);
            let reference = render_direct(&mut painter, healthy, 0.0, 32, false);
            assert_eq!(
                pixels, reference,
                "{name}: recovery changed the next frame's attachment or origin"
            );
            assert_eq!(
                &pixels[(10 * 32 + 10) * 4..(10 * 32 + 10) * 4 + 4],
                &[0, 0, 0, 255],
                "{name}: next frame must draw after the filter quota refusal"
            );
        }));
        if outcome.is_err() {
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "filter quota refusal rows failed: {failures:?}"
    );
    let mut normal = WgpuPainter::with_shared_device(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
    );
    let costly = Case {
        name: "cumulative sampling work",
        rect: Rect::from_xywh(-1000.0, -1000.0, 2032.0, 2032.0),
        filter: Filter::Blur(1000.0, 1000.0),
        scale: (1.0, 1.0),
        scope: Scope::Plain,
        primitive: Primitive::Rect,
        visible: true,
    };
    normal.begin_frame().expect("sampling work frame begins");
    normal.save_layer_with_image_filter(costly.filter.direct());
    draw_source(&mut normal, costly);
    normal.restore_layer();
    let (texture, _) = crate::test_support::create_sampleable_target(
        &device,
        "sampling work target",
        32,
        32,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let result = normal.render_to_texture(&texture, &mut encoder);
    normal.finish_frame();
    assert!(
        matches!(
            result,
            Err(crate::EngineError::PreparedResourceLimit {
                resource: "cumulative effect sampling work",
                ..
            })
        ),
        "large valid filter must refuse sampling work before allocation: {result:?}"
    );
    let healthy = Case {
        name: "after sampling work refusal",
        rect: Rect::from_xywh(8.0, 8.0, 8.0, 8.0),
        filter: Filter::Blur(0.0, 0.0),
        ..costly
    };
    let pixels = render_direct(&mut normal, healthy, 0.0, 32, true);
    let reference = render_direct(&mut normal, healthy, 0.0, 32, false);
    assert_eq!(
        pixels, reference,
        "sampling work refusal must restore attachment and origin"
    );
    assert_eq!(
        &pixels[(10 * 32 + 10) * 4..(10 * 32 + 10) * 4 + 4],
        &[0, 0, 0, 255],
        "sampling work refusal must leave next frame deliverable"
    );
}
