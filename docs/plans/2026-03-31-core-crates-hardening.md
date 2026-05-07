# Core Crates Hardening Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix soundness bugs, unify duplicated systems, wire integration gaps, and create a minimal working app loop — preparing the foundation for widget tree reactivation.

**Architecture:** Bottom-up fixes (foundation → scheduler → rendering → app). Each task is independent within its priority wave.

**Tech Stack:** Rust, flui-foundation, flui-scheduler, flui-rendering, flui-app, flui-assets, flui-engine

---

## Wave 1: Foundation & Scheduler Fixes

### Task 1: Unify Scheduler IDs with Foundation IDs

**Problem:** `flui-scheduler` has its own `TypedId<M: IdMarker>` system (`TaskId`, `FrameId`, `TickerId`, `CallbackId`) parallel to `flui-foundation`'s `Id<T: Marker>`. Two isomorphic ID types.

**Files:**
- Modify: `crates/flui-scheduler/src/id.rs` — replace `TypedId<M>` with `Id<T>` from foundation
- Modify: `crates/flui-scheduler/src/task.rs` — update TaskId usage
- Modify: `crates/flui-scheduler/src/ticker.rs` — update TickerId usage
- Modify: `crates/flui-scheduler/src/scheduler.rs` — update CallbackId, FrameId usage
- Modify: `crates/flui-scheduler/src/frame.rs` — update FrameCallbackId
- Modify: `crates/flui-foundation/src/id.rs` — add `TaskId`, `TickerId` markers if not present

**What to do:**
1. Check which scheduler ID types already exist in foundation's `ids!` macro (FrameCallbackId is there)
2. Add missing markers to foundation: `TaskMarker`, `TickerMarker` → `TaskId`, `TickerId`
3. Replace all `TypedId<*Marker>` usages in scheduler with `flui_foundation::*Id`
4. Replace `IdGenerator<M>` with foundation's `Id::new()` (uses AtomicU64)
5. Delete `crates/flui-scheduler/src/id.rs` if fully replaced
6. Update `Handle<M>`, `FrameHandle`, `TaskHandle` to use foundation IDs

**Verify:** `cargo check -p flui-scheduler && cargo test -p flui-scheduler --lib`

**Commit:** `refactor(scheduler): unify IDs with flui-foundation Id<T> system`

---

### Task 2: Fix MergedListenable in Foundation

**Problem:** `MergedListenable` stores source listenables but never subscribes to them. Merging is purely nominal — notifications don't propagate.

**Files:**
- Modify: `crates/flui-foundation/src/notifier.rs` — fix MergedListenable

**What to do:**
1. Read current `MergedListenable` implementation
2. Fix: when sources are added, subscribe to each source's change notifications
3. When any source notifies, forward to MergedListenable's own listeners
4. Store subscription IDs for cleanup on drop
5. Add tests: merge two ValueNotifiers, change one, verify merged listeners fire

```rust
impl MergedListenable {
    pub fn new(sources: Vec<Box<dyn Listenable + Send>>) -> Self {
        let notifier = ChangeNotifier::new();
        let notifier_clone = notifier.clone();

        let listener_ids: Vec<_> = sources.iter().map(|source| {
            let n = notifier_clone.clone();
            source.add_listener(Arc::new(move || n.notify()))
        }).collect();

        Self { sources, notifier, listener_ids }
    }
}
```

**Verify:** `cargo test -p flui-foundation --lib`

**Commit:** `fix(foundation): MergedListenable now actually subscribes to sources`

---

### Task 3: Fix ObserverList deadlock potential

**Problem:** `SyncObserverList::for_each` holds RwLock read lock during iteration. If callback tries to add/remove observers → deadlock.

**Files:**
- Modify: `crates/flui-foundation/src/observer.rs`

**What to do:**
1. Read current `SyncObserverList::for_each` implementation
2. Fix: clone observer entries while holding lock, release lock, then iterate cloned entries
3. This matches the take/restore pattern used in `WindowCallbacks`

```rust
pub fn for_each(&self, f: impl FnMut(&T)) {
    // Clone entries while holding lock
    let entries: Vec<T> = {
        let guard = self.inner.read();
        guard.iter().cloned().collect()
    };
    // Iterate without lock held
    for entry in &entries {
        f(entry);
    }
}
```

Note: This requires `T: Clone`. Check if the existing bound allows it.

**Verify:** `cargo test -p flui-foundation --lib`

**Commit:** `fix(foundation): prevent deadlock in SyncObserverList::for_each`

---

## Wave 2: Minimal Working App Loop

### Task 4: flui-app direct rendering path (no widget tree)

**Problem:** `run_desktop()` depends on flui-view (disabled). No working app example exists.

**Files:**
- Modify: `crates/flui-app/src/app/runner.rs` — add `run_direct()` path
- Create: `examples/direct_render.rs` — minimal working example

