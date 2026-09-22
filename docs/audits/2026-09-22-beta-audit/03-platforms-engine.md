# FLUI — Platform Backends and Rendering Engine Audit

Sources: `crates/flui-platform/**`, `crates/flui-app/**`, `platforms/**`,
`docs/BETA.md` (present only on branch `origin/codex/beta-release-preparation`,
not on `main` at `fbed9408` — read via `git show`), `.github/workflows/ci.yml`,
`justfile`, `crates/flui-engine/**`, `crates/flui-painting/**`,
`crates/flui-layer/**`, `crates/flui-assets/**`.

Note on `docs/BETA.md`: it does not exist on `main`. The most current copy
lives on `origin/codex/beta-release-preparation` and is the primary source for
the live-verification evidence below (dated 2026-09-19 through 2026-09-22).
Treat everything sourced from it as "prepared on a feature branch, not yet
merged to main."

## 1. Platform matrix

Legend for evidence level: **live** = real OS input/output verified by a human
or scripted operator-equivalent channel (CGEventPost, XCUITest, adb, real X11);
**CI** = executed automatically in `.github/workflows/ci.yml`; **compile-only**
= `cargo check`/`clippy` cross-compiles the target but nothing runs; **absent**
= no code path or no evidence at all.

| Capability | macOS (AppKit/Metal) | Windows (Win32) | Linux (X11/Wayland via winit) | iOS (UIKit) | Android | Web/WASM |
|---|---|---|---|---|---|---|
| Window creation | live (`macos-close-path`, `macos-frame-pump`, launch-route gate) | compile-only (`cross-typecheck`, `x86_64-pc-windows-msvc`) | CI (`live-smoke` job, Xvfb + real X11) | live (XCUITest counter/demo launch) | live (one emulator run, 2026-09-22) | live (one browser pane, one machine) |
| Resize | live (`macos-lifecycle` probe: ±1px on `setFrame:display:`) | absent live; issue #185 (Windows resize jitter) is a known open item, unverified either way here | not exercised by `live-smoke` (fixed 1200×800 Xvfb screen) | untested (safe-area check is portrait-only, no resize-while-mounted) | untested (no rotate/resize case run) | live: `ResizeObserver` + CSS `100vw/100vh` follow-viewport landed 2026-09-22, one viewport-change case verified |
| DPI/scale | implicit in launch-route/lifecycle probes; not separately isolated | compile-only | not verified | live (`safeAreaInsets`, device pixel ratio implied by simulator) | density 420 noted in the one run | live: backing store at device pixel ratio verified (1960×2520 @ dpr 2) |
| Pointer/mouse | live (`CGEventPost`/`CGHIDEventTap` clicks) | absent | CI (`live-smoke`: XTEST drag/scroll) | n/a (touch only) | n/a (touch only) | live (canvas clicks in browser pane) |
| Touch | n/a | n/a | n/a | live (XCUITest taps, `check-ios-input.py`) | live (adb `input tap`, one working case after `bf2725be` fix) | absent — no touch test on web |
| Keyboard + physical key mapping | hand-written table `shared/keys_macos.rs` | hand-written table `shared/keys.rs`; both unified vs winit's delegated `ui-events-winit` bridge only after issue #1092 fix (`platforms/winit/events.rs`) | delegated to `ui-events-winit` (same bridge as Masonry/Xilem) | untested | untested | untested |
| IME/text input | live but synthetic: `just macos-ime`/`ime_probe` exercises `NSTextInputClient` routing with synthesized keys — "no genuine input method runs, so real IME composition is unverified" | absent | absent | absent (no IME check on iOS) | absent | absent |
| Clipboard | present (`platforms/macos/clipboard.rs`) but explicitly "unverified" per BETA.md macOS row | present (`platforms/winit/clipboard.rs`) but issue #1065 (Windows clipboard) flagged as an open concern; not exercised live here | present, not separately live-verified beyond compile/unit tests | present (`platforms/ios/clipboard.rs`), untested | absent code path found | present (`platforms/web/clipboard.rs`), untested |
| Cursor | code present in macOS/winit window modules; not isolated in any live probe | compile-only | not isolated | n/a | n/a | not isolated |
| Multi-window | live: `examples/multi_window_demo.rs`, `open_window(..., SeparateRealms)` — primary opens a fully mounted secondary window, both close cleanly. `SharedRealm` explicitly refused at admission (no per-presentation raster contract yet) | compile-only; "Windows show" cross-compiled only | untested | UIKit single-scene policy: "Shipping bundles declare one scene at a time" — full multiwindow explicitly unimplemented | untested | untested |
| Lifecycle (suspend/resume/quit/minimize/hide) | live and thorough: `macos-lifecycle` probe covers minimize/hide/restore/unhide with frame-count budgets (0 frames while hidden, back to full rate on restore); native quit/reopen probes (`exit_policy_probe`, `on_reopen`) cover explicit quit, Cmd+Q-equivalent process exit (3 runs), reopen with/without window | untested; "no live window, input, lifecycle, or IME verification has been performed on Windows" | not covered beyond process-exit in `live-smoke`/`live-smoke-wayland` (close-path teardown ordering only) | live: resign-active/active protocol probe (owned delegate + CADisplayLink), scene disconnect/reconnect verified; true OS background suspension and multiwindow lifecycle not proven | untested beyond the one launch (no pause/resume/rotate) | untested (no visibility/hide lifecycle check; "no resize/visibility lifecycle check" explicitly noted) |
| Accessibility (AccessKit) | live: `macos-a11y` / `examples/a11y_probe.rs` drives one button + one static text through a real `AXUIElement` client (not a VoiceOver session); fixed two real defects (GenericContainer invisibility, missing tap action) same day, 2026-09-22 | compiles under `--features a11y` (`cross-typecheck`), not exercised | Linux AT-SPI adapter exists (`platforms/linux/accessibility.rs`) but carries a D-Bus stack and is "not exercised" — compile-only via the a11y feature | **absent**: "iOS publishes no accessibility tree at all" — this is why touch verification had to use pixel-hash oracles instead of a11y identifiers | untested/absent | untested/absent |
| vsync / frame pacing | live and measured: `macos-workload` probe found `desired_maximum_frame_latency: 1` halved frame rate under real work (20ms p50 = 50fps on a 100Hz panel); fixed to 2, recovered to full 10ms p50 panel rate (ADR-0029 addendum); bare frame pump measured 100.2 fps independently | untested | untested | untested | untested | untested |
| GPU surface recreation / device loss | host-tested only: automatic retry backoff for both mobile runners (`21af0752`) tested "against a scripted backend only, not a real device or emulator failure" | untested | untested | same as Android: unit/host-tested, not live | same as iOS | untested |
| Hot reload | live: `macos-hot-reload-loop` drives the CLI's actual event stream — cold build, hot edit, broken edit (no crash/no restart), fixed edit, state-preservation proof via a monotonic counter, idle no-op, SIGINT clean exit — this is the `WorkerHost` reload tier only | untested | untested | "the iOS worker path" is explicitly **not** driven by the hot-reload probe | untested/no web runner integration noted for other reasons | **absent**: "hot-reload has no web runner" |

