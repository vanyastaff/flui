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

- **Tests excluded from CI** — STATUS_HEAP_CORRUPTION investigation in progress
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
