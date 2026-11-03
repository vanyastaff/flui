//! A transform backdrop filter shows the backdrop magnified: the stripes it
//! covers come out wider, the ones beside it stay as they were.
use super::*;
use flui_foundation::geometry::Point;
use flui_layer::ImageFilterLayer;
use flui_painting::paint::{ImageSampling, ImageTransform};

/// Vertical stripes 4 px wide, black from x = 0, white from x = 4, and so on.
fn stripes() -> Layer {
    let mut canvas = Canvas::new();
    for stripe in 0..SIDE / 4 {
        let color = if stripe % 2 == 0 {
            Color::BLACK
        } else {
            Color::WHITE
        };
        canvas.draw_rect(
            Rect::from_xywh(f64::from(stripe * 4), 0.0, 4.0, f64::from(SIDE)),
            &Paint::fill(color).with_anti_alias(false),
        );
    }
    Layer::from(PictureLayer::new(canvas.finish()))
}

/// The stripes under `filter` as a backdrop filter over 16..48 on both
/// axes, under `matrix`.
fn scene(matrix: Matrix4, filter: ImageFilter) -> LayerTree {
    blended(matrix, filter, BlendMode::Src)
}

/// [`scene`], composited with `blend`.
fn blended(matrix: Matrix4, filter: ImageFilter, blend: BlendMode) -> LayerTree {
    let mut tree = LayerTree::new(Layer::from(TransformLayer::new(matrix)));
    tree.push_child(tree.root(), stripes());
    tree.push_child(
        tree.root(),
        Layer::from(BackdropFilterLayer::new(
            filter,
            blend,
            Rect::from_xywh(16.0, 16.0, 32.0, 32.0),
        )),
    );
    tree
}

const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

