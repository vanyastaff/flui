### Added

- The renderer falls back when the preferred GPU backend gives no device (ADR-0244 §1): Vulkan to GLES 3.0 on Linux and Android, WebGPU to WebGL2 on the web, hardware Direct3D 12 to WARP on Windows. Each backend is tried in an instance of its own, and `WGPU_BACKEND` narrows the order.
- `flui::GpuBackend` and `GpuCapabilities::backend`: which backend the renderer chose, logged once at start.
- `UnsupportedSystem::NoSupportedGpu` and `EngineError::NoSupportedGpu`, carrying a `BackendFailure` (backend and reason) for each backend tried, when none answers.

### Changed

- Gradient stops are read from a texture instead of a fragment-stage storage buffer, which GLES 3.0 and WebGL2 lack.
- `flui-engine` compiles wgpu's `gles` backend on Linux and Android and `webgl` on the web.

### Fixed

- On GL drivers that offer dual-source blending (Mesa's llvmpipe among them), a gradient or image drawn with `Plus` or another destination-reading blend after a coverage-correct draw no longer draws nothing: the isolation pass's targets now set their blend function instead of leaving the dual-source one Mesa refuses with two draw buffers.
- A rect, gradient or clip edge on pixel boundaries covers its corner pixels fully on drivers with fine derivatives (lavapipe, llvmpipe), where it left them at 94 %: the antialiasing band is measured through the local position's derivatives and the field's normal instead of differentiating the distance, which creases inside each corner.
