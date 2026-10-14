### Added

- `flui_testing::backend_conformance`: `assert_backend_conforms` certifies an input backend
  through `BackendUnderTest` against one table of rows — pointer assembly, clock choice, order
  and wrap, the phased `Scroll` and scale-and-rotation `PanZoom` grammar, undecodable readings,
  availability as claimed, and no backend `Claimed` cancel (ADR-0215 §7). The headless backend
  and the shared decoders of every native backend run it in CI.
