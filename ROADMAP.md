# Flui Development Roadmap

> Comprehensive prioritized plan based on GLOSSARY analysis (~3500 types)
>
> **Philosophy:** Bottom-up architecture - Types → Core → Rendering → Widgets → Gestures → Material
>
> **Goal:** Build complete Flutter-like UI framework in Rust with three-tree architecture

---

## 📊 Current Status Overview

### Completed Crates (100%)

#### ✅ flui_types - Foundation Types (100% - COMPLETE)
- **13,677 lines of code** | **524 tests**
- All 11 base modules complete
- **Modules:**
  - ✅ Geometry (Point, Rect, Size, Offset, RRect) - 1910 lines, 68 tests
  - ✅ Layout (Axis, Alignment, EdgeInsets, MainAxisAlignment, FlexFit) - 2136 lines, 49 tests
  - ✅ Styling (Color, Border, Gradient, BoxDecoration, Shadow) - 3287 lines, 116 tests
  - ✅ Typography (TextStyle, TextAlign, TextSpan, FontWeight) - 983 lines, 50 tests
  - ✅ Painting (BoxFit, ImageRepeat, BlendMode, Clip) - 1048 lines, 62 tests
  - ✅ Animation (Curve, Tween, AnimationStatus) - 1089 lines, 37 tests
  - ✅ Physics (SpringSimulation, FrictionSimulation, GravitySimulation) - 902 lines, 47 tests
  - ✅ Constraints (BoxConstraints, SliverConstraints, SliverGeometry) - 1008 lines, 41 tests
  - ✅ Gestures Details (TapDownDetails, DragStartDetails, Velocity) - 758 lines, 23 tests
  - ✅ Semantics Data (SemanticsTag, SemanticsAction, SemanticsEvent) - 599 lines, 35 tests
  - ✅ Platform Types (TargetPlatform, Brightness, DeviceOrientation) - 557 lines, 24 tests

#### ✅ flui_foundation - Foundation Layer (100% - COMPLETE)
- **~2000 lines of code** | **~50 tests**
- **Modules:**
  - ✅ Keys (Key, LocalKey, ValueKey, GlobalKey, UniqueKey)
  - ✅ Observables (ChangeNotifier, ValueNotifier, Listenable)
  - ✅ Diagnostics (DiagnosticsNode, DiagnosticsProperty, DiagnosticableTree)

#### ✅ flui_core - Core Architecture (100% - COMPLETE)
- **~4000 lines of code** | **49 tests**
- Complete three-tree architecture implementation
- **Widget System:**
  - ✅ Widget trait (with DynClone + Downcast)
  - ✅ StatelessWidget
  - ✅ StatefulWidget + State<T>
  - ✅ InheritedWidget (with dependency tracking)
  - ✅ RenderObjectWidget (Leaf, SingleChild, MultiChild)
- **Element System:**
  - ✅ Element trait (with DowncastSync)
  - ✅ ComponentElement
  - ✅ StatefulElement (with State lifecycle)
  - ✅ InheritedElement (with dependency tracking + notify_dependents)
  - ✅ RenderObjectElement
- **Other:**
  - ✅ BuildContext (with depend_on_inherited_widget<T>)
  - ✅ ElementTree + PipelineOwner
  - ✅ RenderObject trait
  - ✅ ParentData system

#### ✅ flui_rendering - Rendering System (33% - 14/42 RenderObjects)
- **~6000 lines of code** | **117 tests**
- **Core:**
  - ✅ RenderBox (base implementation)
  - ✅ RenderProxyBox (single child pass-through)
