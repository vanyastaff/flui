# FLUI DevTools - Feature Overview

## ✅ Implemented Features

### 1. 🎯 Performance Profiler
**Status**: ✅ Complete  
**File**: `src/profiler.rs` (614 lines)

**Capabilities**:
- Frame-level performance tracking
- Phase profiling (Build, Layout, Paint, Custom)
- RAII guards for automatic timing (`PhaseGuard`)
- Jank detection based on configurable threshold
- FPS calculation
- Frame history (ring buffer)
- Thread-safe with `Arc<Mutex<>>`

**API**:
```rust
let profiler = Profiler::new();
profiler.begin_frame();
{
    let _guard = profiler.profile_phase(FramePhase::Build);
    // work...
}
profiler.end_frame();
let stats = profiler.frame_stats();
```

**Tests**: 7 tests covering basic usage, jank detection, FPS, thread safety

---

### 2. 🔎 Inspector Counters
**Status**: ✅ Complete — but not a tree inspector (see below)
**File**: `src/inspector.rs` (181 lines)
**Feature Flag**: `inspector`

This is a counting/logging [`TreeObserver`] over the ADR-0040 observation
seam, not a widget-tree inspector. It has no access to the widget, element,
or render trees — only `flui-foundation` on its dependency list (dependency
inversion) — and cannot select, highlight, or walk widgets.

