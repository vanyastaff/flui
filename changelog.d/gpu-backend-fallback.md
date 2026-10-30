### Added

- The renderer falls back when the preferred GPU backend gives no device (ADR-0244 §1): Vulkan to GLES 3.0 on Linux and Android, WebGPU to WebGL2 on the web, hardware Direct3D 12 to WARP on Windows. Each backend is tried in an instance of its own, and `WGPU_BACKEND` narrows the order.
- `flui::GpuBackend` and `GpuCapabilities::backend`: which backend the renderer chose, logged once at start.
- `UnsupportedSystem::NoSupportedGpu` and `EngineError::NoSupportedGpu`, carrying a `BackendFailure` (backend and reason) for each backend tried, when none answers.

### Changed

- Gradient stops are read from a texture instead of a fragment-stage storage buffer, which GLES 3.0 and WebGL2 lack.
- `flui-engine` compiles wgpu's `gles` backend on Linux and Android and `webgl` on the web.