**What to do:**
1. Create a `run_direct()` function that bypasses flui-view/flui-rendering entirely:

```rust
pub fn run_direct(
    config: AppConfig,
    render_fn: impl FnMut(&mut flui_layer::SceneBuilder, (f32, f32)) + Send + 'static,
) -> Result<()> {
    let platform = current_platform()?;
    let window = platform.open_window(config.window_options())?;

    // Init GPU renderer
    let mut renderer = pollster::block_on(
        flui_engine::wgpu::Renderer::new(&window)
    )?;

    // Wire input → damage
    window.on_input(Box::new(|input| {
        // Mark damage on any input
        DispatchEventResult::default()
    }));

    // Wire frame rendering
    let mut render_fn = render_fn;
    window.on_request_frame(Box::new(move || {
        let size = window.physical_size();
        let mut builder = flui_layer::SceneBuilder::new();
        render_fn(&mut builder, (size.width.0 as f32, size.height.0 as f32));
        let scene = builder.build();
        if let Err(e) = renderer.render_scene(&scene) {
            tracing::error!("Render error: {}", e);
        }
    }));

    window.request_redraw();
    platform.run(Box::new(|| {}));
    Ok(())
}
```

2. Create `examples/direct_render.rs`:

```rust
use flui_app::run_direct;
use flui_layer::SceneBuilder;

fn main() -> anyhow::Result<()> {
    run_direct(
        AppConfig::new().title("Direct Render").size(800, 600),
        |builder, (w, h)| {
            // Draw directly with layer/painting API
            let mut canvas = flui_painting::Canvas::new(w, h);
            canvas.draw_rect(Rect::from_ltrb(100, 100, 300, 200), &paint);
            builder.push_canvas(canvas.finish());
        },
    )
}
```

**Important:** This may require adjusting ownership patterns (window + renderer in closure). Read `run_desktop()` carefully and adapt.

**Verify:** `cargo run --example direct_render`

**Commit:** `feat(app): add run_direct() for rendering without widget tree`

---

### Task 5: Wire platform input → engine damage tracker

**Problem:** Input events don't trigger redraws. Damage tracker exists but no one marks damage on input.

**Files:**
- Modify: `crates/flui-app/src/app/runner.rs` — in the `on_input` callback, call `renderer.mark_full_repaint()` + `window.request_redraw()`

**What to do:**

In the desktop runner's `on_input` callback:

```rust
window.on_input(Box::new(move |input| {
    // Any input → mark damage → request redraw
    renderer.mark_full_repaint();
    window.request_redraw();
    DispatchEventResult::default()
}));
```

This ensures: pointer move → damage marked → next frame renders.

**Verify:** `cargo check -p flui-app`

**Commit:** `feat(app): wire input events to damage tracker for auto-redraw`

---

### Task 6: Fix web renderer in flui-app

**Problem:** In `run_web()`, renderer is initialized in `spawn_local` but the RAF callback has no access to it. No `render_scene` call.

**Files:**
- Modify: `crates/flui-app/src/app/runner.rs` — fix `run_web()` path

**What to do:**
1. Read current `run_web()` implementation
2. Move renderer to a `Rc<RefCell<Option<Renderer>>>` accessible from RAF callback
3. In RAF callback, if renderer is Some, call `render_scene()`

**Verify:** `cargo check -p flui-app --target wasm32-unknown-unknown` (if wasm target installed)

**Commit:** `fix(app): wire web renderer to RAF loop for actual rendering`

---

## Wave 3: Rendering Crate Preparation

### Task 7: Replace raw pointer in RenderView

**Problem:** `RenderView` stores `owner: Option<*const PipelineOwner>` — soundness violation. Constitution says zero unsafe outside platform/engine.

**Files:**
- Modify: `crates/flui-rendering/src/view/render_view.rs`

**What to do:**
1. Replace `*const PipelineOwner` with `Option<Weak<RwLock<PipelineOwner>>>`
2. Access via `owner.as_ref().and_then(|w| w.upgrade())`
3. Remove `unsafe impl Send for RenderView` and `unsafe impl Sync for RenderView`
4. If `PipelineOwner` is not behind `Arc<RwLock<>>`, refactor to make it so

**Verify:** `cargo check -p flui-rendering`

**Commit:** `fix(rendering): replace raw pointer with Weak<RwLock<>> in RenderView`

---

### Task 8: Audit DirtyNode.id offset convention

**Problem:** Comments say `DirtyNode.id` is 0-based slab index, but code does `RenderId::new(id)` which expects 1-based.

**Files:**
- Modify: `crates/flui-rendering/src/pipeline/owner.rs`