- **Layout RenderObjects (10/15):**
  - ✅ RenderFlex (Row/Column) - 550 lines, 15 tests
  - ✅ RenderPadding - 280 lines, 8 tests
  - ✅ RenderStack - 330 lines, 13 tests
  - ✅ RenderConstrainedBox (SizedBox) - 180 lines, 10 tests
  - ✅ RenderAspectRatio - 390 lines, 17 tests
  - ✅ RenderLimitedBox - 380 lines, 13 tests
  - ✅ RenderIndexedStack - 430 lines, 13 tests
  - ✅ RenderPositionedBox (Align/Center) - 410 lines, 16 tests
  - ✅ RenderFractionallySizedBox - 400 lines, 15 tests
  - ✅ RenderDecoratedBox - 320 lines, 10 tests
  - ⏳ RenderWrap
  - ⏳ RenderIntrinsicWidth, RenderIntrinsicHeight
  - ⏳ RenderFlow
  - ⏳ RenderTable
- **Visual Effects RenderObjects (3/10):**
  - ✅ RenderOpacity - 280 lines, 15 tests
  - ✅ RenderTransform - matrix transformations
  - ✅ RenderClipRRect - rounded clipping
  - ⏳ RenderClipRect, RenderClipOval, RenderClipPath
  - ⏳ RenderPhysicalModel, RenderPhysicalShape
  - ⏳ RenderCustomPaint
  - ⏳ RenderBackdropFilter
- **Other RenderObjects (0/17):**
  - ⏳ RenderIgnorePointer, RenderAbsorbPointer (interaction)
  - ⏳ RenderParagraph (text)
  - ⏳ RenderImage (images)
  - ⏳ Sliver system (15+ types for scrolling)

#### ✅ flui_widgets - Widget Library (17/1000+ widgets - 2%)
- **~7000 lines of code** | **292 tests**
- **Basic Layout (7/10):**
  - ✅ Container - 335 lines, 18 tests
  - ✅ SizedBox - 279 lines, 18 tests
  - ✅ Padding - 242 lines, 11 tests
  - ✅ Center - 210 lines, 11 tests
  - ✅ Align - 332 lines, 17 tests
  - ✅ DecoratedBox - 464 lines, 15 tests
  - ✅ AspectRatio - 340 lines, 19 tests
  - ⏳ ConstrainedBox
  - ⏳ LimitedBox
  - ⏳ FractionallySizedBox
- **Flex Layout (4/6):**
  - ✅ Row - 261 lines, 13 tests
  - ✅ Column - 261 lines, 13 tests
  - ✅ Flexible - 440 lines, 19 tests
  - ✅ Expanded - 420 lines, 13 tests
  - ⏳ Flex
  - ⏳ Wrap
- **Stack Layout (3/3):**
  - ✅ Stack - 542 lines, 18 tests
  - ✅ Positioned - 737 lines, 22 tests
  - ✅ IndexedStack - 624 lines, 22 tests
- **Visual Effects (3/7):**
  - ✅ Opacity - 350 lines, 18 tests
  - ✅ Transform - 536 lines, 23 tests
  - ✅ ClipRRect - 609 lines, 21 tests
  - ⏳ ClipRect, ClipOval, ClipPath
  - ⏳ BackdropFilter

#### ✅ flui_app - Application Framework (100% - COMPLETE)
- **~500 lines of code**
- **Modules:**
  - ✅ FluiApp (main app structure)
  - ✅ run_app() (entry point)
  - ✅ eframe integration
  - ✅ Window management

### In Progress Crates

#### 🚧 flui_gestures - Gesture System (~2% - 5/125 types)
- **Status:** Event handling infrastructure started
- **Completed:**
  - ✅ PointerEvent types (Down, Up, Move, Enter, Exit, Cancel)
  - ✅ PointerEventData (position, device info)
  - ✅ PointerDeviceKind (Mouse, Touch, Stylus, etc.)
  - ✅ HitTestResult + HitTestEntry
  - ✅ GestureDetector widget (basic structure)
- **Next Steps:**
  - ⏳ Implement hit testing in RenderObjects
  - ⏳ Integrate pointer events with eframe
  - ⏳ Complete GestureDetector callbacks
  - ⏳ TapGestureRecognizer
  - ⏳ DragGestureRecognizer

### Not Started Crates

