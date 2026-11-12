### Changed

- `InkWell` makes its state layer's animation at the first state that shows the layer and lets it go once the well is back at rest, so a resting well holds no animation controller and no frame registration (its mount cost is held by `packages/flui-material/perf/baseline/ink_well_over_text.toml`).