**What to do:**
1. Find all places where `DirtyNode` is created — what value goes into `.id`?
2. Find all places where `DirtyNode.id` is used — is `RenderId::new(id)` called?
3. If `.id` stores raw slab index (0), then `RenderId::new(0)` panics (NonZeroUsize)
4. Fix: either store `RenderId` values in DirtyNode (correct), or add +1 offset
5. Add a comment clarifying the convention

**Verify:** `cargo check -p flui-rendering`

**Commit:** `fix(rendering): clarify and fix DirtyNode id offset convention`

---

### Task 9: Consolidate HitTestable traits

**Problem:** Two `HitTestable` traits — one in flui-interaction (sealed), one in flui-rendering (with view_id). Both in prelude.

**Files:**
- Modify: `crates/flui-rendering/src/binding/mod.rs` — rename or remove duplicate
- Modify: `crates/flui-rendering/src/hit_testing/` — use interaction's types

**What to do:**
1. Read both `HitTestable` definitions
2. If rendering's version adds `view_id`, rename it to `ViewHitTestable` or merge the parameter into interaction's version
3. Ensure no name collision in prelude

**Verify:** `cargo check -p flui-rendering`

**Commit:** `refactor(rendering): consolidate HitTestable trait with flui-interaction`

---

### Task 10: Fix RenderFlex to use FlexParentData

**Problem:** `RenderFlex` uses `BoxParentData` instead of `FlexParentData`. Cannot express `Expanded` / `Flexible`.

**Files:**
- Modify: `crates/flui-rendering/src/objects/flex.rs`

**What to do:**
1. Change `type ParentData = BoxParentData` to `type ParentData = FlexParentData`
2. In `perform_layout`, read `parent_data.flex` and `parent_data.fit` for each child
3. Implement two-pass flex layout:
   - Pass 1: layout non-flex children, sum their sizes
   - Pass 2: distribute remaining space to flex children by flex factor
4. This matches Flutter's `RenderFlex.performLayout()`

**Verify:** `cargo check -p flui-rendering && cargo test -p flui-rendering --lib`

**Commit:** `fix(rendering): RenderFlex uses FlexParentData for proper flex layout`

---

## Wave 4: Asset Integration

### Task 11: Wire AssetRegistry → TextureCache

**Problem:** flui-assets has `AssetRegistry` with caching. flui-engine has `TextureCache`. No connection.

**Files:**
- Modify: `crates/flui-engine/src/wgpu/texture_cache.rs` — add `load_from_asset()` method
- Modify: `crates/flui-engine/Cargo.toml` — add optional `flui-assets` dependency

**What to do:**
1. Add `flui-assets` as optional dependency of `flui-engine` (feature: `assets`)
2. Add method to TextureCache:

```rust
#[cfg(feature = "assets")]
pub async fn load_from_asset(&mut self, asset: flui_assets::ImageAsset) -> Result<&TextureView> {
    let registry = flui_assets::AssetRegistry::global();
    let handle = registry.load(asset).await?;
    // handle.data is Image → convert to RGBA → load_from_rgba()
    self.load_from_rgba(id, handle.width, handle.height, &handle.rgba_bytes)
}
```

**Verify:** `cargo check -p flui-engine --features assets`

**Commit:** `feat(engine): integrate flui-assets for texture loading`

---

### Task 12: Wire AssetRegistry → cosmic-text FontSystem

**Problem:** Font loading is ad-hoc. flui-assets has `FontAsset` with caching. cosmic-text's `FontSystem` needs font bytes.

**Files:**
- Create: `crates/flui-engine/src/wgpu/font_loader.rs` — bridge between assets and cosmic-text
- Modify: `crates/flui-engine/src/wgpu/text.rs` — use font_loader

**What to do:**
1. Create `FontLoader` that uses `flui-assets::AssetRegistry` to load fonts:

```rust
pub struct FontLoader;

impl FontLoader {
    pub async fn load_font(font_system: &mut FontSystem, path: &str) -> Result<()> {
        let registry = flui_assets::AssetRegistry::global();
        let font = flui_assets::FontAsset::file(path);
        let handle = registry.load(font).await?;
        font_system.db_mut().load_font_data(handle.bytes.clone());
        Ok(())
    }

    pub fn load_font_sync(font_system: &mut FontSystem, bytes: Vec<u8>) {
        font_system.db_mut().load_font_data(bytes);
    }
}
```

2. Use in `TextRenderer::new()` to add bundled fallback fonts

**Verify:** `cargo check -p flui-engine --features assets`

**Commit:** `feat(engine): font loading bridge between flui-assets and cosmic-text`

---

## Execution Order

```
Wave 1 (Foundation fixes): Tasks 1, 2, 3 — all independent, parallel
Wave 2 (App loop): Tasks 4, 5, 6 — sequential (4 first, then 5+6)
Wave 3 (Rendering prep): Tasks 7, 8, 9, 10 — independent, parallel
Wave 4 (Assets): Tasks 11, 12 — independent, parallel
```
