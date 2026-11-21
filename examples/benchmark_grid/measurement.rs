//! Wall-clock samples of the existing synchronous widget driver, not CPU/GPU time.
use std::time::{Duration, Instant};

use flui_testing::{FrameReport, HeadlessBinding, MountOptions, MountOwners};
use flui_widgets::{FocusRoot, GestureArenaScope};
use serde::Serialize;

use super::scene::{GridScene, HEIGHT, WIDTH};

pub const FRAME: Duration = Duration::from_nanos(16_666_667);

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Retained,
    Local,
    Rebuild,
}

pub fn mount(scene: &GridScene) -> HeadlessBinding {
    let mut binding = HeadlessBinding::new();
    let root = GestureArenaScope::new(binding.arena().clone(), FocusRoot::new(scene.clone()));
    let _ = binding.mount_root(
        &root,
        MountOwners::fresh(),
        MountOptions::tight(f64::from(WIDTH), f64::from(HEIGHT)),
    );
    binding
}

#[derive(Serialize)]
pub struct Sample {
    pub elapsed_ns: u64,
    pub counters: [(&'static str, u64); 10],
}

pub fn measure(operation: impl FnOnce() -> FrameReport) -> Sample {
    let start = Instant::now();
    let report = operation();
    let elapsed = start.elapsed();
    Sample {
        elapsed_ns: u64::try_from(elapsed.as_nanos()).expect("BUG: sample exceeds 584 years"),
        counters: report.counters(),
    }
}

pub fn sample(
    binding: &mut HeadlessBinding,
    scene: &GridScene,
    mode: Mode,
    iteration: usize,
) -> Sample {
    measure(|| {
        match mode {
            Mode::Retained => {}
            Mode::Local => scene
                .header
                .update(|text| *text = format!("Benchmark {}", iteration % 2)),
            Mode::Rebuild => {
                binding.perform_reassemble();
                binding.reassemble_render_tree();
            }
        }
        binding.pump_frame_report(FRAME)
    })
}

/// Nearest-rank percentile, including both ends; input is sorted by the caller.
pub fn percentile(sorted: &[u64], percent: usize) -> u64 {
    assert!(!sorted.is_empty() && (1..=100).contains(&percent));
    sorted[(sorted.len() * percent).div_ceil(100) - 1]
}

#[cfg(test)]
pub fn paragraphs(binding: &HeadlessBinding) -> Vec<String> {
    binding
        .pipeline_owner()
        .expect("BUG: mounted scene has no pipeline")
        .with(|owner| {
            let snapshot = flui_rendering::testing::inspect::render_snapshot(owner);
            snapshot
                .find_by_kind("RenderParagraph")
                .into_iter()
                .filter_map(flui_rendering::testing::inspect::paragraph_text)
                .collect::<Vec<String>>()
        })
}
