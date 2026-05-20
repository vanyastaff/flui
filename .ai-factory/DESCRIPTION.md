# FLUI

## Overview

**FLUI** is a modular, Flutter-inspired declarative UI framework for Rust. It targets high-performance GPU-accelerated rendering across desktop, mobile, and web while honoring native Rust idioms (ownership, traits + generics + enums over inheritance, no nullability, zero-cost abstractions). The framework is currently in the platform-integration phase: foundation layers are stable, and high-level rendering / view crates are being landed incrementally.

## Core Features

- **Three-tree architecture:** immutable `View` tree → mutable `Element` tree → layout/paint `Render` tree, with a strict pipeline (Build → Layout → Paint).
- **Type-safe arity system:** render children are parameterized by `Leaf`, `Single`, `Optional`, or `Variable`, eliminating a class of layout bugs at compile time.
- **GPU-first rendering:** `wgpu`-backed engine with `lyon` tessellation and `cosmic-text` / `glyphon` text rendering.
- **Cross-platform abstraction:** native Win32, AppKit, and headless backends behind the `Platform` trait, with a `winit` fallback under construction.
- **Hot-reload pipeline:** scene plugins via `dlopen` (`flui-hot-reload`) for desktop iteration without process restarts.
- **Async build pipeline:** `flui-build` defines a sealed `PlatformBuilder` trait with typestate context for Android, iOS, desktop, and web targets.
- **Accessibility & semantics:** dedicated `flui-semantics` crate aligning with platform accessibility APIs.
- **On-demand rendering:** event-driven loop with `ControlFlow::Wait`; frames are produced only when a tree is dirty (60 fps target when active).

## Tech Stack

- **Programming language:** Rust 1.94 (edition 2024).
- **Workspace:** Cargo workspace with 20+ crates organized in foundation / core / rendering / framework / application layers.
- **Async runtime:** `tokio` 1.43 (LTS), `futures`, `pin-project-lite`.
- **Synchronization:** `parking_lot` 0.12, `dashmap` 6.1, `crossbeam` 0.8, `flume` 0.11.
- **Collections:** `slab` (with the 1-based `NonZeroUsize` ID offset pattern), `smallvec` 1.13.
- **Graphics:** `wgpu` 25.x (pinned — 26.0 is broken upstream), `glam` 0.30, `lyon`, `glyphon`, `cosmic-text`, `guillotiere`.
- **Platform:** `winit` 0.30.12, `arboard` 3.4, `windows` 0.52 / 0.59 (Win32), `cocoa` 0.26.0 (AppKit), `ui-events` (W3C event model), `keyboard-types` 0.8, `raw-window-handle` 0.6.
- **Diagnostics:** `tracing`, `tracing-subscriber`, `tracing-forest`.
- **Errors:** `thiserror` for libraries, `anyhow` for application/glue code.
- **Macros / generics:** `ambassador` 0.4.2 for delegation, `bon` 3.8 for builders.
- **Testing:** `cargo test`, `criterion` for benchmarks, `cargo clippy --workspace -- -D warnings` and `cargo fmt --all` enforced.
- **CLI tooling:** `clap` 4.5, `cliclack` 0.3.6, `console` 0.15 in `flui-cli`.

## Architecture

See `.ai-factory/ARCHITECTURE.md` for detailed architecture guidelines.

**Pattern:** Layered Modular Workspace + Three-Tree Pipeline.

## Architecture Notes

- **Layered crate DAG:** dependencies flow strictly downward (foundation → core → rendering → framework → application). Cycles are forbidden.
- **Three-tree pipeline:**
  - `View` is configuration: immutable, single `build()` method per view.
  - `Element` holds mutable state and reconciliation logic; stored in a `Slab`, keyed by 1-based `NonZeroUsize` IDs.
  - `Render` performs layout (`Constraints` → `Size`) and paint (layer generation), parameterized by `Arity` for type-safe child management.
- **Pipeline phases:** `BuildPhase` (widget rebuilds via `BuildOwner`) → `LayoutPhase` (size computation via `RenderTree`) → `PaintPhase` (layer emission via `PaintContext`).
- **Render lifecycle:** `attach(owner)` → `mark_needs_layout` → `layout(constraints)` / `perform_layout()` → `paint(context)` → `detach()`.
- **Platform abstraction:** `Platform` trait with `WindowsPlatform`, `MacOSPlatform`, `HeadlessPlatform` implementations. Patterns include callback registries (`on_quit`, `on_window_event`), type erasure via `Box<dyn PlatformWindow>`, and interior mutability via `Arc<Mutex<T>>` / `parking_lot`.
- **Build system:** sealed `PlatformBuilder` trait + typestate `BuilderContextBuilder<P, Pr>` for compile-time validation across `AndroidBuilder`, `IosBuilder`, `DesktopBuilder`, `WebBuilder`.
- **Reference sources:** `.flutter/` (Flutter source as architecture reference) and `.gpui/` (GPUI as Rust platform-pattern reference). Both are studied, never copied — patterns are always adapted to FLUI's type-safe idioms (Arity system, Ambassador delegation, no nullability).
- **Speckit alignment:** all major work must align with `.specify/memory/constitution.md` (v2.2.0).

## Non-Functional Requirements

- **Logging:** always via `tracing`; subscriber configurable via env (`RUST_LOG=...`). `println!` / `dbg!` are forbidden in shipped code.
- **Error handling:** library APIs return typed errors via `thiserror`; `anyhow::Error` is allowed only in application/CLI/build glue. No `unwrap()` outside tests, examples, and documented `// SAFETY:` invariants.
- **Performance:** 60 fps target with on-demand rendering (`ControlFlow::Wait`); render only when dirty; long-running work runs on the platform background executor.
- **Memory safety:** `unsafe` is confined to `flui-platform`, `flui-painting`, and `flui-engine`. Widget and application layers must remain `unsafe`-free.
- **Concurrency:** sync primitives via `parking_lot`; channels via `flume`; sharded maps via `dashmap`. Public types document Send/Sync expectations.
- **Coverage targets** (constitutional): core ≥ 80 %, platform ≥ 70 %, widget ≥ 85 %.
- **Cross-platform support:** Windows (Win32), macOS (AppKit), Linux (winit), Android (NDK), iOS (planned), Web (wasm32 via `wasm-pack`).
- **Build hygiene:** `cargo clippy --workspace -- -D warnings` and `cargo fmt --all` are required to pass before merge.
- **Git hygiene:** Conventional Commits (`feat`, `fix`, `refactor`, `test`, `docs`, `chore`); no destructive git operations without explicit user permission.