## 2. Per-platform narrative status (per docs/BETA.md's own verdicts, branch-only doc)

BETA.md defines four states: **unverified**, **blocked**, **experimental**,
**beta verified**. As of 2026-09-22, **none is "beta verified."** Its own
table:

- **macOS (AppKit/Metal/ARM64) — beta candidate** (the strongest tier reached;
  not itself one of the four defined states, used to mean "furthest along").
  Live operator-equivalent input, native close/quit/reopen, launch-route
  rendering, IME routing (not real IME), a11y on one control. Gaps: no
  VoiceOver session, no physical Cmd+Q/menu routing, no nested modal loops,
  `SharedRealm` still refused, clipboard/suspend-resume unverified, one
  unreproduced first-run flake.
- **iOS Simulator (iPhone 16e, iOS 26.2) — experimental.** Touch + Home/return
  retention via XCUITest (no accessibility tree at all on this backend), safe
  area layout live-verified, scene disconnect/reconnect probed. No physical
  device, no IME/keyboard, no landscape, no full multiwindow/background-launch
  rendering.
- **Linux (X11/Wayland) — experimental.** Only CI-executed synthetic/scripted
  input (`live-smoke`, `live-smoke-wayland` jobs) — no human/operator-equivalent
  input like macOS has. No IME, no resident/background lifecycle, accessibility
  bridge not exercised. Wayland coverage is narrower than X11 (close-path
  teardown ordering only).
