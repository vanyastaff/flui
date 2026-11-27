# ADR-0404: The web's WebGL2 fallback is a feature an application names

- **Status:** Implemented (2026-11-27)
- **Date:** 2026-11-27
- **Supersedes in part:** [ADR-0244](ADR-0244-os-version-floors-and-gates.md) decision 1's Web
  fallback only ("WebGPU on the Web … falls back to … WebGL2 on the Web when no adapter answers"):
  WebGL2 is tried after WebGPU where the application names the `webgl2` feature. Unchanged: the
  probe, one instance per attempt, the refusal of decision 3 and every native order.
- **Related:** [ADR-0088](ADR-0088-official-packages-sdk-and-facade.md) §6 (the facade turns nothing on
  by default), [ADR-0397](ADR-0397-an-icon-face-is-in-a-binary-only-where-a-package-or-the-application-asks-for-it.md),
  [ADR-0398](ADR-0398-the-release-build-mode-leaves-trace-and-debug-events-out.md) and
  [ADR-0402](ADR-0402-a-binary-links-the-development-agent-only-where-a-hook-is-installed.md) (the
  other parts of a release binary that only some applications use)
- **Sources:** `wgpu` 30.0 `Cargo.toml` (`webgl = ["web", "wgpu-core/webgl", "dep:wgpu-hal", …]`,
  its web dependencies under `cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))`) and
  `wgpu-core` 30.0 (`webgl = ["wgpu-core-deps-wasm/webgl"]`, the crate that brings `wgpu-hal`'s GL
  backend to wasm32); web.dev, "WebGPU is now supported in major browsers" (2025-11)

## Context and problem statement

On wasm32 `flui-engine` turned on wgpu's `webgpu`, `gles` and `webgl` for every application, and
`device::backend` asked WebGL2 when no WebGPU adapter answered (ADR-0244 §1). What bears on it:

| Fact | Evidence |
|---|---|
| WebGPU on the web is the browser's own API: wgpu forwards to it and links neither `wgpu-core` nor a HAL backend. `webgl` links `wgpu-core`, `wgpu-hal`'s GL backend and the GLSL path of `naga` into the module | the wgpu and wgpu-core manifests (Sources); `cargo update` drops `wgpu-core-deps-wasm` from the lock graph once `webgl` is off |
| Built as CI's `size` job builds it, the generated counter's wasm32 module is 12,803,927 bytes with the fallback and 10,680,923 without (2,123,004, 16.6%); brotli at quality 11 takes them to 2,994,272 and 2,517,093 (477,179, 16%), what a page transfers; native binaries are unchanged | local builds with the `size` job's commands, 2026-11-24 and 2026-11-27; `counter_wasm_bytes` in `perf/size.toml` |
| WebGPU ships in Chrome and Edge on Windows, macOS, ChromeOS and Android 12+, Safari 26 and Firefox on Windows and Apple-silicon macOS. It does not yet in Chrome or Firefox on Linux, Firefox on Android or Intel Macs, or on Android before 12 | Sources (web.dev, 2025-11) |
| No browser has run the WebGL2 fallback; the pixel suites pass on llvmpipe GL only | `docs/gaps.md`, "Version floors and the GPU fallback not run on devices" |
| A browser without the backends a build carries is already refused at start, with `EngineError::NoSupportedGpu` naming each backend tried, as `UnsupportedSystem` (ADR-0244 §3) | `crates/flui-app/src/app/runner/web.rs`, `crates/flui-engine/src/device/backend.rs` |
| Flutter's default web build draws through WebGL in every browser it supports; a build flag chooses its renderer | Flutter's web renderers (`flutter build web`, `--wasm`) |
| The facade turns no feature on by default (ADR-0088 §6), and `flui create` writes the features a generated project uses into its manifest | `Cargo.toml`, `crates/flui-cli/src/templates/` |

Who decides whether a web build carries the WebGL2 fallback?

## Decision drivers

- An application that ships only to WebGPU browsers does not carry 2.1 MB of a backend it never
  draws with.
- A generated application keeps today's reach: it draws in a browser without WebGPU.
- The choice is visible where the author makes it, in the application's own manifest, and states
  its price.
