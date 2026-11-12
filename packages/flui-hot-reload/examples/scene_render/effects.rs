//! Six deterministic panels exercising the native layer compositor.
use flui_foundation::{
    LayerId,
    geometry::{Matrix4, Offset, RRect, Rect, Size},
};
use flui_layer::{
    BackdropFilterLayer, ClipRectLayer, FollowerLayer, Layer, LayerLink, LayerTree, LeaderLayer,
    OffsetLayer, PictureLayer, Scene, ShaderMaskLayer, TransformLayer,
};
use flui_painting::{
    BlendMode, Canvas, Paint, Shader,
    paint::{ImageFilter, TileMode},
    styling::Color,
};

const SIDE: f64 = 200.0;
fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect<f64> {
    Rect::from_xywh(x, y, width, height)
}
fn picture(canvas: Canvas) -> Layer {
    Layer::from(PictureLayer::new(canvas.finish()))
}
fn solid(bounds: Rect<f64>, color: Color) -> Layer {
    let mut canvas = Canvas::new();
    canvas.draw_rect(bounds, &Paint::fill(color).with_anti_alias(false));
    picture(canvas)
}
fn mask(
    tree: &mut LayerTree,
    parent: LayerId,
    shader: Shader,
    mode: BlendMode,
    bounds: Rect<f64>,
) -> LayerId {
    tree.push_child(
        parent,
        Layer::from(ShaderMaskLayer::new(shader, mode, bounds)),
    )
}
fn nested(tree: &mut LayerTree, parent: LayerId) {
    // Reference red tile and two nested half-alpha groups, over the same white.
    tree.push_child(parent, solid(rect(15.0, 25.0, 45.0, 150.0), Color::RED));
    let outer = mask(
        tree,
        parent,
        Shader::solid(Color::rgba(255, 255, 255, 128)),
        BlendMode::DstIn,
        rect(70.0, 25.0, 110.0, 150.0),
    );
    tree.push_child(outer, solid(rect(70.0, 25.0, 55.0, 150.0), Color::RED));
    let inner = mask(
        tree,
        outer,
        Shader::solid(Color::rgba(255, 255, 255, 128)),
        BlendMode::DstIn,
        rect(125.0, 25.0, 55.0, 150.0),
    );
    tree.push_child(inner, solid(rect(125.0, 25.0, 55.0, 150.0), Color::RED));
}
fn gradients(tree: &mut LayerTree, parent: LayerId) {
    for index in 0..3 {
        let x = 12.0 + f64::from(index) * 60.0;
        let colors = vec![Color::rgb(255, 100, 20), Color::rgb(30, 80, 255)];
        let shader = match index {
            0 => Shader::linear_gradient(
                Offset::new(x, 30.0),
                Offset::new(x + 48.0, 160.0),
                colors,
                None,
                TileMode::Clamp,
            ),
            1 => Shader::radial_gradient(
                Offset::new(x + 18.0, 80.0),
                90.0,
                colors,
                None,
                TileMode::Clamp,
                None,
                None,
            ),
            _ => Shader::sweep_gradient(
                Offset::new(x + 24.0, 100.0),
                colors,
                None,
                TileMode::Clamp,
                0.0,
                std::f64::consts::TAU,
            ),
        };
        let center = x + 24.0;
        let matrix = Matrix4::translation(center, 100.0, 0.0)
            * Matrix4::rotation_z(0.18)
            * Matrix4::translation(-center, -100.0, 0.0);
        let transformed = tree.push_child(parent, Layer::from(TransformLayer::new(matrix)));
        mask(
            tree,
            transformed,
            shader,
            BlendMode::Src,
            rect(x, 30.0, 48.0, 140.0),
        );
    }
}
fn backdrop(tree: &mut LayerTree, parent: LayerId) {
    let mut canvas = Canvas::new();
    // Both axes contain discontinuities, so sigma_y=0 remains visibly sharp
    // across horizontal checker edges while the second kernel softens them.
    for y in 0..10 {
        for x in 0..10 {
            canvas.draw_rect(
                rect(f64::from(x) * 20.0, f64::from(y) * 20.0, 20.0, 20.0),
                &Paint::fill(if (x + y) % 2 == 0 {
                    Color::BLACK
                } else {
                    Color::WHITE
                })
                .with_anti_alias(false),
            );
        }
    }
    tree.push_child(parent, picture(canvas));
    for (bounds, sigma_x, sigma_y) in [
        (rect(15.0, 30.0, 130.0, 85.0), 4.0, 0.0),
        (rect(55.0, 85.0, 130.0, 85.0), 2.0, 5.0),
    ] {
        tree.push_child(
            parent,
            Layer::from(BackdropFilterLayer::new(
                ImageFilter::Blur { sigma_x, sigma_y },
                BlendMode::Src,
                bounds,
            )),
        );
    }
    tree.push_child(parent, solid(rect(160.0, 15.0, 25.0, 25.0), Color::GREEN));
}
fn destructive(tree: &mut LayerTree, parent: LayerId) {
    // Clear affects the isolated blue child. Its transparent result composites
    // SrcOver and reveals the orange parent; it does not erase the parent.

    tree.push_child(
        parent,
        solid(rect(20.0, 20.0, 160.0, 160.0), Color::rgb(255, 150, 0)),
    );
    let group = mask(
        tree,
        parent,
        Shader::solid(Color::WHITE),
        BlendMode::Clear,
        rect(55.5, 45.5, 90.0, 110.0),
    );
    tree.push_child(group, solid(rect(0.0, 0.0, SIDE, SIDE), Color::BLUE));
    tree.push_child(parent, solid(rect(155.0, 155.0, 30.0, 30.0), Color::GREEN));
}
fn follower(tree: &mut LayerTree, parent: LayerId) {
    let link = LayerLink::new();
    let leader = tree.push_child(
        parent,
        Layer::from(LeaderLayer::with_offset(
            link,
            Size::new(65.0, 65.0),
            Offset::new(90.0, 35.0),
        )),
    );
    tree.push_child(
        leader,
        solid(rect(0.0, 0.0, 65.0, 65.0), Color::rgb(255, 170, 0)),
    );
    let branch = tree.push_child(
        parent,
        Layer::from(OffsetLayer::new(Offset::new(10.0, 120.0))),
    );
    let linked = tree.push_child(
        branch,
        Layer::from(
            FollowerLayer::new(link)
                .with_size(Size::new(65.0, 65.0))
                .with_show_when_unlinked(false),
        ),
    );
    tree.push_child(linked, solid(rect(12.0, 12.0, 40.0, 40.0), Color::BLUE));
    tree.push_child(parent, solid(rect(20.0, 155.0, 160.0, 25.0), Color::GREEN));
}
fn rounded(tree: &mut LayerTree, parent: LayerId) {
    let matrix = Matrix4::translation(25.0, 45.0, 0.0) * Matrix4::scaling(0.01, 0.01, 1.0);
    let scaled = tree.push_child(parent, Layer::from(TransformLayer::new(matrix)));
    let mut canvas = Canvas::new();
    let shader = Shader::linear_gradient(
        Offset::ZERO,
        Offset::new(15000.0, 11000.0),
        vec![Color::rgb(0, 180, 160), Color::rgb(80, 35, 230)],
        None,
        TileMode::Clamp,
    );
    canvas.draw_rrect(
        RRect::from_rect_circular(rect(0.0, 0.0, 15000.0, 11000.0), 2200.0),
        &Paint::fill(Color::WHITE).with_shader(shader),
    );
    tree.push_child(scaled, picture(canvas));
}