**Capabilities**:
- Tallies element mounts, moves, rebuilds (per [`RebuildReason`](https://docs.rs/flui-foundation) cause), and unmounts as a running realm mutates
- Point-in-time [`InspectorSnapshot`] with per-counter monotonicity (no cross-counter consistency guarantee)
- Detects stream end (`detached()`) so a snapshot can report whether the observation stream is final
- Thread-safe via private atomics — no lock in the public surface (SP-6)

**API**:
```rust
use flui_devtools::inspector::InspectorCounters;

let counters = InspectorCounters::new();
// Install via WidgetsBinding::install_tree_observer(Arc::new(counters.clone()))
// or hold `&counters` as a `&dyn TreeObserver` for direct dispatch.
let snapshot = counters.snapshot();
println!("mounts: {}, rebuilds: {}", snapshot.mounts, snapshot.rebuilds);
```

**Tests**: 1 test covering mount/move/rebuild/unmount tallying, per-reason
counts, and the detached flag.

**What this is NOT**: there is no `WidgetTreeNode`, no `select_widget`,
`get_widget_tree`, `highlight_widget`, or `find_widgets_by_type` — those
APIs never existed. Pull-shaped inspection (walking a live tree) waits on a
future seam; see `src/lib.rs`'s crate-level docs for the current boundary.

---

### 3. ⏱️ Timeline View
**Status**: ✅ Complete  
**File**: `src/timeline.rs` (619 lines)

**Capabilities**:
- Timeline event recording
- Category-based organization (Frame, Build, Layout, Paint, Custom)
- Nested event support
- Chrome DevTools trace export (chrome://tracing)
- Thread-aware tracking
- Event filtering by category/range
- Capacity limits to prevent memory bloat

**API**:
```rust
let timeline = Timeline::new();
{
    let _guard = timeline.record_event("Build", EventCategory::Build);
    // work...
}
let json = timeline.export_chrome_trace();
```

**Chrome Trace Format**: Compatible with `chrome://tracing` for visualization

**Tests**: 11 tests for recording, categories, exports, thread safety

---

### 4. 🔥 Hot Reload
**Status**: ✅ Complete  
**File**: `src/hot_reload.rs` (202 lines)  
**Feature Flag**: `hot-reload`

**Capabilities**:
- Cross-platform file watching (using `notify` crate)
- Debounced change events (default 500ms)
- Callback-based change notifications
- Async (non-blocking) and blocking modes
- Multiple path watching
- RAII `WatchHandle` for automatic cleanup

**API**:
```rust
let mut reloader = HotReloader::new();
reloader.watch("./src")?;
reloader.on_change(|path| {
    println!("Changed: {:?}", path);
});
let _handle = reloader.watch_async();
```

**Tests**: 4 tests for creation, watching, callbacks, stop

---

## 📊 Statistics

| Module | Lines of Code | Tests | Status |
|--------|--------------|-------|--------|
| **common.rs** | 91 | - | ✅ |
| **profiler.rs** | 614 | 7 | ✅ |
| **inspector.rs** | 181 | 1 | ✅ |
| **timeline.rs** | 619 | 11 | ✅ |
| **hot_reload.rs** | 202 | 4 | ✅ |
| **lib.rs** | 139 | 1 | ✅ |
| **Total** | **1,846 LOC** | **24 tests** | ✅ |

Counted directly from `src/*.rs` (`wc -l`, `rg -c '#\[test\]'`) — re-run
those if this table drifts again; nothing enforces it mechanically.

---

## 🚫 Not Implemented

Earlier revisions of this document (and the crate README) advertised a
network monitor, memory profiler, remote-debug server, and a
`tracing-support` feature. **None of these were ever implemented** — there
is no corresponding module, no `Cargo.toml` feature flag, and no such
capability behind `full`. `src/lib.rs`'s crate-level docs are the current
source of truth for what exists; treat any capability not listed there as
fictional until it has a module and a feature flag to match.

---

## 🎨 Design Principles

1. **Minimal Overhead**: DevTools should not significantly impact app performance
   - RAII guards for automatic cleanup
   - Lock-free where possible
   - Bounded memory (ring buffers)

2. **Thread Safety**: All APIs are thread-safe
   - `Arc<Mutex<>>` or `Arc<RwLock<>>` for shared state
   - No data races

3. **Feature Gated**: Optional features don't bloat the binary
   - Default features: none (`default = []`) — a release build with no
     features enabled compiles zero devtools code, opens no port, and runs
     no background work
   - Opt in per-feature: `profiling`, `timeline`, `hot-reload`, `inspector`;
     `full` enables all four

4. **Ergonomic API**: Easy to use, hard to misuse
   - RAII guards (PhaseGuard, EventGuard, WatchHandle)
   - Sensible defaults
   - Clear error messages

5. **Standards Compatible**: Export formats compatible with industry tools
   - Chrome DevTools trace format
   - JSON exports

---

## 📦 Dependencies

Read straight from `Cargo.toml` — no fictional entries.

### Unconditional
- `web-time` - Cross-platform timing (the maintained replacement for `instant`)
- `serde`, `serde_json` - Serialization (devtools protocol payloads)
- `parking_lot` - Fast locks
- `windows-sys` (Windows only) - process/memory info

### Feature-gated
- `flui-hot-reload` (feature `hot-reload`, via its `source-watch` feature) - underlying file watcher (`SourceWatcher`)
- `flui-foundation` + `tracing` (feature `inspector`) - `TreeObserver`/`RebuildReason` types and debug-level event logging

There is no `flui_core` (that crate has never existed in this workspace —
see `docs/crates.md`), no `dashmap`, and no `dhat`/`tokio-tungstenite`
pulled in for features that were never built.

---

## 🧪 Testing

All modules have comprehensive test coverage:
- Unit tests for core functionality
- Thread safety tests
- Integration tests (where applicable)
- Doctest examples

Run tests:
```bash
cargo test -p flui-devtools
cargo test -p flui-devtools --all-features
```

---

## 📚 Documentation

- **README.md**: Quick start guide
- **FEATURES.md**: This file - detailed feature overview
- **API docs**: `cargo doc --open -p flui_devtools`
- **Examples**: `examples/` directory

---

## 🎯 Comparison with Flutter DevTools

| Feature | Flutter DevTools | FLUI DevTools | Status |
|---------|-----------------|---------------|--------|
| Performance Profiler | ✅ | ✅ | Complete |
| Widget Inspector (tree walk/select/highlight) | ✅ | ❌ | Not implemented |
| Inspector counters (mount/rebuild/unmount tallies) | — | ✅ | Complete |
| Timeline View | ✅ | ✅ | Complete |
| Memory Profiler | ✅ | ❌ | Not implemented, not planned |
| Network Monitor | ✅ | ❌ | Not implemented, not planned |
| Debugger | ✅ | ❌ | Not implemented, not planned |
| Logging | ✅ | ❌ | Not implemented, not planned |
| Hot Reload | ✅ | ✅ | Complete (file-watch only; no state preservation) |

---

## 🔥 Hot Reload Comparison

| Framework | Hot Reload | State Preservation |
|-----------|------------|-------------------|
| Flutter | ✅ Instant | ✅ Automatic |
| React (Fast Refresh) | ✅ Fast | ✅ Automatic |
| FLUI DevTools | ✅ File-based | ⏳ Manual (TODO) |

---

## 💡 Usage Examples

See `examples/` directory:
- `profiler_demo.rs` - Frame profiling (requires `--features profiling`)

That is the only example in the crate today. There is no
`inspector_demo.rs`, `timeline_demo.rs`, or `hot_reload_demo.rs`.

---

Last verified against `src/*.rs` and `Cargo.toml`: 2026-08-04.