#### ⏳ flui_animation - Animation Controllers (~0%)
- **From GLOSSARY:** ~60 types
- **Priority:** HIGH (needed for Material widgets)
- **Core types:**
  - Animation<T>
  - AnimationController
  - Ticker, TickerProvider
  - CurvedAnimation
  - AnimationMin, AnimationMax

#### ⏳ flui_scheduler - Frame Scheduling (~0%)
- **From GLOSSARY:** ~12 types
- **Priority:** MEDIUM (needed for animations)
- **Core types:**
  - SchedulerBinding
  - SchedulerPhase
  - FrameTiming

#### ⏳ flui_painting - Painting Utilities (~0%)
- **From GLOSSARY:** ~160 types
- **Priority:** MEDIUM (needed for images/text)
- **Core types:**
  - TextPainter
  - ImageProvider, ImageCache
  - Canvas, Paint, Path (maybe from egui)

#### ⏳ flui_semantics - Accessibility (~0%)
- **From GLOSSARY:** ~43 types
- **Priority:** LOW
- **Core types:**
  - SemanticsNode, SemanticsOwner
  - SemanticsConfiguration

#### ⏳ flui_service - Platform Services (~0%)
- **From GLOSSARY:** ~530 types
- **Priority:** LOW
- **Core types:**
  - PlatformViewController
  - AssetBundle
  - Clipboard
  - MessageChannel

#### ⏳ flui_material - Material Design (~0%)
- **From GLOSSARY:** ~1000+ types
- **Priority:** HIGH (user-facing components)
- **Core widgets:**
  - Scaffold, AppBar
  - FloatingActionButton
  - Dialog, BottomSheet
  - Card, Chip, ListTile
  - Material theming

---

## 🎯 Prioritized Development Phases

### Phase 1: Event Handling Infrastructure (Week 7-8) - 85% COMPLETE ✅

**Goal:** Enable interactive widgets (buttons, taps, drags)

**Status:** 85% complete - Core infrastructure ready!

#### Week 7 Tasks - COMPLETED ✅:
- [x] Create PointerEvent types in flui_types/events.rs
- [x] Create HitTestResult + HitTestEntry
- [x] Add hit_test() method to RenderObject trait (3-level system)
- [x] Implement hit testing in RenderBox + RenderProxyBox
- [x] Implement hit testing in 5 specialized RenderObjects (ClipRRect, Opacity, Transform, etc.)
- [x] Integrate pointer events with eframe (FluiApp::process_pointer_events)
- [x] Add PipelineOwner::dispatch_pointer_event with hit testing
- [x] Complete GestureDetector widget with builder pattern
- [x] Export GestureDetector in flui_widgets prelude
- [x] Add 2 tests for GestureDetector (on_tap, on_tap_down)
- [x] **203 tests passing** in flui_rendering (hit testing integration)

**Dependencies:**
- ✅ flui_types (PointerEvent types)
- ✅ flui_core (RenderObject trait)
- ✅ flui_rendering (RenderBox)
- ✅ flui_app (eframe integration)

**Success Criteria:**
- [ ] Can detect tap on any widget
- [ ] Hit testing correctly identifies widget under cursor
- [ ] Button widget responds to clicks
- [ ] Counter example works with buttons (not just auto-increment)

#### Week 8 Tasks:
- [ ] Add remaining gesture recognizers to flui_gestures:
  - [ ] TapGestureRecognizer (single tap)
  - [ ] DoubleTapGestureRecognizer
  - [ ] LongPressGestureRecognizer
  - [ ] DragGestureRecognizer (HorizontalDrag, VerticalDrag, PanDrag)
- [ ] Implement GestureArena (for gesture conflict resolution)
- [ ] Add VelocityTracker (for fling gestures)
- [ ] Create Draggable widget
- [ ] Create gesture_demo example

**Estimated Lines:** ~2500 lines, ~80 tests

---

### Phase 2: Text Rendering (Week 9-10)

**Goal:** Display text with proper styling and layout

**Dependencies:**
- ✅ flui_types (TextStyle, TextSpan complete)
- ⏳ flui_painting (TextPainter - to create)
- ⏳ flui_rendering (RenderParagraph - to create)