pub(super) fn build(width: f64, height: f64) -> Scene {
    let mut tree = LayerTree::new(solid(rect(0.0, 0.0, width, height), Color::rgb(20, 30, 48)));
    let panel_width = ((width - 32.0) / 3.0).max(1.0);
    let panel_height = ((height - 24.0) / 2.0).max(1.0);
    for index in 0..6 {
        let column = index % 3;
        let row = index / 3;
        let matrix = Matrix4::translation(
            8.0 + f64::from(column) * (panel_width + 8.0),
            8.0 + f64::from(row) * (panel_height + 8.0),
            0.0,
        ) * Matrix4::scaling(panel_width / SIDE, panel_height / SIDE, 1.0);
        let transformed = tree.push_child(tree.root(), Layer::from(TransformLayer::new(matrix)));
        let panel = tree.push_child(
            transformed,
            Layer::from(ClipRectLayer::hard_edge(rect(0.0, 0.0, SIDE, SIDE))),
        );
        tree.push_child(panel, solid(rect(0.0, 0.0, SIDE, SIDE), Color::WHITE));
        match index {
            0 => nested(&mut tree, panel),
            1 => gradients(&mut tree, panel),
            2 => backdrop(&mut tree, panel),
            3 => destructive(&mut tree, panel),
            4 => follower(&mut tree, panel),
            _ => rounded(&mut tree, panel),
        }
    }
    Scene::new(tree)
}