- No default feature on the facade (ADR-0088 §6), and no feature that removes something.
- Native targets are untouched.

## Considered options

1. Keep the fallback in every web build (today).
2. Drop the fallback: the Web's floor becomes a WebGPU browser.
3. A `webgl2` feature, on by default in the facade.
4. A `webgl2` feature the application names, which `flui create` writes into the projects it
   generates.

## Decision

Chosen option: 4, because it keeps a generated application's reach while letting an author who
targets WebGPU browsers drop the backend with one edit of a line that says what it costs, and it
keeps the facade's defaults empty.

### §1. `webgl2` builds the fallback and puts it in the web order

`flui-engine`'s `webgl2` feature turns on `wgpu/webgl`, which wgpu builds only for wasm32, so no
other target links anything for it. `device::backend::Family::Web`'s order is WebGPU, then WebGL2
in an instance of its own, where the build names `webgl2`, and WebGPU alone where it does not. A
browser that gives no WebGPU adapter to a build without `webgl2` is refused at start as ADR-0244
§3 says, with the one failure. `flui-app`'s `webgl2` and the facade's forward to it.

### §2. An application names it; `flui create` writes it

The facade keeps `default = []`. Every application template of `flui create` (`empty`, `basic`,
`counter`, and the host of a `--hot-reload` project) names `webgl2` on its `flui` dependency, with
a comment that removing it ships the web build on WebGPU only, about 2 MB smaller. The `widget`
template is a library and names none: the application that links it chooses. An application
whose manifest is not generated names it as it names any other feature.

### §3. The floor says so

`platform-support.toml`'s Web row reads "WebGPU, or WebGL2 where the application names the
`webgl2` feature", and `docs/version-map.md` is generated from it.

### Consequences

- Good, because an application that drops the feature ships a 16.6% smaller wasm32 module (the
  counter: 10,680,923 bytes against 12,803,927).
- Good, because a generated application draws where it did, and its author sees the trade in
  their manifest.
- Good, because native builds and the facade's defaults are unchanged.
- Bad, because a hand-written manifest that omits `webgl2` refuses a browser without WebGPU, which
  it would have drawn in before. The refusal names WebGPU as the backend tried.
- Bad, because the `size` job builds the generated counter, which names `webgl2`, so it does not
  measure the WebGPU-only module, and the workspace's own wasm32 builds (`examples/web_counter`,
  `wasm-check`) name no `webgl2` and carry no GL backend.
- Neutral, because the WebGL2 path is as unproven in a browser as it was (`docs/gaps.md`).

### Confirmation

- `gpu_backend_fallback_order` (`flui-engine`, `device::backend`) holds both web orders: WebGL2
  after WebGPU with `webgl2`, WebGPU alone without it, and `Family::Web` taking the build's.
- `flui-cli`'s `generated_*_project_compiles` check that each application template names
  `webgl2` and says what removing it does, and that the `widget` template names none.
- `flui-cli`'s `platform_floor` holds `docs/version-map.md` to `platform-support.toml`.

## Pros and cons of the options

### 1. Keep the fallback everywhere

- Good, because every browser with WebGPU or WebGL2 draws.
- Bad, because every web application carries 2.1 MB that a WebGPU browser never runs.

### 2. Drop the fallback

- Good, because every web build is 2.1 MB smaller and one unproven path goes.
- Bad, because Linux desktops, Firefox on Android and Android before 12 are refused, a reach
  Flutter's default web build keeps.

### 3. On by default in the facade

- Good, because a manifest that says nothing keeps today's reach, as wgpu's own default features
  do.
- Bad, because the facade's defaults are empty (ADR-0088 §6), and an author has to know to set
  `default-features = false` to see the trade.

### 4. Named by the application, written by `flui create`

- Good, because the trade is a line in the author's manifest.
- Good, because the facade's defaults stay empty.
- Bad, because a hand-written manifest gets the smaller reach unless its author reads the
  feature's documentation.

## More information

Revisit when WebGPU ships on the platforms the table lists without it: the template stops naming
`webgl2`, and the feature can go with the next floor change (ADR-0244 decision 9).
