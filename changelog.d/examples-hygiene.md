### Removed

- The demo apps and their tests: the `material_demo`, `cupertino_demo`, `vertical_slice_demo`,
  `two_screens` (Notes), `widgets_gallery`, `colored_box_app`, `text_app`, `sliver_demo`,
  `motion_lab`, `form`, `screenshot`, `notes_faults` and `animated_box_app` examples, the
  interaction gallery, and the demo layer snapshots. `examples/counter.rs`, the hot-reload
  examples and the `web_demo` and `painting_demo` WASM demos remain.
- The self-driving device probes: the `a11y_probe`, `lifecycle_probe`, `workload_probe`,
  `resize_jitter_probe` and `ios_demo` examples, every example of `flui-native`, and
  `tools/device-checks`. `cargo xtask device` and its checks are gone with them.
- `cargo xtask live-smoke`, `cargo xtask demo-snapshots`, `tools/live-smoke` and the `live-smoke`
  CI job.
- The examples of `flui-animation`, `flui-assets`, `flui-foundation`, `flui-interaction`,
  `flui-painting`, `flui-native`, `flui-rendering`, `flui-scheduler` and `flui-devtools`, and
  the book's todo tutorial.
