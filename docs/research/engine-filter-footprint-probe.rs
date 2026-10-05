// Research-only witness, mounted temporarily as a child of painter/mod.rs.
// Does not repair production code. See the companion audit for the run command.
use super::WgpuPainter;
use crate::command_ir::{DrawItem, ImageFilterPass, ImageFilterSpec};
use flui_foundation::geometry::{Offset, Rect};
use flui_painting::{
    Canvas, Paint,
    paint::{Clip, ImageFilter},
    styling::Color,
};
use std::{sync::Arc, time::Instant};

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    rect: Rect<f64>,
    sigma: (f32, f32),
    scale: (f32, f32),
    chain: bool,
    nested: bool,
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
        ("fractional", -5.25, 9.75),
        ("outside_radius", -20.0, 10.0),
    ] {
        rows.push(Case {
            name,
            rect: Rect::from_xywh(x, y, 4.0, 4.0),
            sigma: (4.0, 4.0),
            scale: (1.0, 1.0),
            chain: false,
            nested: false,
        });
    }
    for (name, sigma, scale, chain, nested) in [
        ("anisotropic", (4.0, 2.0), (1.0, 1.0), false, false),
        ("scaled", (4.0, 2.0), (2.0, 1.5), false, false),
        ("nested_clip_opacity", (4.0, 4.0), (1.0, 1.0), false, true),
        ("chain_edge", (4.0, 4.0), (1.0, 1.0), true, false),
        ("inner_clip", (4.0, 4.0), (1.0, 1.0), false, false),
        ("x_only", (4.0, 0.0), (1.0, 1.0), false, false),
        ("y_only", (0.0, 4.0), (1.0, 1.0), false, false),
        ("zero", (0.0, 0.0), (1.0, 1.0), false, false),
    ] {
        let rect = match name {
            "zero" => Rect::from_xywh(8.0, 8.0, 4.0, 4.0),
            "y_only" => Rect::from_xywh(10.0, -6.0, 4.0, 4.0),
            _ => Rect::from_xywh(-5.25, 9.75, 4.0, 4.0),
        };
        rows.push(Case {
            name,
            rect,
            sigma,
            scale,
            chain,
            nested,
        });
    }
    rows
}

fn passes(c: Case) -> ImageFilterSpec {
    if c.chain {
        ImageFilterSpec::Chain(smallvec::smallvec![
            ImageFilterPass::Blur {
                sigma_x: c.sigma.0,
                sigma_y: c.sigma.1
            },
            ImageFilterPass::Blur {
                sigma_x: c.sigma.0,
                sigma_y: c.sigma.1
            }
        ])
    } else {
        ImageFilterSpec::Blur {
            sigma_x: c.sigma.0,
            sigma_y: c.sigma.1,
        }
    }
}

fn direct(p: &mut WgpuPainter, c: Case, pad: f64, size: u32) -> Vec<u8> {
    let start = Instant::now();
    eprintln!(
        "PROBE_BEGIN route=direct name={} pad={} size={}",
        c.name, pad, size
    );
    p.resize(size, size);
    p.begin_frame().expect("probe begin");
    p.translate(Offset::new(pad, pad));
    p.scale(c.scale.0, c.scale.1);
    if c.nested {
        p.save();
        p.clip_rect(Rect::from_xywh(-8.0, 0.0, 48.0, 32.0), Clip::HardEdge);
    }
    if c.name != "zero_reference" {
        p.save_layer_with_image_filter(passes(c));
    }
    if c.name == "inner_clip" {
        p.save();
        p.clip_rect(Rect::from_xywh(-8.0, 0.0, 16.0, 32.0), Clip::HardEdge);
    }
    if c.nested {
        p.save_layer(None, &Paint::fill(Color::BLACK).with_opacity(0.5));
    }
    p.draw_rect(c.rect, &Paint::fill(Color::BLACK));
    if c.nested {
        p.restore_layer();
    }
    if c.name == "inner_clip" {
        p.restore();
    }
    if c.name != "zero_reference" {
        p.restore_layer();
    }
    if c.nested {
        p.restore();
    }
    for item in &p.draw_order {
        if let DrawItem::Filter(op) = item {
            eprintln!(
                "FOOTPRINT {} pad={} origin={:?} dim={:?} source={:?} grown={:?} filters={}",
                c.name,
                pad,
                op.fb_origin,
                op.fb_dim,
                op.content_bounds,
                op.grown_bounds,
                op.passes.len()
            );
        }
    }
    let (tex, view) = crate::test_support::create_sampleable_target(
        p.device(),
        "probe",
        size,
        size,
        wgpu::TextureFormat::Rgba8Unorm,
    );
    crate::test_support::clear_target(p.device(), p.queue(), &view, wgpu::Color::WHITE);
    let mut encoder = p.device().create_command_encoder(&Default::default());
    p.render_to_texture(&tex, &mut encoder)
        .expect("probe encode");
    p.submit_encoder(encoder).expect("probe submit");
    let bytes = crate::test_support::readback_bytes(p.device(), p.queue(), &tex, size, size);
    p.finish_frame();
    eprintln!(
        "PROBE_END name={} pad={} elapsed_ms={:.3}",
        c.name,
        pad,
        start.elapsed().as_secs_f64() * 1000.0
    );
    bytes
}

