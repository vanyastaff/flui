# FLUI Crate Integration Guide

**Purpose:** How FLUI crates work together
**Last Updated:** 2025-01-10

> This document explains how the 12 FLUI crates integrate to form a complete UI framework. For detailed crate architecture, see individual `*_ARCHITECTURE.md` files.

---

## Table of Contents

1. [Dependency Overview](#dependency-overview)
2. [Core Integration Flows](#core-integration-flows)
3. [Common Integration Scenarios](#common-integration-scenarios)
4. [Integration Patterns](#integration-patterns)

---

## Dependency Overview

### Crate Dependency Graph

```text
                         flui_app
                            ↓
        ┌───────────────────┴───────────────────┐
        ↓                                       ↓
   flui_widgets                          flui_devtools
        ↓                                       ↓
   flui_core ←──────────────────────────────────┘
        ↓
   ┌────┴─────┬──────────┬──────────┐
   ↓          ↓          ↓          ↓
flui_rendering  flui_gestures  flui_animation  flui_assets
   ↓
   ├─ flui_painting
   └─ flui_engine
        ↓
   flui_types
```

### Dependency Layers

| Layer | Crates | Purpose |
|-------|--------|---------|
| **Layer 0: Foundation** | `flui_types` | Base types (Size, Rect, Color) - zero dependencies |
| **Layer 1: GPU/Assets** | `flui_painting`, `flui_engine`, `flui_assets` | Low-level GPU rendering and asset loading |
| **Layer 2: Core** | `flui_core` | Element tree, pipeline, hooks, View trait |
| **Layer 3: Rendering** | `flui_rendering`, `flui_gestures`, `flui_animation` | RenderObjects and input/animation systems |
| **Layer 4: Widgets** | `flui_widgets` | High-level declarative UI components |
| **Layer 5: Application** | `flui_app`, `flui_devtools` | Application framework and developer tools |

**Dependency Rule:** Lower layers never depend on higher layers (strict hierarchy)

---

## Core Integration Flows

### Flow 1: Widget → Element → Render

**Purpose:** Transform declarative UI into renderable scene graph

```text
┌─────────────────────────────────────────────────────────────┐
│ Phase 1: BUILD                                              │
│                                                              │
│  User Code                  flui_widgets                     │
│  ┌────────────┐            ┌────────────┐                  │
│  │ Container  │  build()   │ Padding    │                  │
│  │  └─ Text   │ ────────> │  └─ Text   │                  │
│  └────────────┘            └────────────┘                  │
│                                  │                           │
│                                  ▼                           │
│                            flui_core                         │
│                            ┌─────────────────┐              │
│                            │ Element Tree    │              │
│                            │  Component      │              │
│                            │   └─ Render     │              │
│                            └─────────────────┘              │
│                                  │                           │
│                                  ▼                           │
│                            flui_rendering                    │
│                            ┌─────────────────┐              │
│                            │ RenderPadding   │              │
│                            │  └─ RenderText  │              │
│                            └─────────────────┘              │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Phase 2: LAYOUT                                             │
│                                                              │
│  flui_core/pipeline                                          │
│  ┌─────────────────┐                                        │
│  │ PipelineOwner   │                                        │
│  │ flush_layout()  │                                        │
│  └────────┬────────┘                                        │
│           │                                                  │
│           ▼                                                  │
│  flui_rendering                                              │
│  ┌─────────────────────────────────────┐                   │
│  │ RenderObject.layout(constraints)    │                   │
│  │  1. Layout children                 │                   │
│  │  2. Compute own size                │                   │
│  │  3. Return size                      │                   │
│  └─────────────────────────────────────┘                   │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Phase 3: PAINT                                              │
│                                                              │
│  flui_core/pipeline                                          │
│  ┌─────────────────┐                                        │
│  │ PipelineOwner   │                                        │
│  │ flush_paint()   │                                        │
│  └────────┬────────┘                                        │
│           │                                                  │
│           ▼                                                  │
│  flui_rendering → flui_painting → flui_engine               │
│  ┌────────────┐   ┌────────────┐   ┌────────────┐         │
│  │ RenderObj  │   │ Canvas API │   │ WgpuPainter│         │
│  │ .paint()   │──>│ DisplayList│──>│ GPU Render │         │
│  └────────────┘   └────────────┘   └────────────┘         │
└─────────────────────────────────────────────────────────────┘
```

**Key Integration Points:**

1. **flui_widgets → flui_core:**
   - Widget implements `View` trait
   - `build()` returns `impl IntoElement`
   - Framework creates `Element` and inserts into tree

2. **flui_core → flui_rendering:**
   - `RenderElement` contains `Box<dyn Render>`
   - Pipeline calls `layout()` and `paint()` on RenderObject
   - Layout cache managed by Element

3. **flui_rendering → flui_painting:**
   - RenderObject creates `Canvas`
   - Records drawing commands to `DisplayList`
   - Returns `BoxedLayer` containing DisplayList

4. **flui_painting → flui_engine:**
   - `PictureLayer` holds `DisplayList`
   - `WgpuPainter` executes draw commands
   - Tessellates paths with Lyon, renders text with Glyphon

**See Also:**
- [WIDGETS_ARCHITECTURE.md](WIDGETS_ARCHITECTURE.md#view-trait-implementation)
- [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md#integration-with-other-layers)
- [PATTERNS.md](PATTERNS.md#three-tree-architecture)

---

### Flow 2: State Update → Rebuild

**Purpose:** React to state changes and update UI

```text
┌─────────────────────────────────────────────────────────────┐
│ 1. User Interaction                                         │
│                                                              │
│  User clicks button                                          │
│        ↓                                                     │
│  Event handler executes                                      │
│        ↓                                                     │
│  signal.set(new_value)  ← flui_core/hooks                   │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Signal Update                                            │
│                                                              │
│  flui_core/hooks/signal.rs                                   │
│  ┌──────────────────────────────────┐                       │
│  │ Signal::set(value)               │                       │
│  │  1. Update value in runtime      │                       │
│  │  2. Notify listeners             │                       │
│  │  3. Schedule rebuild              │                       │
│  └──────────────────────────────────┘                       │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Rebuild Scheduling                                       │
│                                                              │
│  flui_core/pipeline                                          │
│  ┌──────────────────────────────────┐                       │
│  │ PipelineOwner.schedule_rebuild() │                       │
│  │  1. Mark element dirty           │                       │
│  │  2. Add to rebuild queue         │                       │
│  │  3. Request frame                 │                       │
│  └──────────────────────────────────┘                       │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. Frame Rendering                                          │
│                                                              │
│  flui_core/pipeline/frame_coordinator.rs                     │
│  ┌──────────────────────────────────┐                       │
│  │ begin_frame()                    │                       │
│  │  ↓                                │                       │
│  │ flush_build()    ← Rebuild dirty │                       │
│  │  ↓                                │                       │
│  │ flush_layout()   ← Recompute     │                       │
│  │  ↓                                │                       │
│  │ flush_paint()    ← Regenerate    │                       │
│  │  ↓                                │                       │
│  │ end_frame()                       │                       │
│  └──────────────────────────────────┘                       │
└─────────────────────────────────────────────────────────────┘
```

**Key Integration Points:**

1. **flui_core/hooks → flui_core/pipeline:**
   - Signal notifies listeners
   - Listener calls `schedule_rebuild(element_id)`
   - Element marked dirty in pipeline

2. **flui_core/pipeline (build phase):**
   - Topologically sort dirty elements
   - Call `element.rebuild()`
   - Element calls `view.build()` again
   - Diff and update element tree

3. **flui_core/pipeline (layout phase):**
   - Elements with layout dirty flag
   - Call `render_object.layout()`
   - Cache result in element

4. **flui_core/pipeline (paint phase):**
   - Elements with paint dirty flag
   - Call `render_object.paint()`
   - Generate new layer tree

**See Also:**
- [PATTERNS.md](PATTERNS.md#copy-based-signals)
- [Pipeline Architecture](../PIPELINE_ARCHITECTURE.md)

---

### Flow 3: Input Event → Widget Handler

**Purpose:** Route user input to appropriate widgets

```text
┌─────────────────────────────────────────────────────────────┐
│ 1. Platform Event                                           │
│                                                              │
│  flui_app                                                    │
│  ┌──────────────────────────────────┐                       │
│  │ Window receives platform event   │                       │
│  │  (mouse click, key press, etc.)  │                       │
│  └────────────────┬─────────────────┘                       │
└──────────────────│─────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Event Conversion                                         │
│                                                              │
│  flui_app → flui_types                                       │
│  ┌──────────────────────────────────┐                       │
│  │ Convert to FLUI PointerEvent     │                       │
│  │  - PointerDown                    │                       │
│  │  - PointerMove                    │                       │
│  │  - PointerUp                      │                       │
│  └────────────────┬─────────────────┘                       │
└──────────────────│─────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Hit Testing                                              │
│                                                              │
│  flui_gestures                                               │
│  ┌──────────────────────────────────┐                       │
│  │ HitTestResult                    │                       │
│  │  1. Traverse render tree         │                       │
│  │  2. Check bounds                  │                       │
│  │  3. Build path to target          │                       │
│  └────────────────┬─────────────────┘                       │
└──────────────────│─────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. Gesture Recognition                                      │
│                                                              │
│  flui_gestures                                               │
│  ┌──────────────────────────────────┐                       │
│  │ GestureRecognizer                │                       │
│  │  - TapRecognizer                  │                       │
│  │  - DragRecognizer                 │                       │
│  │  - ScaleRecognizer                │                       │
│  │  → Gesture arena resolution       │                       │
│  └────────────────┬─────────────────┘                       │
└──────────────────│─────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────────────┐
│ 5. Widget Callback                                          │
│                                                              │
│  flui_widgets                                                │
│  ┌──────────────────────────────────┐                       │
│  │ GestureDetector                  │                       │
│  │  onTap: || { ... }               │                       │
│  │  onDrag: |details| { ... }       │                       │
│  └──────────────────────────────────┘                       │
└─────────────────────────────────────────────────────────────┘
```

**Key Integration Points:**

1. **flui_app → flui_types:**
   - Platform-specific event conversion
   - Normalized event types across platforms

2. **flui_gestures → flui_rendering:**
   - Hit testing via `RenderObject.hit_test()`
   - Builds path through render tree

3. **flui_gestures → flui_widgets:**
   - Widget provides gesture callbacks
   - GestureDetector widget wraps gesture recognizers

**See Also:**
- [GESTURES_ARCHITECTURE.md](GESTURES_ARCHITECTURE.md)
- [APP_ARCHITECTURE.md](APP_ARCHITECTURE.md)

---

### Flow 4: Asset Loading → Image Display

**Purpose:** Load and display images/fonts efficiently

```text
┌─────────────────────────────────────────────────────────────┐
│ 1. Widget Requests Asset                                    │
│                                                              │
│  flui_widgets                                                │
│  ┌──────────────────────────────────┐                       │
│  │ Image::asset("logo.png")         │                       │
│  └────────────────┬─────────────────┘                       │
└──────────────────│─────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Asset Resolution                                         │
│                                                              │
│  flui_assets                                                 │
│  ┌──────────────────────────────────┐                       │
│  │ AssetRegistry::global()          │                       │
│  │  1. Check cache                   │                       │
│  │  2. If miss, load from bundle     │                       │
│  │  3. Decode image                  │                       │
│  │  4. Store in cache                │                       │
│  └────────────────┬─────────────────┘                       │
└──────────────────│─────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Image Upload to GPU                                      │
│                                                              │
│  flui_painting                                               │
│  ┌──────────────────────────────────┐                       │
│  │ ImageHandle created              │                       │
│  │  - Upload to GPU texture          │                       │
│  │  - Store texture ID               │                       │
│  └────────────────┬─────────────────┘                       │
└──────────────────│─────────────────────────────────────────┘
                   │
                   ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. Image Rendering                                          │
│                                                              │
│  flui_rendering → flui_painting → flui_engine               │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐               │
│  │ RenderImg│   │ Canvas   │   │ Wgpu     │               │
│  │ .paint() │──>│ drawImage│──>│ texture  │               │
│  └──────────┘   └──────────┘   └──────────┘               │
└─────────────────────────────────────────────────────────────┘
```

**Key Integration Points:**

1. **flui_widgets → flui_assets:**
   - Widget provides asset path
   - AssetRegistry resolves and loads

2. **flui_assets → flui_painting:**
   - Decoded image data
   - ImageHandle creation

3. **flui_painting → flui_engine:**
   - Image draw command in DisplayList
   - GPU texture binding

**See Also:**
- [ASSETS_ARCHITECTURE.md](ASSETS_ARCHITECTURE.md)
- [PAINTING_ARCHITECTURE.md](PAINTING_ARCHITECTURE.md)

---

## Common Integration Scenarios

### Scenario 1: Adding a New Widget

**Steps:**

1. **Create View** (flui_widgets)
```rust
// In flui_widgets/src/my_widget.rs
impl View for MyWidget {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Option A: Compose existing widgets
        Container::new().child(Text::new(self.text))

        // Option B: Create RenderObject
        (RenderMyWidget::new(self.data), self.child)
    }
}
```

2. **If Option B, Create RenderObject** (flui_rendering)
```rust
// In flui_rendering/src/objects/my_widget.rs
impl Render for RenderMyWidget {
    fn layout(&mut self, ctx: &LayoutContext) -> Size { ... }
    fn paint(&self, ctx: &PaintContext) -> BoxedLayer { ... }
    fn arity(&self) -> Arity { Arity::Exact(1) }
}
```

3. **Test Integration**
```rust
// In tests/integration_test.rs
let widget = MyWidget::new("test");
let element = widget.build(ctx);
// Verify element structure
```

**See Also:** [PATTERNS.md](PATTERNS.md#unified-view-trait)

---

### Scenario 2: Adding Custom Layout Algorithm

**Steps:**

1. **Implement Render Trait** (flui_rendering)
   - Define layout logic
   - Specify arity (child count)

2. **Create Widget Wrapper** (flui_widgets)
   - Ergonomic API
   - Builder pattern

3. **Test Layout**
   - Verify constraints propagation
   - Test edge cases

**See Also:** [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md#creating-custom-renderobjects)

---

### Scenario 3: Adding Platform Channel

**Steps:**

1. **Define Channel** (flui_app)
   - Channel name
   - Method codec

2. **Register Handler**
   - Platform-specific implementation

3. **Call from Widget**
   - MethodChannel.invokeMethod()

**See Also:** [APP_ARCHITECTURE.md](APP_ARCHITECTURE.md#platform-channels)

---

## Integration Patterns

### Pattern: Provider/Consumer

**Purpose:** Dependency injection through element tree

**Implementation:**
- Provider widget inserts data into element tree
- Consumer widget looks up data via BuildContext

**Key Files:**
- `crates/flui_core/src/element/provider.rs`

---

### Pattern: Notification Bubbling

**Purpose:** Event propagation up the tree

**Implementation:**
- Child dispatches notification
- Notification bubbles to parent elements
- Listeners registered in ancestor widgets

**Key Files:**
- `crates/flui_core/src/foundation/notification.rs`

---

### Pattern: Inherited Widgets

**Purpose:** Share data down the tree without explicit passing

**Implementation:**
- InheritedWidget stores data
- Descendants access via BuildContext.dependOnInheritedWidgetOfExactType()

**Key Files:**
- `crates/flui_core/src/element/provider.rs`

---

## Summary

This guide covers how FLUI crates integrate:

- **Dependency Graph**: 5-layer hierarchy from types → app
- **Core Flows**: Widget→Render, State→Rebuild, Input→Handler, Asset→Display
- **Common Scenarios**: Adding widgets, custom layout, platform channels
- **Integration Patterns**: Provider, Notification, Inherited widgets

For detailed crate architecture, see individual `*_ARCHITECTURE.md` files.

---

## Navigation

- [Back to Architecture Index](README.md)
- [Patterns Reference](PATTERNS.md)
- [Core Architecture](CORE_FEATURES_ROADMAP.md)
- [Widget Development](WIDGETS_ARCHITECTURE.md)