#### Week 9 Tasks:
- [ ] Create flui_painting crate
- [ ] Implement TextPainter using egui's text layout
- [ ] Create RenderParagraph in flui_rendering
- [ ] Implement line breaking, wrapping
- [ ] Handle TextAlign, TextDirection

#### Week 10 Tasks:
- [ ] Create Text widget in flui_widgets
- [ ] Create RichText widget (with TextSpan)
- [ ] Create DefaultTextStyle (InheritedWidget)
- [ ] Support multi-line text
- [ ] Create text_demo example

**Estimated Lines:** ~3000 lines, ~100 tests

**Success Criteria:**
- [ ] Can display single-line text
- [ ] Can display multi-line text with wrapping
- [ ] TextStyle properties work (color, size, weight)
- [ ] RichText with multiple TextSpans works
- [ ] Text alignment works

---

### Phase 3: Image Rendering (Week 11-12)

**Goal:** Display images with proper sizing and caching

**Dependencies:**
- ✅ flui_types (BoxFit, ImageRepeat complete)
- ⏳ flui_painting (ImageProvider - to create)
- ⏳ flui_rendering (RenderImage - to create)

#### Week 11 Tasks:
- [ ] Implement ImageProvider trait in flui_painting
- [ ] Create MemoryImage, NetworkImage, AssetImage
- [ ] Implement ImageCache
- [ ] Create RenderImage in flui_rendering
- [ ] Implement BoxFit sizing logic

#### Week 12 Tasks:
- [ ] Create Image widget in flui_widgets
- [ ] Support DecorationImage in BoxDecoration
- [ ] Implement ImageRepeat modes
- [ ] Handle image loading states
- [ ] Create image_demo example

**Estimated Lines:** ~2500 lines, ~80 tests

---

### Phase 4: Scrolling & Slivers (Week 13-16)

**Goal:** Scrollable lists and grids

**Dependencies:**
- ✅ flui_types (SliverConstraints, SliverGeometry complete)
- ✅ flui_gestures (DragGestureRecognizer from Phase 1)
- ⏳ flui_rendering (Sliver RenderObjects - to create)

#### Week 13-14: Sliver Infrastructure
- [ ] Create RenderSliver base in flui_rendering
- [ ] Implement RenderViewport
- [ ] Create ScrollPosition, ScrollController
- [ ] Implement RenderSliverList
- [ ] Implement RenderSliverToBoxAdapter

#### Week 15-16: Scrollable Widgets
- [ ] Create SingleChildScrollView widget
- [ ] Create ListView widget (+ ListView.builder)
- [ ] Create GridView widget
- [ ] Implement scroll physics (BouncingScrollPhysics, ClampingScrollPhysics)
- [ ] Create scrolling_demo example

**Estimated Lines:** ~5000 lines, ~150 tests

---

### Phase 5: Animation Controllers (Week 17-18)

**Goal:** Animated widgets and transitions

**Dependencies:**
- ✅ flui_types (Curve, Tween complete)
- ⏳ flui_scheduler (Ticker - to create)
- ⏳ flui_animation (AnimationController - to create)

#### Week 17 Tasks:
- [ ] Create flui_scheduler crate
- [ ] Implement Ticker + TickerProvider
- [ ] Integrate with eframe frame callbacks
- [ ] Create flui_animation crate
- [ ] Implement Animation<T> trait
- [ ] Implement AnimationController

#### Week 18 Tasks:
- [ ] Implement CurvedAnimation
- [ ] Create AnimatedContainer widget
- [ ] Create AnimatedOpacity widget
- [ ] Create AnimatedAlign widget
- [ ] Create animation_demo example

**Estimated Lines:** ~3000 lines, ~100 tests

---

### Phase 6: Input Widgets (Week 19-22)

**Goal:** Forms, text input, checkboxes, sliders

**Dependencies:**
- ✅ Phase 1 (gestures)
- ✅ Phase 2 (text rendering)
- ⏳ flui_service (text input platform integration - to create)