pub(super) fn magnified_backdrop_doubles_the_stripes_it_covers(renderer: &HeadlessRenderer) {
    let magnify = ImageFilter::try_magnify(Point::new(32.0, 32.0), 2.0).expect("a valid scale");
    let plain = capture(renderer, &scene(Matrix4::IDENTITY, ImageFilter::blur(0.0)));
    let magnified = capture(renderer, &scene(Matrix4::IDENTITY, magnify.clone()));
    dump_named("backdrop-transform-magnified", &magnified);
    // Inside the loupe pixel x shows the backdrop at (x + 32) / 2, so the
    // stripe 24..28 fills 16..24 and the stripe 28..32 fills 24..32.
    for (x, expected) in [(18, BLACK), (22, BLACK), (25, WHITE), (30, WHITE)] {
        expect_pixel(&magnified, (x, 32), expected);
    }
    // The same pixels unmagnified: 22 is white and 25 black, so the
    // witnesses above tell magnified from plain.
    expect_pixel(&plain, (22, 32), WHITE);
    expect_pixel(&plain, (25, 32), BLACK);
    // Beside the loupe the stripes stay.
    for (x, expected) in [(10, BLACK), (13, WHITE), (54, WHITE), (57, BLACK)] {
        expect_pixel(&magnified, (x, 32), expected);
    }

    // Between texels the loupe blends them. Pixel 23 shows (23.5 + 32) / 2 =
    // 27.75, a quarter of the way from the black texel 27 to the white 28:
    // about 64, where a nearest read gives 0. Pixel 16, at the loupe's edge,
    // shows 24.25, a quarter of the white texel 23 past the shown rect: about
    // 64 too, where a copy without that texel clamps to black.
    let grey = |bytes: &[u8], x: u32| {
        let [red, ..] = pixel(bytes, x, 32);
        assert!(
            (40..=90).contains(&red),
            "pixel {x} reads {red}, not a blend"
        );
    };
    grey(&magnified, 23);
    grey(&magnified, 16);
    // A blend that reads the destination samples the same way: over the
    // white stripe 20..24, multiply keeps the blended grey.
    let multiplied = capture(
        renderer,
        &blended(Matrix4::IDENTITY, magnify.clone(), BlendMode::Multiply),
    );
    grey(&multiplied, 23);
    // Read nearest, the same pixels take the texel they fall in: 27.75 the
    // black texel 27 and 24.25 the black texel 24, through either composite
    // path.
    let nearest = ImageFilter::Transform {
        transform: ImageTransform::try_scale_about(Point::new(32.0, 32.0), 2.0, 2.0)
            .expect("a valid scale"),
        sampling: ImageSampling::Nearest,
    };
    let sharp = capture(renderer, &scene(Matrix4::IDENTITY, nearest.clone()));
    dump_named("backdrop-transform-nearest", &sharp);
    for (x, expected) in [(16, BLACK), (23, BLACK), (24, WHITE), (31, WHITE)] {
        expect_pixel(&sharp, (x, 32), expected);
    }
    let sharp_multiplied = capture(
        renderer,
        &blended(Matrix4::IDENTITY, nearest, BlendMode::Multiply),
    );
    expect_pixel(&sharp_multiplied, (23, 32), BLACK);

    // Under a layer transform the magnifier is in the layer's space: moved
    // 8 px right, the loupe covers 24..56 and its focal point is at 40, so
    // device pixel 34 shows the layer's (34.5 - 8 + 32) / 2 = 29.25, in the
    // white stripe 28..32. Magnified about device 32 instead it would show
    // 33.25 - 8 = 25.25, and unmagnified 26.5: both black.
    let moved = capture(
        renderer,
        &scene(Matrix4::translation(8.0, 0.0, 0.0), magnify),
    );
    expect_pixel(&moved, (34, 32), WHITE);

    // A grouped pane whose transform reads only past the surface shows
    // nothing, yet still captures the group's backdrop before its child
    // paints: the next pane of the group shows the white under the green
    // child, not the child.
    let group = BackdropGroupKey::new();
    let away = ImageFilter::Transform {
        transform: ImageTransform::try_new(1.0, 0.0, 0.0, 1.0, 200.0, 0.0).expect("a translation"),
        sampling: ImageSampling::Linear,
    };
    let mut grouped = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::WHITE));
    let pane = grouped.push_child(
        grouped.root(),
        Layer::from(
            BackdropFilterLayer::new(
                away,
                BlendMode::SrcOver,
                Rect::from_xywh(8.0, 8.0, 24.0, 48.0),
            )
            .with_group(Some(group.clone())),
        ),
    );
    grouped.push_child(
        pane,
        picture(Rect::from_xywh(8.0, 8.0, 24.0, 48.0), Color::rgb(0, 255, 0)),
    );
    grouped.push_child(
        grouped.root(),
        Layer::from(
            BackdropFilterLayer::new(
                ImageFilter::blur(0.0),
                BlendMode::Src,
                Rect::from_xywh(24.0, 8.0, 32.0, 48.0),
            )
            .with_group(Some(group)),
        ),
    );
    expect_pixel(&capture(renderer, &grouped), (28, 32), WHITE);

    // Past the attachment the backdrop is transparent, and a tap there
    // blends with it: moved 16.25 px right, pixel 16 shows source 0.25, three
    // quarters of the white texel 0 and a quarter of the transparency left
    // of it, so about 191 alpha where repeating the edge texel gives 255.
    let moved_right = ImageFilter::Transform {
        transform: ImageTransform::try_new(1.0, 0.0, 0.0, 1.0, 16.25, 0.0).expect("a translation"),
        sampling: ImageSampling::Linear,
    };
    let mut edge = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::WHITE));
    edge.push_child(
        edge.root(),
        Layer::from(BackdropFilterLayer::new(
            moved_right,
            BlendMode::Src,
            Rect::from_xywh(16.0, 16.0, 32.0, 32.0),
        )),
    );
    let edged = capture(renderer, &edge);
    let [.., alpha] = pixel(&edged, 16, 32);
    assert!((170..=210).contains(&alpha), "pixel 16 has alpha {alpha}");
    expect_pixel(&edged, (24, 32), WHITE);
    // A sample just past the attachment still reads the edge texel: moved
    // 16.75 px right, pixel 16 shows source -0.25, a quarter of the white
    // texel 0, so about 64 alpha where cropping it away leaves 255.
    let past_edge = ImageFilter::Transform {
        transform: ImageTransform::try_new(1.0, 0.0, 0.0, 1.0, 16.75, 0.0).expect("a translation"),
        sampling: ImageSampling::Linear,
    };
    let mut fringe = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::WHITE));
    fringe.push_child(
        fringe.root(),
        Layer::from(BackdropFilterLayer::new(
            past_edge,
            BlendMode::Src,
            Rect::from_xywh(16.0, 16.0, 32.0, 32.0),
        )),
    );
    let [.., alpha] = pixel(&capture(renderer, &fringe), 16, 32);
    assert!((40..=90).contains(&alpha), "pixel 16 has alpha {alpha}");

    // On a target as wide as the device allows, the halo takes the copy past
    // the texture limit: the frame fails with a typed limit, not a wgpu
    // validation panic.
    let widest = renderer.max_texture_dimension();
    let mut wide = LayerTree::new(picture(
        Rect::from_xywh(0.0, 0.0, f64::from(widest), 4.0),
        Color::WHITE,
    ));
    wide.push_child(
        wide.root(),
        Layer::from(BackdropFilterLayer::new(
            ImageFilter::Transform {
                transform: ImageTransform::try_new(1.0, 0.0, 0.0, 1.0, 0.25, 0.0)
                    .expect("a translation"),
                sampling: ImageSampling::Linear,
            },
            BlendMode::Src,
            Rect::from_xywh(0.0, 0.0, f64::from(widest), 4.0),
        )),
    );
    assert!(matches!(
        renderer.render_layer_tree(&wide, (widest, 4)),
        Err(crate::EngineError::PreparedResourceLimit {
            resource: "backdrop copy dimension",
            ..
        })
    ));

    // A magnifier costs its output, not the texels it copies: a pane of
    // 4096 x 4096 magnified 1024 times copies a few texels but resamples
    // them into a 64 MiB texture over its output. Three such panes fit the
    // 256 MiB the frame may prepare; five take 320 MiB, past it.
    let side = 4096.0;
    let zoomed = |panes: usize, blend: BlendMode| {
        let mut tree = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, side, side), Color::WHITE));
        for _ in 0..panes {
            tree.push_child(
                tree.root(),
                Layer::from(BackdropFilterLayer::new(
                    ImageFilter::try_magnify(Point::new(side / 2.0, side / 2.0), 1024.0)
                        .expect("a valid scale"),
                    blend,
                    Rect::from_xywh(0.0, 0.0, side, side),
                )),
            );
        }
        renderer.render_layer_tree(&tree, (4096, 4096))
    };
    let fits = zoomed(3, BlendMode::SrcOver);
    assert!(fits.is_ok(), "{:?}", fits.map(|bytes| bytes.len()));
    let outcome = zoomed(5, BlendMode::SrcOver);
    assert!(
        matches!(
            outcome,
            Err(crate::EngineError::PreparedResourceLimit {
                resource: "GPU payload bytes",
                ..
            })
        ),
        "{:?}",
        outcome.map(|bytes| bytes.len())
    );
    // Under a blend that reads the destination, each pane also copies the
    // destination under its output: three panes then take 384 MiB.
    let outcome = zoomed(3, BlendMode::Multiply);
    assert!(
        matches!(
            outcome,
            Err(crate::EngineError::PreparedResourceLimit {
                resource: "GPU payload bytes",
                ..
            })
        ),
        "{:?}",
        outcome.map(|bytes| bytes.len())
    );
}

