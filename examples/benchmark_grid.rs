//! Run `cargo heli run --locked --release --example benchmark_grid -- --help`.
//! See `examples/benchmark_grid/README.md` for the timing boundary and correctness contract.
#[path = "benchmark_grid/correctness.rs"]
mod correctness;
#[path = "benchmark_grid/measurement.rs"]
mod measurement;
#[path = "benchmark_grid/scene.rs"]
mod scene;

use measurement::{Mode, mount, percentile, sample};
use scene::{GridScene, HEIGHT, Kind, Shape, WIDTH};
use serde_json::json;
use std::{
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
    time::Instant,
};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help") {
        println!(
            "benchmark_grid --items 100|1000|10000|53000 --kind labels|buttons --mode retained|local|rebuild --frames 10000 --warmup 120 --out NEW_DIRECTORY [--capture]\nMeasures wall time of state mutation + HeadlessBinding::pump_frame_report. No native presentation or GPU timing."
        );
        return Ok(());
    }
    let mut items = 1000;
    let mut frames = 10000;
    let mut warmup = 120;
    let mut kind = Kind::Labels;
    let mut mode = Mode::Retained;
    let mut out = None;
    let mut capture = false;
    let mut index = 0;
    while index < args.len() {
        let key = &args[index];
        if key == "--capture" {
            capture = true;
            index += 1;
            continue;
        }
        let value = args.get(index + 1).ok_or("missing argument value")?;
        match key.as_str() {
            "--items" => items = value.parse::<usize>()?,
            "--frames" => frames = value.parse::<usize>()?,
            "--warmup" => warmup = value.parse::<usize>()?,
            "--kind" => {
                kind = match value.as_str() {
                    "labels" => Kind::Labels,
                    "buttons" => Kind::Buttons,
                    _ => return Err("unknown kind".into()),
                }
            }
            "--mode" => {
                mode = match value.as_str() {
                    "retained" => Mode::Retained,
                    "local" => Mode::Local,
                    "rebuild" => Mode::Rebuild,
                    _ => return Err("unknown mode".into()),
                }
            }
            "--out" => out = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown argument: {key}").into()),
        }
        index += 2;
    }
    let shape = Shape::for_items(items).ok_or("unsupported item count")?;
    if !(1..=100_000).contains(&frames) || warmup > 10_000 {
        return Err("sample count out of range".into());
    }
    let out = out.ok_or("--out is required")?;
    // A failed run leaves its directory behind; it cannot overwrite a valid run.
    fs::create_dir(&out)?;
    let clock_samples: Vec<u64> = (0..10_000)
        .map(|_| measurement::measure(flui_testing::FrameReport::default).elapsed_ns)
        .collect();
    let started = Instant::now();
    let scene = GridScene::new(shape, kind);
    let mut binding = mount(&scene);
    let mount_ns = started.elapsed().as_nanos();
    let render_nodes = binding
        .pipeline_owner()
        .ok_or("missing pipeline")?
        .with(|owner| owner.render_tree().len());
    correctness::verify_scene(&binding, &scene, "Benchmark 0")?;
    correctness::verify_input(&mut binding, &scene)?;
    if capture {
        let gpu = pollster::block_on(flui_engine::HeadlessRenderer::new())?;
        let pixels = gpu.render_layer_tree(
            binding.layer_tree().ok_or("missing layers")?,
            (WIDTH, HEIGHT),
        )?;
        correctness::verify_pixels(&pixels, shape)?;
        image::save_buffer(
            out.join("before.png"),
            &pixels,
            WIDTH,
            HEIGHT,
            image::ColorType::Rgba8,
        )?;
        let _ = sample(&mut binding, &scene, Mode::Local, 1);
        let changed = gpu.render_layer_tree(
            binding.layer_tree().ok_or("missing layers")?,
            (WIDTH, HEIGHT),
        )?;
        correctness::verify_pixels(&changed, shape)?;
        correctness::verify_header_delta(&pixels, &changed)?;
        image::save_buffer(
            out.join("changed.png"),
            &changed,
            WIDTH,
            HEIGHT,
            image::ColorType::Rgba8,
        )?;
        let _ = sample(&mut binding, &scene, Mode::Local, 0);
    }
    for iteration in 1..=warmup {
        let _ = sample(&mut binding, &scene, mode, iteration);
    }
    let mut samples = Vec::with_capacity(frames);
    // Serialization, disk writes, tree inspection, GPU capture and statistics are outside timing.
    for iteration in (warmup + 1)..=(warmup + frames) {
        samples.push(sample(&mut binding, &scene, mode, iteration));
    }
    let expected = if matches!(mode, Mode::Local) {
        format!("Benchmark {}", (warmup + frames) % 2)
    } else {
        String::from("Benchmark 0")
    };
    correctness::verify_scene(&binding, &scene, &expected)?;
    let mut raw = BufWriter::new(File::create_new(out.join("samples.jsonl"))?);
    for value in &samples {
        serde_json::to_writer(&mut raw, value)?;
        writeln!(raw)?;
    }
    raw.flush()?;
    let mut times: Vec<u64> = samples.iter().map(|value| value.elapsed_ns).collect();
    times.sort_unstable();
    let mut counts = std::collections::BTreeMap::<&str, u64>::new();
    for value in &samples {
        for (key, count) in value.counters {
            *counts.entry(key).or_default() += count;
        }
    }
    serde_json::to_writer(
        File::create_new(out.join("clock-samples.json"))?,
        &clock_samples,
    )?;
    let mut clock = clock_samples;
    clock.sort_unstable();
    let summary = json!({
        "schema": 1, "status": "VALID", "items": items,
        "kind": format!("{kind:?}"), "mode": format!("{mode:?}"),
        "warmup": warmup, "samples": frames, "mount_wall_ns": mount_ns,
        "wall_ns": {
            "min": times[0], "p50": percentile(&times,50),
            "p95": percentile(&times,95), "p99": percentile(&times,99), "max": times[frames-1]
        },
        "empty_measurement_wall_ns": {"min":clock[0], "p50":percentile(&clock,50), "p99":percentile(&clock,99)},
        "counter_totals": counts,
        "correctness": {"paragraphs_before_and_after":items+1,"final_header":expected,"geometry_and_input":"passed", "gpu_pixels":if capture {"passed"} else {"not_requested"}},
        "scene": {
            "columns":shape.columns,"rows":shape.rows,"sidebar_items":shape.sidebar,
            "visible_items_including_partially_clipped":shape.columns * shape.rows.min(58) + shape.sidebar.min(36),
            "render_nodes":render_nodes,"body_repaint_boundary":true,"virtualized":false,
            "font":"bundled Roboto Regular","viewport":[WIDTH,HEIGHT],"scale":1
        },
        "virtual_dt_ns": measurement::FRAME.as_nanos(),
        "timing_boundary":"state mutation plus HeadlessBinding::pump_frame_report; wall time, not process CPU time",
        "gpu_time": null, "input_to_present": null, "native_runtime": false, "args":args
    });
    serde_json::to_writer_pretty(File::create_new(out.join("summary.json"))?, &summary)?;
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