#### Week 19-20: Text Input
- [ ] Create RenderEditableLine in flui_rendering
- [ ] Implement text cursor positioning
- [ ] Implement text selection
- [ ] Create TextField widget
- [ ] Handle keyboard input from eframe

#### Week 21-22: Form Inputs
- [ ] Create Checkbox widget
- [ ] Create Radio widget
- [ ] Create Switch widget
- [ ] Create Slider widget
- [ ] Create Form + FormField widgets
- [ ] Create input_demo example

**Estimated Lines:** ~4000 lines, ~120 tests

---

### Phase 7: Material Basics (Week 23-26)

**Goal:** Core Material Design widgets

**Dependencies:**
- ✅ All previous phases
- ⏳ flui_material crate (to create)

#### Week 23-24: Material Foundation
- [ ] Create flui_material crate
- [ ] Implement Material widget (base)
- [ ] Implement InkWell, InkResponse (ripple effects)
- [ ] Create ThemeData
- [ ] Create ColorScheme
- [ ] Create MaterialApp

#### Week 25-26: Material Widgets
- [ ] Create TextButton, ElevatedButton, OutlinedButton
- [ ] Create IconButton, FloatingActionButton
- [ ] Create Card widget
- [ ] Create Scaffold + AppBar
- [ ] Create BottomNavigationBar
- [ ] Create material_demo example

**Estimated Lines:** ~6000 lines, ~180 tests

---

### Phase 8: Advanced Material (Week 27-30)

**Goal:** Dialogs, navigation, complex components

#### Week 27-28: Dialogs & Overlays
- [ ] Create Dialog, AlertDialog, SimpleDialog
- [ ] Create BottomSheet, ModalBottomSheet
- [ ] Create SnackBar
- [ ] Implement Overlay system
- [ ] Create dialogs_demo example

#### Week 29-30: Navigation & Complex Components
- [ ] Create Drawer, EndDrawer
- [ ] Create TabBar + TabBarView
- [ ] Create ExpansionTile, ListTile
- [ ] Create Chip variants
- [ ] Create DataTable
- [ ] Create material_advanced_demo example

**Estimated Lines:** ~8000 lines, ~200 tests

---

## 📈 Progress Tracking

### Overall Framework Completion

| Area | Status | Completion | Lines | Tests |
|------|--------|------------|-------|-------|
| **Foundation** | ✅ Complete | 100% | ~13,677 | ~524 |
| **Core Architecture** | ✅ Complete | 100% | ~4,000 | ~49 |
| **Rendering** | 🚧 In Progress | 33% | ~6,000 | ~117 |
| **Widgets** | 🚧 In Progress | 2% | ~7,000 | ~292 |
| **Gestures** | 🚧 Started | 2% | ~500 | ~0 |
| **Animation** | ⏳ Not Started | 0% | 0 | 0 |
| **Painting** | ⏳ Not Started | 0% | 0 | 0 |
| **Material** | ⏳ Not Started | 0% | 0 | 0 |
| **TOTAL** | 🚧 In Progress | **~15%** | **~31,177** | **~982** |

### GLOSSARY Coverage

| Category | Total Types | Implemented | % |
|----------|-------------|-------------|---|
| **flui_types** | ~237 | ~237 | 100% |
| **flui_foundation** | ~100 | ~100 | 100% |
| **flui_core** | ~40 | ~40 | 100% |
| **flui_rendering** | ~550 | ~14 | 3% |
| **flui_widgets** | ~1000+ | ~17 | 2% |
| **flui_gestures** | ~125 | ~5 | 4% |
| **flui_animation** | ~60 | ~0 | 0% |
| **flui_painting** | ~160 | ~0 | 0% |
| **flui_material** | ~1000+ | ~0 | 0% |
| **flui_scheduler** | ~12 | ~0 | 0% |
| **flui_semantics** | ~43 | ~0 | 0% |
| **flui_service** | ~530 | ~0 | 0% |
| **TOTAL** | **~3,500+** | **~413** | **~12%** |