/// A transform turning by `angle` about the middle of the pane.
fn turn(angle: f64, sampling: ImageSampling) -> ImageFilter {
    let turned = Matrix4::translation(32.0, 32.0, 0.0)
        * Matrix4::rotation_z(angle)
        * Matrix4::translation(-32.0, -32.0, 0.0);
    ImageFilter::Transform {
        transform: ImageTransform::try_new(
            turned.m[0],
            turned.m[1],
            turned.m[4],
            turned.m[5],
            turned.m[12],
            turned.m[13],
        )
        .expect("a rotation is invertible"),
        sampling,
    }
}

pub(super) fn turned_backdrop_turns_the_stripes_it_covers(renderer: &HeadlessRenderer) {
    // Turned a quarter about 32, pixel (x, y) of the pane shows the backdrop
    // at (y, 64 - x): the vertical stripes lie across it. Down the column
    // x = 32 they alternate every 4 px; along the row y = 32 every pixel
    // shows x = 32.5, in the black stripe 32..36.
    for sampling in [ImageSampling::Nearest, ImageSampling::Linear] {
        let quarter = capture(
            renderer,
            &scene(
                Matrix4::IDENTITY,
                turn(std::f64::consts::FRAC_PI_2, sampling),
            ),
        );
        dump_named("backdrop-transform-quarter", &quarter);
        for (y, expected) in [(18, BLACK), (21, WHITE), (25, BLACK), (29, WHITE)] {
            expect_pixel(&quarter, (32, y), expected);
        }
        // Unturned, 22 and 29 are white: the row is black only turned.
        for x in [18, 22, 25, 29, 40, 45] {
            expect_pixel(&quarter, (x, 32), BLACK);
        }
        // Beside the pane the stripes stay.
        expect_pixel(&quarter, (13, 32), WHITE);
        expect_pixel(&quarter, (54, 32), WHITE);
    }

    // At an angle no axis survives: each pixel of the pane shows the stripe
    // under its centre turned back by 0.3. Where the four texels a bilinear
    // read blends lie in one stripe, the pixel takes that stripe's colour.
    let angle: f64 = 0.3;
    let turned = capture(
        renderer,
        &scene(Matrix4::IDENTITY, turn(angle, ImageSampling::Linear)),
    );
    dump_named("backdrop-transform-turned", &turned);
    let plain = capture(renderer, &scene(Matrix4::IDENTITY, ImageFilter::blur(0.0)));
    let (mut checked, mut moved) = (0, 0);
    for y in 17..47 {
        for x in 17..47 {
            let (px, py) = (f64::from(x) + 0.5 - 32.0, f64::from(y) + 0.5 - 32.0);
            // The inverse turn sends the centre back by `angle`.
            let source = 32.0 + px * angle.cos() + py * angle.sin();
            let edge = (source / 4.0).round() * 4.0;
            if (source - edge).abs() < 1.0 {
                continue;
            }
            let expected = if (source / 4.0).floor() as i64 % 2 == 0 {
                BLACK
            } else {
                WHITE
            };
            expect_pixel(&turned, (x, y), expected);
            checked += 1;
            if pixel(&plain, x, y) != expected {
                moved += 1;
            }
        }
    }
    assert!(
        checked > 400 && moved > 100,
        "{checked} pixels checked, {moved} differ from the unturned backdrop"
    );
}