fn scene(c: Case, pad: f64) -> flui_layer::LayerTree {
    let mut b = flui_layer::SceneBuilder::new();
    b.push_offset(Offset::new(pad, pad));
    b.push_transform(flui_foundation::geometry::Matrix4::scaling(
        f64::from(c.scale.0),
        f64::from(c.scale.1),
        1.0,
    ));
    if c.nested {
        b.push_clip_rect(Rect::from_xywh(-8.0, 0.0, 48.0, 32.0), Clip::HardEdge);
    }
    let blur = ImageFilter::Blur {
        sigma_x: f64::from(c.sigma.0),
        sigma_y: f64::from(c.sigma.1),
    };
    b.push_image_filter(if c.chain {
        ImageFilter::Compose(vec![blur.clone(), blur])
    } else {
        blur
    });
    if c.name == "inner_clip" {
        b.push_clip_rect(Rect::from_xywh(-8.0, 0.0, 16.0, 32.0), Clip::HardEdge);
    }
    if c.nested {
        b.push_opacity(0.5);
    }
    let mut canvas = Canvas::new();
    canvas.draw_rect(c.rect, &Paint::fill(Color::BLACK));
    b.add_picture(canvas.finish());
    b.build()
}

fn compare(small: &[u8], large: &[u8]) -> (u8, usize) {
    let mut max = 0;
    let mut differing = 0;
    for y in 0..32 {
        for x in 0..32 {
            for k in 0..4 {
                let d =
                    small[(y * 32 + x) * 4 + k].abs_diff(large[((y + 32) * 96 + x + 32) * 4 + k]);
                max = max.max(d);
                if d > 3 {
                    differing += 1;
                }
            }
        }
    }
    (max, differing)
}

#[test]
fn foreground_crop_research_witness() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPU required");
    eprintln!("ADAPTER {:?}", adapter.get_info());
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("device");
    let mut painter = WgpuPainter::with_shared_device(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8Unorm,
        (32, 32),
    );
    let headless =
        pollster::block_on(crate::HeadlessRenderer::new()).expect("headless GPU required");
    let mut failures = Vec::new();
    for c in cases() {
        let start = Instant::now();
        let small = direct(&mut painter, c, 0.0, 32);
        let large = direct(&mut painter, c, 32.0, 96);
        let (max, count) = compare(&small, &large);
        let ss = headless
            .render_layer_tree(&scene(c, 0.0), (32, 32))
            .expect("scene small");
        let sl = headless
            .render_layer_tree(&scene(c, 32.0), (96, 96))
            .expect("scene large");
        let (smax, scount) = compare(&ss, &sl);
        eprintln!(
            "RESULT {} direct_max={} direct_channels_gt3={} scene_max={} scene_channels_gt3={} pair_scene_direct_max={} pair_large_scene_direct_max={} elapsed_ms={:.3}",
            c.name,
            max,
            count,
            smax,
            scount,
            ss.iter()
                .zip(&small)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .expect("pixels"),
            sl.iter()
                .zip(&large)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .expect("pixels"),
            start.elapsed().as_secs_f64() * 1000.0
        );
        if c.name == "outside_radius" {
            assert!(
                small.iter().all(|v| *v == 255),
                "outside-radius source appeared"
            );
        }
        if c.name == "zero" {
            let reference = direct(
                &mut painter,
                Case {
                    name: "zero_reference",
                    ..c
                },
                0.0,
                32,
            );
            let zero_max = small
                .iter()
                .zip(&reference)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .expect("pixels");
            eprintln!("ZERO filtered_vs_unfiltered_max={zero_max}");
            if zero_max > 1 {
                failures.push("zero_identity");
            }
        }
        if max > 3 || smax > 3 {
            failures.push(c.name);
        }
    }
    let double_scene = |nested: bool| {
        let mut b = flui_layer::SceneBuilder::new();
        let blur = ImageFilter::blur(2.0);
        if nested {
            b.push_image_filter(blur.clone());
            b.push_image_filter(blur);
        } else {
            b.push_image_filter(ImageFilter::Compose(vec![blur.clone(), blur]));
        }
        let mut canvas = Canvas::new();
        canvas.draw_rect(
            Rect::from_xywh(40.0, 32.0, 2.0, 24.0),
            &Paint::fill(Color::BLACK),
        );
        b.add_picture(canvas.finish());
        b.build()
    };
    let composed = headless
        .render_layer_tree(&double_scene(false), (96, 96))
        .expect("chain");
    let nested = headless
        .render_layer_tree(&double_scene(true), (96, 96))
        .expect("nested");
    let chain_max = composed
        .iter()
        .zip(&nested)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .expect("pixels");
    eprintln!("CHAIN composed_vs_nested_max={chain_max}");
    if chain_max > 3 {
        failures.push("chain_support");
    }
    let perf = Case {
        name: "perf_interior",
        rect: Rect::from_xywh(12.0, 12.0, 8.0, 8.0),
        sigma: (4.0, 4.0),
        scale: (1.0, 1.0),
        chain: false,
        nested: false,
    };
    for _ in 0..3 {
        let _ = direct(&mut painter, perf, 0.0, 32);
    }
    let mut times = Vec::new();
    for _ in 0..10 {
        let start = Instant::now();
        let _ = direct(&mut painter, perf, 0.0, 32);
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    eprintln!(
        "PERF capture_wait_readback_ms min={:.3} median={:.3} max={:.3} warmup=3 samples=10",
        times[0],
        (times[4] + times[5]) / 2.0,
        times[9]
    );
    assert!(
        failures.is_empty(),
        "baseline foreground crop/support defects: {failures:?}"
    );
}
