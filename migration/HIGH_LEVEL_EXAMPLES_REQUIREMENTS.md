# High-Level Widget Examples - Requirements Analysis

**Date**: 2025-10-28
**Goal**: Write examples using Widget API instead of low-level Painter API

---

## Current Situation

### ❌ Current Examples (Low-Level)

Examples currently use **low-level Painter API** directly:

```rust
// File: crates/flui_engine/examples/container_showcase_demo.rs
impl AppLogic for ContainerShowcaseDemo {
    fn render(&mut self, painter: &mut dyn Painter) {
        // Manual painting with Painter
        painter.rect(rect, &paint);
        painter.circle(center, radius, &paint);
        painter.text(text, position, size, &paint);
    }
}
```

**Problems**:
- ❌ No widget composition
- ❌ No automatic layout
- ❌ Manual positioning of everything
- ❌ No state management
- ❌ Can't test Widget → RenderObject → Layer chain
- ❌ Doesn't match real app usage

### ✅ Desired Examples (High-Level)

Examples should use **Widget API**:

```rust
// Desired: High-level widget composition
impl Widget for MyApp {
    fn build(&self, ctx: &BuildContext) -> Box<dyn Widget> {
        Box::new(
            Container::builder()
                .padding(EdgeInsets::all(20.0))
                .color(Color::BLUE)
                .child(
                    Column::builder()
                        .children(vec![
                            Box::new(Text::new("Hello")),
                            Box::new(Container::builder()
                                .width(100.0)
                                .height(50.0)
                                .color(Color::RED)
                                .build()),
                        ])
                        .build()
                )
                .build()
        )
    }
}
```

**Benefits**:
- ✅ Widget composition (like real apps)
- ✅ Automatic layout
- ✅ Tests full Widget → RenderObject → Layer pipeline
- ✅ State management support
- ✅ Matches production code patterns

---

## What We Have

### ✅ Core Infrastructure

1. **flui_core** - Core framework
   - `Widget` trait
   - `RenderObject` trait
   - `PipelineOwner` (manages element tree)
   - Element system (ComponentElement, RenderObjectElement)
   - Build → Layout → Paint pipeline

2. **flui_widgets** - Widget library
   - `Container` ✅
   - `Padding` ✅
   - `Align` ✅
   - `DecoratedBox` ✅
   - `SizedBox` ✅
   - `Transform` ✅
   - `Text` (needs checking)
   - `Column`, `Row` (needs checking)
   - `Center` (needs checking)

3. **flui_app** - Application framework
   - `FluiApp` - manages application lifecycle
   - Integration with egui/eframe
   - Pointer event handling
   - Three-phase rendering

4. **flui_rendering** - RenderObject implementations
   - All render objects for widgets above ✅

5. **flui_engine** - Layer and painting
   - Layer system ✅
   - Painter abstraction ✅
   - egui backend ✅

---

## What's Missing / Needs Verification

### 1. ❓ App Entry Point for Widget-Based Examples

**Current**: Examples extend `AppLogic` trait which uses Painter directly

```rust
// Current low-level approach
struct MyDemo;
impl AppLogic for MyDemo {
    fn render(&mut self, painter: &mut dyn Painter) {
        // Low-level painting
    }
}
```

**Needed**: Entry point that accepts root Widget

```rust
// Desired high-level approach
struct MyApp;
impl Widget for MyApp {
    fn build(&self, ctx: &BuildContext) -> Box<dyn Widget> {
        // Widget composition
    }
}

fn main() {
    flui_app::run_app(Box::new(MyApp))?;
}
```

**Status**: ✅ EXISTS! See `flui_app::FluiApp::new(root_widget)` in [app.rs](../crates/flui_app/src/app.rs:71)

**Missing**: Convenient `run_app()` function wrapper

### 2. ❓ eframe Integration for Widget Apps

**Needed**: Bridge between `FluiApp` (widget-based) and eframe window

**Status**: ✅ PARTIALLY EXISTS

`FluiApp` has `update()` method that works with egui::Ui:

```rust
// File: crates/flui_app/src/app.rs
impl FluiApp {
    pub fn update(&mut self, ui: &egui::Ui) {
        // Handles build → layout → paint pipeline
    }
}
```

**Missing**: eframe::App implementation that wraps FluiApp

### 3. ❓ Widget State Management

**Needed**: `StatefulWidget` support for interactive examples

**Status**: ✅ EXISTS in flui_core

```rust
// File: crates/flui_core/src/widget/stateful.rs
pub trait StatefulWidget: Widget {
    type State: WidgetState<Self>;
    fn create_state(&self) -> Self::State;
}
```

**Verification Needed**: Does it work end-to-end?

### 4. ❓ Common Layout Widgets

**Check if these work**:
- `Column` - vertical layout
- `Row` - horizontal layout
- `Stack` - overlapping children
- `Center` - centers child
- `Flex` - flexible layout

**Status**: ⚠️ NEED TO VERIFY

Let's check what exists in flui_widgets.

### 5. ❓ Text Widget

**Needed**: Working `Text` widget for displaying text

**Status**: ⚠️ NEED TO VERIFY

Check if Text widget exists and works.

---

## Investigation Needed

### Step 1: Check Existing Widgets

<details>
<summary>File: crates/flui_widgets/src/lib.rs</summary>

Check what widgets are exported and available.
</details>

### Step 2: Check run_app() Function

<details>
<summary>File: crates/flui_app/src/lib.rs</summary>

Check if there's a `run_app()` convenience function.
</details>

### Step 3: Check eframe Integration

<details>
<summary>File: crates/flui_app/src/window.rs</summary>