/// `content` under a layer filter `filter`, over white.
fn filtered(filter: ImageFilter, content: Layer) -> LayerTree {
    let mut tree = LayerTree::new(picture(Rect::from_xywh(0.0, 0.0, 64.0, 64.0), Color::WHITE));
    let layer = tree.push_child(tree.root(), Layer::from(ImageFilterLayer::new(filter)));
    tree.push_child(layer, content);
    tree
}

pub(super) fn layer_transform_moves_and_turns_its_content(renderer: &HeadlessRenderer) {
    // Magnified 2x about 32, pixel x of the layer shows its content at
    // (x + 32) / 2 across the whole layer: 10 shows 21.25 (white, black
    // unmagnified), 22 shows 27.25 (black, white unmagnified) and 25 shows
    // 28.75 (white, black unmagnified).
    let magnify = ImageFilter::try_magnify(Point::new(32.0, 32.0), 2.0).expect("a valid scale");
    let magnified = capture(renderer, &filtered(magnify, stripes()));
    dump_named("layer-transform-magnified", &magnified);
    for (x, expected) in [(10, WHITE), (22, BLACK), (25, WHITE)] {
        expect_pixel(&magnified, (x, 32), expected);
    }

    // Turned a quarter about 32, the vertical stripes lie across the layer:
    // row 32 shows x = 32.5 everywhere, black, and column 32 alternates.
    let quarter = capture(
        renderer,
        &filtered(
            turn(std::f64::consts::FRAC_PI_2, ImageSampling::Nearest),
            stripes(),
        ),
    );
    dump_named("layer-transform-quarter", &quarter);
    for x in [6, 22, 29, 45, 58] {
        expect_pixel(&quarter, (x, 32), BLACK);
    }
    for (y, expected) in [(18, BLACK), (21, WHITE), (54, WHITE), (58, BLACK)] {
        expect_pixel(&quarter, (32, y), expected);
    }

    // Moved 20 px right, a square drawn at 4..12 shows at 24..32, outside
    // the content's own bounds, and leaves the white under where it was.
    let moved = ImageFilter::Transform {
        transform: ImageTransform::try_new(1.0, 0.0, 0.0, 1.0, 20.0, 0.0).expect("a translation"),
        sampling: ImageSampling::Nearest,
    };
    let shifted = capture(
        renderer,
        &filtered(
            moved,
            picture(Rect::from_xywh(4.0, 4.0, 8.0, 8.0), Color::BLACK),
        ),
    );
    dump_named("layer-transform-moved", &shifted);
    for (x, expected) in [
        (6, WHITE),
        (10, WHITE),
        (25, BLACK),
        (30, BLACK),
        (34, WHITE),
    ] {
        expect_pixel(&shifted, (x, 8), expected);
    }
}
