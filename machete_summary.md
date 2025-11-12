# Cargo Machete Cleanup Summary

## What cargo machete --fix removed:

### Root Cargo.toml
- ✅ Removed unused workspace deps: `glam`, `glyphon`, `cosmic-text`, `guillotiere`, `bytemuck`
- ✅ Removed unused root deps: `parking_lot`, `pollster`, `env_logger`, `winit`, `tokio`, `serde`
- ✅ Removed unused profiling deps: `puffin`, `tracing-tracy`, `puffin_http`
- ✅ Updated rust-version from 1.90 → 1.91
- ✅ Updated pollster version 0.3 → 0.4
- ✅ Improved wgpu version documentation (25.x rationale)

### flui_animation/Cargo.toml
- ✅ Removed unused: `tracing`

### flui_app/Cargo.toml
- ✅ Removed unused: `wgpu` (NOT used in code)
- ✅ Added missing features: desktop, mobile, android, ios, web, images, pretty-logs
- ✅ Fixed Android dependencies (proper NDK + JNI setup)

### flui_assets/Cargo.toml
- ✅ Removed unused: `futures`, `triomphe`, `ahash`, `anyhow`, `ttf-parser`, `tracing`
- ✅ Commented out features with missing deps: `hot-reload`, `mmap-fonts`, `parallel-decode`

### flui_core/Cargo.toml
- ✅ Removed unused: `flui_derive`, `tracing-subscriber`, `tracing-forest`, `smallvec`, `downcast-rs`, `ahash`
- ✅ Removed `puffin` (commented out profiling feature)
- ✅ Added `flui_log` dependency
- ✅ Added `pretty-logs` feature

### flui_derive/Cargo.toml
- ✅ Removed unused: `proc-macro2`, `darling`

### flui_devtools/Cargo.toml
- ✅ Removed unused: `flui_types`, `tokio`, `tokio-tungstenite`, `dhat`, `tracing`, `tracing-subscriber`, `dashmap`
- ✅ Commented out broken features: `network-monitor`, `memory-profiler`, `remote-debug`, `tracing-support`

### flui_engine/Cargo.toml
- ✅ Added back engine-specific deps that machete removed but are needed for features:
  - `guillotiere = "0.6.2"` (for atlas-checks feature)
  - `cosmic-text = "0.15.0"` (for shape-run-cache feature)
- ✅ Commented out `memory-profiler` feature (depends on broken flui_devtools feature)

### flui_gestures/Cargo.toml
- ✅ Removed unused: `tracing`

### flui_interaction/Cargo.toml
- ✅ Removed unused: `ahash`

### flui_painting/Cargo.toml
- ✅ Removed unused: `glam` (Math ops use Matrix4 from flui_types)

### flui_rendering/Cargo.toml
- ✅ Removed unused: `downcast-rs`, `bitflags`
- ✅ Added missing: `serde` (optional, for serde feature)

### flui_types/Cargo.toml
- ✅ Fixed `full` feature: `tracing` → `log`
- ✅ Fixed `pretty_log` feature: `log/pretty` → `flui_log/pretty`

### flui_widgets/Cargo.toml
- ✅ Removed unused: `flui_engine`, `dyn-clone`, `once_cell`

## Compilation Status

✅ **Workspace compiles successfully** with only warnings (unused variables, dead code, missing docs)

## Key Insights

1. **cargo machete is aggressive** - It removes dependencies even if they're used by optional features
2. **Always verify feature definitions** after running machete --fix
3. **Check actual code usage** with grep, not just Cargo.toml declarations
4. **Engine-specific libraries** (glam, glyphon, lyon, guillotiere, cosmic-text) correctly kept in flui_engine only

## Dependencies That Were Correctly Removed

- ❌ glam from flui_painting (unused in code, Matrix4 from flui_types is sufficient)
- ❌ wgpu from flui_app (unused in code, only needed in flui_engine)
- ❌ egui dependencies (removed earlier in session)

## Dependencies That Needed to be Added Back

- ✅ guillotiere in flui_engine (needed for atlas-checks feature)
- ✅ cosmic-text in flui_engine (needed for shape-run-cache feature)
- ✅ serde in flui_rendering (needed for serde feature)

## Final Result

**Before:** Many unused dependencies, bloated dependency tree
**After:** Clean, minimal dependencies with proper feature flags
