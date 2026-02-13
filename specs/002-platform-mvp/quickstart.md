# Quickstart: flui-platform MVP Completion

**Branch**: `002-platform-mvp` | **Date**: 2026-02-13

## Prerequisites

- Rust 1.91+ (workspace `rust-version`)
- Windows 10+ (for DirectWrite and Win32 development)
- `cargo` with workspace support

## Build & Test

```bash
# Check the platform crate compiles
cargo check -p flui-platform

# Run all platform tests
cargo test -p flui-platform

# Run with logging
RUST_LOG=debug cargo test -p flui-platform -- --nocapture

# Clippy lint check
cargo clippy -p flui-platform -- -D warnings

# Full workspace check (ensure no regressions)
cargo check --workspace
cargo test --workspace
```

## Implementation Order

### Phase 1: PlatformWindow Callbacks + Control (FOUNDATION)

**Files to modify:**
1. `crates/flui-platform/src/traits/window.rs` — Add 9 callback + 10 control + 12 query methods to PlatformWindow trait
2. `crates/flui-platform/src/traits/input.rs` — Add `DispatchEventResult` type
3. `crates/flui-platform/src/traits/platform.rs` — Add `WindowAppearance`, `WindowBackgroundAppearance`, `WindowBounds` types
4. `crates/flui-platform/src/platforms/windows/window.rs` — Implement callbacks via WindowCallbacks struct with Mutex take/restore
5. `crates/flui-platform/src/platforms/windows/platform.rs` — Wire WndProc messages to per-window callbacks
6. `crates/flui-platform/src/platforms/headless/platform.rs` — Mock callbacks + event injection

**Test first:**
```rust
#[test]
fn test_window_callback_on_resize() {
    let platform = HeadlessPlatform::new();
    let window = platform.open_window(WindowOptions::default()).unwrap();
    let resized = Arc::new(AtomicBool::new(false));
    let flag = resized.clone();
    window.on_resize(Box::new(move |_size, _scale| { flag.store(true, Ordering::SeqCst); }));
    // Inject resize event on headless
    window.as_test().unwrap().simulate_resize(Size::new(800.0, 600.0));
    assert!(resized.load(Ordering::SeqCst));
}
```

### Phase 2: Platform Trait Expansion

**Files to modify:**
1. `crates/flui-platform/src/traits/platform.rs` — Add ~16 new methods with defaults
2. `crates/flui-platform/src/cursor.rs` — NEW: CursorStyle enum
3. `crates/flui-platform/src/platforms/windows/platform.rs` — Implement: activate, hide, set_cursor_style, window_appearance, open_url, prompt_for_paths, keyboard_layout
4. `crates/flui-platform/src/platforms/headless/platform.rs` — Mock all new methods

### Phase 3: Task<T> and Executors

**Files to modify:**
1. `crates/flui-platform/src/task.rs` — NEW: Task<T>, Priority
2. `crates/flui-platform/src/executor.rs` — Return Task<T> from spawn methods
3. `crates/flui-platform/src/lib.rs` — Re-export Task, Priority

### Phase 4: DirectWrite Text Backend

**Files to modify:**
1. `crates/flui-platform/src/platforms/windows/text_system.rs` — NEW: DirectWriteTextSystem
2. `crates/flui-platform/src/traits/platform.rs` — Expand PlatformTextSystem trait (add_fonts, font_id, font_metrics, glyph_for_char, layout_line)
3. `crates/flui-platform/Cargo.toml` — Add `Win32_Graphics_DirectWrite` feature

### Phase 5: Fix Examples & Integration

**Files to modify:**
1. All files in `crates/flui-platform/examples/` — Fix compilation errors
2. `examples/test_background.rs`, `examples/window_features.rs`, `examples/windows11_features.rs` — Fix unused imports, missing methods
3. Add new example: `examples/platform_callbacks.rs` — Demonstrate per-window callbacks

## Key Patterns

### Callback take/restore (reentrancy-safe)
```rust
fn dispatch_input(&self, event: PlatformInput) -> DispatchEventResult {
    let cb = self.callbacks.on_input.lock().take();
    if let Some(mut cb) = cb {
        let result = cb(event);
        *self.callbacks.on_input.lock() = Some(cb);
        result
    } else {
        DispatchEventResult::default()
    }
}
```

### Task<T> spawn and await
```rust
let task: Task<Vec<PathBuf>> = platform.prompt_for_paths(PathPromptOptions {
    files: true,
    directories: false,
    multiple: true,
});
let paths = task.await?;
```

### PlatformWindow with callbacks
```rust
let window = platform.open_window(options)?;
window.on_input(Box::new(|event| {
    // Handle mouse/keyboard events
    DispatchEventResult::default()
}));
window.on_resize(Box::new(|size, scale| {
    tracing::info!(?size, scale, "window resized");
}));
window.on_should_close(Box::new(|| {
    true // allow close
}));
```

## Verification Checklist

- [ ] `cargo check -p flui-platform` passes
- [ ] `cargo test -p flui-platform` passes
- [ ] `cargo clippy -p flui-platform -- -D warnings` passes
- [ ] `cargo check --workspace` passes (no regressions)
- [ ] All examples compile: `cargo check --examples`
- [ ] PlatformWindow has >= 37 methods
- [ ] Platform trait has >= 35 methods
- [ ] HeadlessPlatform has zero `unimplemented!()` calls
- [ ] All new public types have `///` doc comments