Check how FluiApp integrates with eframe.
</details>

---

## Minimal Requirements for High-Level Examples

To write examples using Widget API, we need:

### Must Have ✅/❌

1. ✅ **Core Widget Infrastructure**
   - Widget trait
   - RenderObject trait
   - PipelineOwner
   - Build → Layout → Paint pipeline

2. ✅ **Basic Widgets**
   - Container (verified working ✅)
   - Padding (verified working ✅)
   - DecoratedBox (verified working ✅)
   - Align (verified working ✅)
   - SizedBox (verified working ✅)

3. ❓ **Layout Widgets** (NEED TO VERIFY)
   - Column
   - Row
   - Center
   - Stack

4. ❓ **Text Widget** (NEED TO VERIFY)
   - Text rendering
   - TextStyle support

5. ❓ **App Entry Point** (NEED TO VERIFY)
   - run_app() function
   - eframe::App wrapper for FluiApp

6. ✅ **State Management** (EXISTS)
   - StatefulWidget trait
   - State lifecycle

### Nice to Have

7. ❌ **Interactive Widgets** (NOT CRITICAL FOR BASIC EXAMPLES)
   - Button
   - GestureDetector
   - Clickable areas

8. ❌ **Advanced Layout** (NOT NEEDED YET)
   - ListView
   - GridView
   - CustomScrollView

---

## Action Plan

### Phase 1: Verification (1-2 hours)

1. **Check what widgets exist** ✓
   - Read flui_widgets/src/lib.rs
   - List all exported widgets
   - Check examples in flui_widgets

2. **Check app entry point** ✓
   - Read flui_app/src/lib.rs
   - Check if run_app() exists
   - Check window.rs for eframe integration

3. **Test basic example** ✓
   - Try to write simplest possible widget-based example
   - Identify what's missing

### Phase 2: Fill Gaps (2-4 hours)

Based on Phase 1 findings:

4. **Add missing entry point** (if needed)
   - Create run_app() convenience function
   - Implement eframe::App wrapper

5. **Verify/fix basic widgets** (if needed)
   - Test Text widget
   - Test Column/Row
   - Fix any issues

6. **Create template example** ✓
   - Simple widget-based hello world
   - Documentation for pattern

### Phase 3: Rewrite Examples (3-5 hours)

7. **Rewrite container demos** ✓
   - container_showcase_demo using widgets
   - alignment_test_demo using widgets

8. **Create new high-level demos** ✓
   - Counter app (StatefulWidget example)
   - Layout demo (Column/Row/Stack)
   - Nested container demo

---

## Example: What We Want to Write

### Simple App Example

```rust
use flui_app::run_app;
use flui_widgets::{Container, Text, Column};
use flui_types::{Color, EdgeInsets};

struct MyApp;

impl flui_core::StatelessWidget for MyApp {
    fn build(&self, _ctx: &BuildContext) -> Box<dyn DynWidget> {
        Box::new(
            Container::builder()
                .padding(EdgeInsets::all(20.0))
                .color(Color::rgb(240, 240, 240))
                .child(
                    Column::builder()
                        .children(vec![
                            Box::new(Text::new("Hello, Flui!")
                                .font_size(32.0)
                                .color(Color::rgb(50, 50, 50))),

                            Box::new(Container::builder()
                                .margin(EdgeInsets::symmetric(0.0, 20.0))
                                .width(200.0)
                                .height(100.0)
                                .decoration(BoxDecoration {
                                    color: Some(Color::rgb(66, 165, 245)),
                                    border_radius: Some(BorderRadius::circular(12.0)),
                                    ..Default::default()
                                })
                                .child(Center::builder()
                                    .child(Text::new("Click Me!")
                                        .color(Color::WHITE))
                                    .build())
                                .build()),
                        ])
                        .build()
                )
                .build()
        )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_app(Box::new(MyApp))
}
```

### Stateful Counter Example

```rust
use flui_core::{StatefulWidget, WidgetState, BuildContext};
use flui_widgets::{Container, Text, Column, GestureDetector};

struct Counter {
    initial_count: i32,
}

struct CounterState {
    count: i32,
}

impl StatefulWidget for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState { count: self.initial_count }
    }
}

impl WidgetState<Counter> for CounterState {
    fn build(&mut self, widget: &Counter, ctx: &BuildContext) -> Box<dyn DynWidget> {
        Box::new(
            Column::builder()
                .children(vec![
                    Box::new(Text::new(format!("Count: {}", self.count))
                        .font_size(24.0)),

                    Box::new(GestureDetector::builder()
                        .on_tap(|| {
                            self.count += 1;
                            // Trigger rebuild
                        })
                        .child(Container::builder()
                            .padding(EdgeInsets::all(12.0))
                            .color(Color::BLUE)
                            .child(Text::new("Increment")
                                .color(Color::WHITE))
                            .build())
                        .build()),
                ])
                .build()
        )
    }
}
```

---

## Expected Timeline

**Total Effort**: 6-11 hours

- **Phase 1 (Verification)**: 1-2 hours
- **Phase 2 (Fill Gaps)**: 2-4 hours
- **Phase 3 (Rewrite Examples)**: 3-5 hours

**Target**: Have high-level widget examples working by end of day.

---

## Next Steps

1. ✅ Read `crates/flui_widgets/src/lib.rs` - see what widgets exist
2. ✅ Read `crates/flui_app/src/lib.rs` - check for run_app()
3. ✅ Check existing examples in flui_app or flui_widgets
4. ⚠️ Try to compile a simple widget-based example
5. ⚠️ Document gaps and create action plan

Let's start with Step 1!