---

## 🔑 Key Architectural Principles

### 1. Bottom-Up Development
- **Never skip layers:** Types → Core → Rendering → Widgets → Material
- **Complete dependencies first:** Don't start Material before Rendering is solid
- **Avoid rework:** Proper foundation prevents future refactoring

### 2. Three-Tree Architecture
- **Widget Tree:** Immutable configuration (user-facing API)
- **Element Tree:** Mutable lifecycle management (rebuilds, state)
- **RenderObject Tree:** Layout and painting (performance-critical)

### 3. Type Safety with Downcasting
- **DynClone:** Widget trait object cloning
- **Downcast:** Safe Widget type conversions
- **DowncastSync:** Thread-safe Element/RenderObject conversions

### 4. Performance First
- **Use workspace dependencies:** ahash, parking_lot (not std)
- **Minimize allocations:** Clone only when necessary
- **Lock-free when possible:** RwLock with careful deadlock avoidance

### 5. Testing Discipline
- **Every type gets tests:** Minimum 1 test per public method
- **Integration tests:** Real widget trees, not just unit tests
- **Examples as tests:** Every major feature gets an example

---

## 📝 Notes

### Current Week (Week 7)
- **Focus:** Event handling infrastructure
- **Goal:** Make widgets interactive
- **Blocker:** Need hit testing in RenderObject
- **Next Milestone:** Working Button widget with tap callback

### Dependencies to Watch
- **egui integration:** May need updates for advanced features
- **eframe input handling:** Currently basic, may need enhancement
- **Text rendering:** Will likely use egui's TextLayout APIs
- **Image loading:** May use image crate or egui's built-in support

### Future Considerations
- **Platform abstraction:** Eventually separate egui/eframe into backend trait
- **WASM support:** Keep wasm32 compatibility in mind
- **Accessibility:** Implement semantic system after core features stable
- **Performance profiling:** Add benchmarks after Phase 4 (scrolling)

---

## 🎉 Achievements

### Week 1-2: Foundation Complete
- ✅ 13,677 lines of foundation types
- ✅ 524 comprehensive tests
- ✅ All 11 GLOSSARY type modules implemented

### Week 3-4: Core Architecture Complete
- ✅ Three-tree architecture (Widget → Element → RenderObject)
- ✅ StatefulWidget with State lifecycle
- ✅ InheritedWidget with dependency tracking
- ✅ BuildContext with Flutter-style API

### Week 5-6: Widget Library Started
- ✅ 17 essential widgets implemented
- ✅ 292 tests (exceeded plan by 2.5x!)
- ✅ RenderObjectWidget integration working
- ✅ Builder pattern with bon
- ✅ Comprehensive documentation (WIDGET_GUIDELINES.md, etc.)

### Week 7: Event Handling Infrastructure - COMPLETE ✅
- ✅ PointerEvent type system (Down, Up, Move, Enter, Exit, Cancel)
- ✅ HitTestResult + HitTestEntry infrastructure
- ✅ RenderObject hit testing (3-level system: hit_test, hit_test_self, hit_test_children)
- ✅ Hit testing in RenderBox + 5 specialized RenderObjects
- ✅ FluiApp pointer event processing (egui → Flui conversion)
- ✅ PipelineOwner::dispatch_pointer_event with full hit testing
- ✅ GestureDetector widget with builder pattern (on_tap, on_tap_down, on_tap_up, on_tap_cancel)
- ✅ 2 GestureDetector tests passing
- ✅ **203 total tests** passing in flui_rendering
- ✅ Complete eframe integration for mouse events

**Week 7 Stats:**
- **12 files** modified
- **~600 lines** of new code
- **5 new tests** (2 GestureDetector + 3 hit testing)
- **203 tests** passing in flui_rendering
- **Phase 1:** 85% complete

---

**Last Updated:** Week 7 End (2025-01-19)
**Next Review:** Week 8 (Advanced gesture recognizers)
**Current Focus:** Phase 1 - Event Handling (85% → 100%)
