### Changed

- **flui-engine shaders compose at build time with WESL**: shaders that share
  modules import them with WESL (`import package::color_filter::filter_mode;`),
  and `build.rs` resolves the imports into plain WGSL with the `wesl` compiler.
  The pipelines include that WGSL, so `naga_oil` and `regex` no longer ship in
  the engine (native or wasm32) and no shader is composed while a pipeline is
  created.
