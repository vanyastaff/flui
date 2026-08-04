# AGENTS.md — flui-platform

Platform abstraction layer. Provides a unified `Platform` trait with concrete implementations.

## What lives here

- `Platform` trait — central abstraction (lifecycle, windows, displays, executors, clipboard)
- `current_platform()` — selects the right backend for the current environment
- `HeadlessPlatform` — mock for CI/testing (no display, no GPU, no OS windowing)
- Platform implementations: `winit/` (cross-platform), `headless/` (testing)
- Native backends: Win32 (windows crate), AppKit (cocoa/objc), Android (android-activity)
- WASM backend: web-sys bindings

## Key constraints

- **CI runs this crate's Linux-runnable suite, not a plain `cargo nextest run -p
  flui-platform`.** The exact command (mirrored by `just test-ci`):

  ```bash
  FLUI_HEADLESS=1 xvfb-run -a cargo nextest run -p flui-platform --all-features --no-fail-fast
  ```

  `--all-features` is required just to compile the `winit/` module (see the
  next bullet). `FLUI_HEADLESS=1` makes `current_platform()` return the
  `HeadlessPlatform` mock, which is what most tests call through and what
  fixes tests that otherwise call `open_window` outside `Platform::run`'s
  `on_ready` callback (a real ordering requirement of the winit event-loop
  model — calling it earlier panics on every backend, not just headless).
  `xvfb-run` covers the remainder: a handful of winit-internals unit tests in
  `src/platforms/winit/platform.rs` construct `WinitPlatform::new()` directly,
  bypassing the `FLUI_HEADLESS` check, and need a real (if virtual) X11
  connection for `arboard`'s clipboard init. Without both devices, running on
  a box with no display fails fast on the clipboard connection; running on a
  box with a real desktop session instead fails on the `open_window`
  ordering panics — neither failure mode means the crate is broken, both are
  fixed by running it the way CI does.
- **Windows and macOS backends still have zero executing tests anywhere** —
  `STATUS_HEAP_CORRUPTION` (ROADMAP-TRACKER item H9) is a Windows-only crash
  that can't reproduce on the Linux CI runners above; `cross-typecheck` lints
  those backends (clippy, no link, no run) as the only coverage they get
  until someone debugs H9 on an actual Windows box.
- The `winit/` module (including its owner-lane tests) only compiles under the
  `winit-backend` feature, not `desktop` (default) — a bare
  `cargo nextest run -p flui-platform` silently skips all of it; use
  `cargo nextest run -p flui-platform --all-features` (or `--features winit-backend`)
  to actually build and run it
- `desktop` feature (default) enables `winit`; `web` feature for WASM
- Native async deps (tokio) are `cfg(not(target_arch = "wasm32"))` only
- `raw-window-handle` 0.6 for window handle abstraction
- Platform-init stubs for linux/ios/android are exempt from port-check trigger #8 (`todo!()` allowed)

## Features

- `desktop` (default) — winit backend
- `winit-backend` — primary Linux backend until native Wayland/X11 lands;
  optional fallback on Windows/macOS
- `web` — WASM platform
- `wayland` / `x11` — Linux display server protocols