- **Windows (Win32) — unverified.** Only `just cross-typecheck`'s
  `cargo clippy --target x86_64-pc-windows-msvc --features a11y` on
  `flui-platform`. Zero live window/input/lifecycle/IME evidence for this
  candidate. Also carries an untraced Windows-only crash (ROADMAP-TRACKER item
  H9, `STATUS_HEAP_CORRUPTION`) that keeps `flui-platform`'s own test suite off
  any Windows CI runner — the workspace `test` job itself dropped
  `windows-latest` from its matrix entirely (comment in `ci.yml`: "day-to-day
  development happens on Windows locally... paying the wgpu/PDB build cost in
  CI is redundant for now").
- **Android (emulator, android-35 arm64) — experimental.** One emulator run,
  one host, debug APK, `adb input tap` (not a finger) advancing a counter 0→1→2.
  This only works after fixing a real bug (`bf2725be`): touch coordinates were
  device pixels, not the logical pixels the framework expects
  (`PointerState::position` contract) — every tap previously landed past the
  viewport edge. No lifecycle (pause/resume/rotate), no keyboard. Software
  rendering path (`-gpu swiftshader_indirect`) produced **no first frame in
  four minutes** — unverified/possibly broken. Debug APK is 406MB (uncompressed
  `.so` with full debug info) — a real distribution blocker, not measured here
  but noted.
- **Web/WASM — experimental.** One browser (Chromium-based pane), one machine,
  `localhost`, WebGPU only (no WebGL fallback). Real shader bug found and
  fixed: two WGSL shapes (branch-guarded `dpdx` in `clipAlpha`/arc gradient)
  compiled fine under naga but were rejected by every real browser ("Tint:
  'dpdx' must only be called from uniform control flow") — this is the kind
  of defect that "compiles for wasm32" (the CI `wasm-check` job) cannot catch;
  it needed an actual browser run. Fixed by making derivatives unconditional +
  `select`; guarded going forward by `scripts/check-wgsl-uniformity.py`, run in
  CI. Resize-to-viewport landed after a second bug (canvas pinned to
  `AppConfig::size`, ignoring CSS). No Firefox/Safari, no touch, no IME, no
  hot-reload runner.

## 3. CI coverage (`.github/workflows/ci.yml`)

- **`test` job (nextest, workspace):** `ubuntu-latest` **only**. Windows was
  explicitly dropped from the matrix (see comment: cost vs. redundancy
  argument, revisit "once the workspace stabilizes"); macOS "was never added."
  `flui-platform` is excluded from the default invocation and re-entered in a
  dedicated Linux-only step with `--all-features` (to reach the winit backend)
  under Xvfb + `FLUI_HEADLESS=1`, covering 171 tests (8 legitimately
  `#[ignore]`d).
- **`live-smoke` / `live-smoke-wayland`:** Linux only, Xvfb (X11) and headless
  weston (Wayland). Real XTEST pointer input against a real windowed demo;
  Wayland variant covers only close-path teardown ordering (issue #713 —
  wgpu swapchain destroyed after its `wl_surface`, a use-after-free that X11's
  Xlib tolerated but Wayland's client-side proxies did not).
- **`gpu-test` job:** `windows-latest`, but this runs the **GPU readback
  pixel-oracle suite** (~440 tests, `flui-engine --features testing`), not a
  Windows platform-integration test. GitHub's Windows runners have no hardware
  GPU, so the adapter selected is **WARP** (Microsoft's software DX12
  rasterizer) — chosen deliberately because the team's own dev machines are
  hardware DX12, so WARP and hardware DX12 share a backend and the ±LSB
  tolerances hold. This is rendering-correctness CI, not Windows-platform CI —
  it never opens a native window or touches `flui-platform`'s Win32 backend.
- **`cross-typecheck`:** the only job that even compiles the AppKit/Win32/
  Android/iOS backends beyond default features — `cargo clippy -p flui-platform
  --features a11y` for `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`,
  `aarch64-linux-android`, `aarch64-apple-ios`. No link, no test execution.
- **`miri`:** `ubuntu-latest`, scoped to specific modules
  (`pipeline::owner`, `owner::global_key`, `wgpu::surface_lease`,
  `cancelling_renderer_new`) — not a platform-backend check.
- **No Android emulator job, no wasm-execution-in-browser job, no macOS runner
  at all** exist in `ci.yml`. `wasm-check`/`wasm-test` compile/link/run under
  Node via `wasm-bindgen-test-runner` — i.e. no browser is involved even in the
  one wasm execution job (comment: "compiling for wasm32 is not running on
  wasm32... no browser and no wasm-pack involved" — issue #985 is what forced
  `wasm-test` to exist at all, because until then "every works-on-the-web claim
  rested on the linker succeeding").
- Net effect: **every live macOS/iOS/Android/Web finding cited above came from
  a human or local script run on a real Mac, not from CI.** CI's platform
  coverage is Linux (real, via Xvfb/weston) + Windows-as-WARP-for-rendering
  + cross-compile-only for everything else.

## 4. justfile platform recipes

Only macOS and one iOS recipe exist as first-class `just` targets; there is
**no** `android-*` or `web-*` group in the justfile itself (the Android/Web
work cited in BETA.md was driven by ad hoc Python scripts and the CLI, not
justfile recipes):

- `macos-close-path` — stages a bundled `.app`, asserts native close-callback
  ordering (issue #1148's AppKit half).
- `macos-frame-pump` — asserts the frame loop keeps re-arming past the primer
  frame (guards the AppKit "discards an in-pass `setNeedsDisplay:`" defect).
- `macos-resize-jitter` — pins `Renderer::warn_on_size_mismatch`; explicitly
  documented as **not** able to discriminate `desired_maximum_frame_latency`
  1 vs 2 (measured zero four times) despite being built for that.
- `macos-ime` — synthetic-key IME routing/protocol probe, not real IME.
- `macos-a11y`, `macos-lifecycle`, `macos-workload`, `macos-hot-reload-loop`,
  `macos-launch-render` — referenced in BETA.md text but some are Python
  scripts (`scripts/check-*.py`) rather than justfile recipes; verify before
  relying on `just <name>` working verbatim for all of them (the justfile
  grep found only `macos-close-path`, `macos-frame-pump`, `macos-resize-jitter`,
  `macos-ime`, `ios-sim` as actual recipes; the rest are invoked via
  `scripts/check-*.py` directly per BETA.md's own commands).
- `ios-sim` — static Material app render + animated-pixels-change-over-2s
  liveness check; re-run 2026-09-22, passed.
- `wasm-link-check`, `cross-typecheck`, `live-smoke`, `live-smoke-wayland` —
  the only recipes that touch non-macOS platforms at all.
- **No `android-*` recipe exists in the justfile** — the one Android run cited
  came from CLI (`flui build android` / `flui run --device`) work in a "peer
  session," not a reproducible `just` target.

## 5. Rendering engine (`flui-engine`)

- **wgpu**: workspace-pinned version (check `[workspace.dependencies].wgpu` in
  root `Cargo.toml` for the exact pin — not inlined here since `flui-engine`
  inherits it via `workspace = true`), with per-backend feature selection:
  `dx12` (Windows), `metal` (macOS/iOS), `vulkan` (Linux/Android), plus a
  fallback multi-backend feature set.
- **Text stack**: `cosmic-text 0.19` for shaping/layout (in `flui-painting`),
  `etagere 0.3` for atlas packing, own `glyph_atlas.rs`/`atlas.rs` in
  `flui-engine`. `unicode-segmentation` (pulled in transitively via
  cosmic-text) now drives grapheme-cluster-correct cursor movement/selection/
  masking in `TextEditingController` (fixed for ZWJ, regional-indicator flags,
  combining marks).
- **Tessellation**: `lyon 1.0.16` for path tessellation (`tessellator.rs`,
  `path_cache.rs`).
- **Shader graph**: `naga_oil` for shader composition/preprocessing
  (`shader_composer.rs`, `shaders/mod.rs`).
- **Effects implemented** (each has its own pipeline + generated shader +
  dedicated test module under `crates/flui-engine/src/`): blur
  (`blur/`, `blur_filter_tests.rs`), color matrix (`color_matrix/`), gamma
  (`gamma/`), morphology (`morphology/`, e.g. dilate/erode), advanced blend
  modes (`advanced_blend/`), generic blend mode (`mode/`), gradients
  (`effects/gradient.rs`, `batches/gradients.rs`), shadows
  (`effects/shadow.rs`). `flui-layer` exposes these as composable layers:
  `BackdropFilterLayer`, `ImageFilterLayer`, `ShaderMaskLayer`, `ColorFilter`,
  `ClipPath`/`ClipRect`/`ClipRRect`/`ClipSuperellipse`, `OpacityLayer`,
  `Leader`/`Follower` (for overlays/tooltips), `TextureLayer`,
  `PlatformViewLayer`. This is a substantial subset of Flutter's Skia/Impeller
  layer catalog already implemented in Rust/wgpu.
- **Known gaps vs Flutter/Impeller** (absence confirmed by repo search, not
  asserted from memory):
  - **No SVG rendering** anywhere in the workspace (`Cargo.toml` search for
    svg-related crates: none in any `flui-*` crate).
  - **No Lottie/vector-animation format support** (no lottie crate, no
    matching module).
  - **No video decoding/playback** (the only "video" hit in the whole
    workspace is a doc-comment phrase "Parallel image/video decoding" in
    `flui-assets/Cargo.toml` describing a *feature name's intent*, with no
    actual video decode implementation behind it).
  - Text stack is cosmic-text (CPU shaping) + wgpu-rasterized glyph atlas —
    functionally comparable to Flutter's SkParagraph/Impeller text but with a
    much younger ecosystem (fewer complex-script/BiDi edge cases proven).
  - GPU device-loss/surface-recreation recovery exists but is **only
    host-tested against a scripted mock backend** — never exercised against a
    genuine device/driver failure on any real GPU, mobile or desktop.
  - Frame pacing has one hard-won, well-documented finding: wgpu's
    `desired_maximum_frame_latency` must be 2, not 1, or real (non-trivial)
    frame workloads silently halve to half the display's refresh rate — this
    was is a "free choice" turned into a real, measured constraint (ADR-0029
    addendum), a good sign of engineering rigor but also a sign the
    engine's frame-pacing story needed live measurement to catch a 2x
    perf cliff that unit/GPU-readback tests could not have caught.

## 6. Assets (`flui-assets`)

- **Image decoding**: via the `image` crate (optional, `images` feature) —
  PNG/JPEG/GIF/WebP/etc. per the crate's own doc comment; not verified per-format
  in this pass, but the dependency covers the standard raster formats. Default
  feature set is `default = []`, i.e. **image decoding is opt-in** and not
  compiled/tested in the default `cargo test --workspace` CI run — a dedicated
  `test-features` CI job exists specifically because "flui-assets and
  flui-widgets both default to `default = []`, so the workspace-default run
  above never compiles their images/network decode-and-cache paths."
- **Fonts**: `flui-assets/src/assets/font.rs`, `types/font_data.rs` handle font
  asset loading; `flui-painting/assets/fonts/inventory.toml` is a provenance
  inventory pinning font bytes by SHA-256 (fonts: FLUI Probe Sans — a
  first-party generated fixture replacing a restricted Arial dependency,
  Roboto pinned to the official Android v2.138 archive, plus Cupertino/
  Material font sources at pinned revisions). No system-font enumeration/
  fallback-to-OS-fonts capability was found in this pass (cosmic-text can do
  this, but no FLUI-side code surfaced wiring it in this search — worth a
  follow-up read of `text_layout/font_resolve.rs` if system-font fallback
  matters for the audit).
- **No SVG asset support, no Lottie/JSON-animation asset support** — consistent
  with the engine-level absence above.
- **Network loading**: `loaders/network.rs` + `network` feature exists (also
  opt-in, also outside default CI coverage except via `test-features`).

## 7. Cited issue numbers — what was actually found

- **#1092** (winit key mapping): real, fixed. `platforms/winit/events.rs` now
  delegates the whole keyboard event to `ui-events-winit` (the same bridge
  Masonry/Xilem use) instead of a hand-picked `KeyCode`→`Location` table; a
  cross-backend regression test (`cross_backend_physical_key_agreement`)
  guards Win32/AppKit's still-hand-written tables against drifting from it.
- **#1147**: real, macOS-specific, referenced twice in
  `platforms/macos/window.rs` around wrapper-clone/window-map ownership on the
  owner thread; ROADMAP-TRACKER lists it as a still-open residual against the
  "Finish macOS backend" milestone.
- **#654**: referenced alongside #1147 in ROADMAP-TRACKER as an open residual
  for the same milestone; no further detail surfaced in this pass (worth a
  direct issue-tracker lookup, not found as inline code commentary beyond the
  P2 tracker row) — user's task brief calls it "wake delivery."
- **#185** (Windows resize jitter): **not found anywhere in the codebase** in
  this pass (no comment, no ADR, no test references it by number) — either
  it's tracked purely on GitHub with no code-side breadcrumb, or the number in
  the task brief doesn't match this repo's issue history. Cannot confirm status.
- **#1065** (Windows clipboard): **not found anywhere in the codebase** in this
  pass either — same caveat as #185.
- **#1046–#1051** (web): not found as a contiguous cited block; the actual web
  defects with evidence in this repo are the WGSL uniformity bug (fixed, dated
  2026-09-21) and the canvas-resize-ignoring-CSS bug (fixed, dated 2026-09-22),
  both documented in BETA.md's "Web: the counter in a browser" section, and
  issue #985 (cited directly) about wasm compiling-but-not-running being a
  historical blind spot that `wasm-test` was built to close.
- **#1185–#1189** (Android): #1185 and #1187 were found, both in
  `docs/adr/ADR-0063-the-renderer-owns-its-surface-target.md` — #1185 filed and
  closed (surface-target ownership), #1187 is Android joining a "census" of
  surface-registration call sites (2026-09-16 amendment). #1186, #1188, #1189
  not found in this pass.

Caveat on issue lookups above: this was a repo-text search (comments, ADRs,
tracker docs), not a GitHub API query — several cited numbers may exist only
as GitHub issues with no in-repo trace, which would explain the "not found"
results without meaning the issue doesn't exist.
