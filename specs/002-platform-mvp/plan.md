# Implementation Plan: flui-platform MVP Completion

**Branch**: `002-platform-mvp` | **Date**: 2026-02-13 | **Spec**: `specs/002-platform-mvp/spec.md`
**Input**: Feature specification from `/specs/002-platform-mvp/spec.md`

## Summary

Bring `flui-platform` to GPUI-level completeness by adding per-window callback architecture to PlatformWindow (~30 methods), expanding Platform trait (~35 methods), creating `Task<T>` async abstraction with priority scheduling, implementing DirectWrite text backend for Windows, and updating both Windows and Headless implementations. This enables flui-app to replace direct winit dependency and completes the platform abstraction layer as foundation for all upper framework layers.

## Technical Context

**Language/Version**: Rust 1.91 (workspace `rust-version`)
**Primary Dependencies**: windows 0.59 (Win32 API), ui-events (W3C events), keyboard-types 0.8, raw-window-handle 0.6, tokio 1.43, parking_lot 0.12, flume 0.11, thiserror, anyhow, tracing
**Storage**: N/A (platform abstraction, no persistence)
**Testing**: `cargo test -p flui-platform`, property-based tests with proptest for event conversion
**Target Platform**: Windows (primary, native Win32), Headless (testing/CI), macOS/Linux (stubs preserved)
**Project Type**: Rust workspace crate (`crates/flui-platform/`)
**Performance Goals**: 60fps rendering (16ms frame budget), <1ms event dispatch, on-demand rendering (ControlFlow::Wait)
**Constraints**: Zero `unsafe` outside platform FFI (constitution III), no polling loops (constitution Performance), no `unwrap()` in library code, all logging via `tracing`
**Scale/Scope**: ~30 PlatformWindow methods, ~35 Platform methods, ~15 new types, 2 platform implementations (Windows + Headless), ~4000 lines new/modified code

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Flutter as Reference, Not Copy | PASS | Adapting GPUI's callback pattern (Rust-native), not copying Flutter's widget layer |
| II. Strict Crate Dependency DAG | PASS | flui-platform is a foundation crate with only flui-types as internal dependency |
| III. Zero Unsafe in Widget/App Layer | PASS | `unsafe` only in flui-platform FFI (Win32 API calls) with `// SAFETY:` comments |
| IV. Composition Over Inheritance | PASS | Trait-based abstraction, `dyn` dispatch justified for platform type erasure |
| V. Declarative API, Imperative Internals | PASS | Public API is trait-based; internals use imperative Win32/mutex patterns |
| Rust Standards (no unwrap, tracing) | PASS | All error handling via thiserror/anyhow, logging via tracing |
| Testing (>= 70% platform coverage) | PENDING | Will be verified after implementation |
| Performance (on-demand rendering) | PASS | Event loop uses ControlFlow::Wait, render on dirty only |
| ID Offset Pattern | N/A | No slab-based IDs in platform layer |
| Platform rules (winit) | JUSTIFIED | Constitution says "winit is primary windowing abstraction" but also says "direct platform code for capabilities winit does not cover". Native Win32 provides per-window callbacks, fullscreen, Mica backdrop, DPI scaling that winit abstracts away. Decision: native Win32 for Windows, winit remains available as optional backend. |

### Violations Requiring Justification

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| Native Win32 instead of winit-only | Per-window callbacks, Mica backdrop, DPI v2, fullscreen hotkey, taskbar progress | winit doesn't expose per-window wndproc callbacks or Windows 11 DWM features |

## Project Structure

### Documentation (this feature)

```text
specs/002-platform-mvp/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output (trait signatures)
└── tasks.md             # Phase 2 output (via /speckit.tasks)
```

### Source Code (repository root)

```text
crates/flui-platform/
├── Cargo.toml                          # Dependencies (add: oneshot, cursor-icon)
├── src/
│   ├── lib.rs                          # Public API re-exports
│   ├── task.rs                         # NEW: Task<T>, Priority, TaskLabel
│   ├── cursor.rs                       # NEW: CursorStyle enum
│   ├── executor.rs                     # MODIFY: Return Task<T> instead of ()
│   ├── config.rs                       # Existing (minor additions)
│   ├── window.rs                       # MODIFY: Align Window trait with PlatformWindow
│   ├── traits/
│   │   ├── mod.rs                      # MODIFY: Re-export new types
│   │   ├── platform.rs                 # MODIFY: Expand Platform trait (~15 new methods)
│   │   ├── window.rs                   # MODIFY: Expand PlatformWindow (~25 new methods)
│   │   ├── display.rs                  # MODIFY: Add uuid(), default_bounds()
│   │   ├── capabilities.rs            # Existing (no changes)
│   │   ├── lifecycle.rs               # Existing (no changes)
│   │   ├── embedder.rs                # Existing (no changes)
│   │   └── input.rs                   # MODIFY: Add DispatchEventResult
│   ├── shared/
│   │   ├── mod.rs                     # Existing
│   │   └── handlers.rs               # MODIFY: Expand PlatformHandlers
│   ├── platforms/
│   │   ├── mod.rs                     # Existing
│   │   ├── windows/
│   │   │   ├── mod.rs                 # MODIFY: Re-export new types
│   │   │   ├── platform.rs           # MODIFY: Implement new Platform methods
│   │   │   ├── window.rs             # MODIFY: Implement callbacks + control methods
│   │   │   ├── display.rs            # MODIFY: Add uuid()
│   │   │   ├── clipboard.rs          # MODIFY: Rich ClipboardItem support
│   │   │   ├── events.rs             # MODIFY: Wire to per-window callbacks
│   │   │   ├── text_system.rs        # NEW: DirectWrite text backend
│   │   │   ├── util.rs               # Existing (minor additions)
│   │   │   └── window_ext.rs         # Existing (no changes)
│   │   └── headless/
│   │       ├── mod.rs                 # Existing
│   │       └── platform.rs           # MODIFY: Implement all new methods + injection
│   └── tests/                         # MODIFY: Add tests for all new functionality
```

**Structure Decision**: Single crate modification within existing workspace. No new crates needed. All changes are additive to existing trait surface with backward-compatible defaults where possible.
