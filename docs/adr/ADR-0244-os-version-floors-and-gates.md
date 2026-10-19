# ADR-0244: The lowest floor the stack allows, and version gates that are types

- **Status:** Proposed
- **Date:** 2026-10-19
- **Supersedes:** [ADR-0236](ADR-0236-display-snapshot-and-refresh-period.md) decision 9 only: its
  typed checks become the single style below, are compared against a declared floor, and the floor
  is enforced at build and at start. Its Android refresh-rate behaviour (decision 9's API 30 hint)
  stays in force as a gate of this shape.
- **Related:** [ADR-0082](ADR-0082-platform-api-contract-crate.md) (where OS types live),
  [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md) (the runtime has no platform edge),
  [ADR-0097](ADR-0097-no-process-global-state-gate.md) (no process-wide version state).

## Context

Every environment FLUI runs on has versions, and nothing in the repository says which ones FLUI
supports. Where a minimum exists it was set in passing, and the values disagree:

- Android: `flui-cli` packages with `minSdk` 23 (`android_package.rs`), `xtask doctor` accepts a
  compiler for API 21, and the backend renders only through wgpu's Vulkan backend, which Android
  provides from API 24. An application installs on an API 23 device and has nothing to draw with.
- iOS: the bundle's `MinimumOSVersion` is copied from the Mach-O `minos`, which is whatever deployment
  target the build environment set (rustc's default is 10.0), while the backend needs `UIScene`
  (iOS 13). The only executed runs are on 26.2.
- macOS: no deployment target and no version read; `window_tiling.rs` carries a `TODO` for a 15.0
  check, and `window_ext.rs` documents 14.0 and 26.0 requirements nothing checks.
- Windows: `WindowsBuild` is read with `RtlGetVersion` and compared only with feature thresholds;
  `SetProcessDpiAwarenessContext` (Windows 10 1607) is linked unconditionally, so the real floor is
  whatever the import table says, not a decision.
- Linux: the release archives' glibc floor is the release runner's (2.39 on `ubuntu-24.04`); no
  Vulkan, Wayland or X11 requirement is written down.
- Web: WebGPU only (`gles` is enabled without `webgl`), with no statement that a browser without
  WebGPU is unsupported.

The stack's own minimums are low. rustc 1.99 supports Windows 10, macOS 11.0 on arm64 and 10.12 on
x86_64, iOS 10.0, every API level the current NDK supports (21), and glibc 2.17. wgpu's DX12
backend needs Windows 10 (WDDM 2.0), its Metal backend macOS 10.13. `windows`, `objc2`,
`wayland-client` and `x11rb` set no floor of their own; the floor comes from what FLUI calls through
them and from the GPU backend it enabled per environment.

Code that uses a newer API has four styles today: a proof type (`WindowsBuild::virtual_mode_aware`,
Apple `OsVersion::since`, Android `NdkFn`, Web `WebFeature`), bare comparisons
(`android_text_weight.rs`: `sdk < 31`; `android_scroll.rs`: `sdk >= 26`), objc2's `available!`
(`ios/status_bar.rs`: `ios = 13.0`), and nothing at all.

## Decision

1. **The floor is the lowest version the stack can run on.** It is not chosen by market share. Per
   environment it is the highest of: the Rust target's minimum, the minimum of the GPU backend FLUI
   enables there, and the minimum of each API the backend cannot work without at all (a window, a
   surface, input, a frame callback). Every other API newer than that is gated (decision 4), never a
   reason to raise the floor. Applied to the current stack:

   | Environment | Floor | Set by |
   |---|---|---|
   | Windows | 10 1507 (build 10240) | rustc target and D3D12; per-monitor DPI v2 (1703) and later APIs are gates |
   | macOS | 10.13 on x86_64, 11.0 on arm64 | wgpu Metal on x86_64; 11.0 is the first arm64 release |
   | iOS | 13.0 | `UIScene`/`UIWindowScene`, through which the backend gets its window |
   | Android | API 23 | the backend's own NDK use (`__system_property_get`, `AndroidApi::detect`); Vulkan or GLES 3.0 |
   | Linux | glibc 2.17, a Vulkan 1.0 or GLES 3.0 driver, Wayland `xdg_wm_base` 1 or X11 with RandR 1.5 | rustc target, wgpu, the window and display protocols the backend binds |
   | Web | a browser with WebGPU or WebGL2 | detected, never a browser version |

   **The GPU API is a probe, not a floor.** Whether a device has a working Vulkan driver, or a browser
   WebGPU, depends on the device, not on the OS version, so it is decided as in decision 5: the
   renderer asks for the preferred backend (Vulkan on Android and Linux, WebGPU on the Web) and falls
   back to GLES 3.0 (WebGL2 on the Web) when no adapter answers. The backends are tried one at a time,
   each with an instance of its own for that one backend, because a combined backend mask does not
   retry: on the Web, wgpu commits to its WebGPU context as soon as `navigator.gpu` exists, so a
   browser that exposes it but returns no adapter (a blocklisted GPU) reaches WebGL2 only through a
   second, GL-only instance. The engine therefore needs nothing
   the downlevel backend lacks: features stay optional, limits come from the adapter, and no shader
   reads a storage buffer in a fragment stage (gradient stops move to a uniform array or a texture).
   Every pixel suite runs on both backends; on Linux, Mesa's llvmpipe gives CI a GLES adapter.
   The fallback is held to that proof: if the GLES path cannot pass the pixel suites at a cost in
   proportion to the devices it serves, it is removed and the floors rise to what the preferred
   backend needs (Android API 24 for Vulkan, a WebGPU browser), by decision 9.

2. **Declared once.** `platform-support.toml` at the repository root holds, per environment, the floor
   and what sets it (decision 1's last column). It is the only place a floor number is written by
   hand. `flui-platform` and `flui-cli` are published crates whose package archives cannot read a
   repository-root file, so `cargo xtask platform-floor --write` generates a committed Rust source in
   each (the floor constant of each version type; the packaging values: Android `minSdk`, the iOS and
   macOS deployment targets passed to rustc, the Windows manifest's `supportedOS`), and the check mode
   fails when a generated file differs from the root. `xtask doctor` reads the root file for
   toolchain checks; the Linux release build targets the floor glibc rather than the runner's.

3. **Below the floor, FLUI refuses to start, loudly.** Where the system honours the packaged
   minimum, the refusal is the system's, before any FLUI code runs: Android will not install an APK
   below `minSdk`, iOS will not install below `MinimumOSVersion`, macOS will not launch a binary whose
   `minos` or `LSMinimumSystemVersion` is newer, and the dynamic loader rejects a Linux binary whose
   glibc symbols are newer. Everywhere the executable does start (Windows, whose manifest does not
   block a launch; the Linux driver and protocol parts of the floor; the Web), the built-in backend
   reads its version once, when the platform is built, and compares it with the floor before anything
   else; below it the constructor returns
   `PlatformError::UnsupportedOs { environment, found, floor }`, and the app runner prints it and exits
   non-zero. A version that cannot be read is not below the floor: it starts and unlocks no gate. On
   the Web, where a page has no exit status, a browser with neither a WebGPU nor a WebGL2 adapter gets
   the same error returned from the web runner instead of a panic: it is written to the console, the
   runner replaces the app's host element with a message naming what is missing, and no frame is
   scheduled.

4. **Above the floor, a gate is a type.** One shape for every environment, in
   `flui-platform::shared::os_version`:
   - each environment has one version type, read once and kept in the platform instance, never in a
     `static`: `WindowsBuild`, `AppleVersion` (macOS and iOS, separate floors), `AndroidApi`. A
     negotiated standard has its own version domain of the same shape: a Wayland interface's bound
     version, kept on the object that bound `min(advertised, ours)`;
   - a feature newer than the floor is a marker type with `const SINCE` and a doc line saying what the
     code does without it, declared next to the code that uses it;
   - `version.since::<F>()` returns `Option<Since<F>>`, and the function that calls the newer API takes
     the `Since<F>` (or is reached only through a handle built from it). The number is private to the
     version type: no `level()`, `sdk_int()` or build accessor outside it, so `if sdk >= 30` cannot be
     written;
   - `since` asserts `F::SINCE > FLOOR` in a `const` block: a gate the floor already covers does not
     compile, so an API at or under the floor is called directly and a floor rise turns the gates it
     makes dead into compile errors;
   - an unknown version unlocks nothing;
   - a function above the floor is never linked: Android resolves it with `dlsym` (ADR-0236's `NdkFn`),
     Windows with `GetProcAddress`, so the binary loads on the floor; an Apple selector above the
     deployment target is sent through `msg_send!` behind the proof.

   `available!`, `respondsToSelector:` used as a version stand-in, and comparing a version with a
   literal are rejected by `clippy.toml` (`disallowed-macros`, `disallowed-methods`) outside the
   version module.

5. **What varies by device is a probe, not a version.** A capability the version does not decide (a
   `respondsToSelector:` that depends on the input device, a Wayland global the compositor may not
   advertise, a browser function, a GPU feature wgpu reports) is detected and kept as a proof of the
   same shape (`WebFeature` is the model). A version is never used to guess hardware or compositor
   support, and a browser version is never read.

6. **The markers are the version map.** `cargo xtask platform-floor --write` generates
   the version-map page under `docs/` from `platform-support.toml` and the markers: per environment, the floor
   and what sets it, then each version that adds something, the markers it unlocks and what FLUI does
   below it. What differs from version to version is read from that page, and it cannot drift from the
   code, because the check mode fails when the committed page differs from the generated one.

7. **Three outcomes, kept apart.** For a feature on a supported version:
   - the version has it and FLUI calls it: works;
   - the version predates the marker's `SINCE`: the marker's documented degradation runs and the
     capability reports itself unavailable; this is not a defect;
   - the version has it and FLUI has not wired it: our gap, a `docs/gaps.md` row, and the scenario that
     covers it fails as an expected failure naming the gap, never a skip.
   A version below the floor is none of these: it is unsupported (decision 3).

8. **Verified, not assumed.** `cargo xtask platform-floor` runs in `checks`: every packaging site and
   template agrees with `platform-support.toml`; the built artifacts carry the floor (Mach-O `minos`,
   the APK manifest's `minSdk`, the highest `GLIBC_` symbol version of the Linux ELF, no Windows import
   newer than the floor build); no source outside the version modules compares a version; the version
   map is current. Each marker has a table row that builds the version at `SINCE` minus one and at
   `SINCE`. `docs/platform-parity.md` gets a "Version floor" row per environment, ✅ only after an
   executed run on the floor version.

9. **A floor moves only when the stack moves.** It rises when a dependency drops a version, when an
   essential API moves (decision 1), or when the floor can no longer be run for the proof of decision 8;
   the change edits `platform-support.toml`, records the reason there, adds a `changelog.d` entry under
   `### Changed`, and deletes the gates decision 4 now rejects.

## Consequences

- `flui-engine` enables `gles` on Android and Linux and `webgl` on the Web, and `select_backend`
  returns the preferred backend with GL behind it; today the Web branch already names `GL` without
  the `webgl` feature, so its "WebGL fallback" is never compiled. On GL wgpu ignores
  `desired_maximum_frame_latency`, so the clock-side in-flight bound is the only one there.
- The linear, radial and sweep gradient shaders stop reading their stops from a storage buffer, which
  GLES 3.0, WebGL2 and many GLES 3.1 drivers do not offer in a fragment shader.
- The Android `minSdk` stays 23 and becomes a decision instead of a template literal; an API 23 device
  without Vulkan renders through GLES instead of failing. `doctor`'s API 21 check follows the file.
- iOS builds target 13.0 and macOS builds 10.13 / 11.0 explicitly instead of inheriting the build
  environment's deployment target.
- Windows' DPI-awareness call moves behind a 1703 gate with per-monitor v1 below it, and every other
  import newer than 10240 is resolved at run time.
- The Linux release build must link against glibc 2.17 (a toolchain that targets a glibc version, not
  the runner's), which the release workflow does not do today.
- `android_text_weight.rs`, `android_scroll.rs` and the JNI field checks move to markers;
  `AndroidApi::level` and `sdk_int` become private. `available!(ios = 13.0)` in `ios/status_bar.rs`
  is deleted: the floor covers it. macOS gains the version read it lacks, which unblocks the
  window-tiling `TODO`.
- `WindowsBuild`, Apple `OsVersion` and `AndroidApi` move under one module sharing `Since<F>`; the
  version types stay per environment because their shapes differ.
- `PlatformError::UnsupportedOs` is a new public error, returned by FLUI's built-in backends. An
  external backend has no seam to return it through today (`Platform::run` takes an `OwnerPlatform`
  outside code cannot mint); that seam is its own decision.
- A low floor costs proof: each floor needs a machine, image, emulator or simulator to run on, and
  until that run is recorded the parity row stays 🟡.
- The code changes land as separate PRs after this ADR is accepted: the file and its gate first, then
  the per-backend migrations.

## Alternatives rejected

- **A floor chosen by market share (for example the three newest Apple releases).** It refuses users
  the stack could serve, and the number has to be argued again every year; the gate cost of a low
  floor is paid once per feature.
- **No floor, feature detection only.** Nothing tells a user which systems are supported, and an old
  system fails somewhere in the middle of a frame instead of at start.
- **Floors as Rust constants in `flui-platform`.** `flui-cli`'s templates, `doctor` and the release
  workflow need the same numbers outside the crate graph; a constant per consumer drifts, as `minSdk`
  23 and API 21 already have.
- **objc2's `available!` on Apple and `if api >= N` on Android.** A boolean is easy to forget, cannot
  be faked in a test without the real OS, and does not fail when a floor rise makes it dead.
- **A hand-written per-version table.** It drifts from the code the day a marker is added; the map is
  generated from the markers instead.
- **Raise Android to API 24 for Vulkan and keep Vulkan the only backend.** It refuses devices the
  stack can serve through GLES, and Vulkan driver presence is a device property, not a version one.
- **Gate on browser versions.** Features ship behind flags, in secure contexts only, or are withdrawn;
  the version answers none of that.
