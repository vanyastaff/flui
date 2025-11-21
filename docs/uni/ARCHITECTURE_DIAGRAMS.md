# FLUI Architecture Diagrams

This document provides detailed visual diagrams of the pipeline and binding architecture.

## 1. Complete Frame Lifecycle

```
┌─────────────────────────────────────────────────────────────────────┐
│ FRAME LIFECYCLE (Current vs Ideal)                                  │
└─────────────────────────────────────────────────────────────────────┘

CURRENT STATE (Partially Implemented):
─────────────────────────────────────
User Input Event
    ↓
[event_loop] RedrawRequested (winit)
    ↓
WgpuEmbedder::render_frame()  [MISSING: scheduler.begin_frame()]
    ↓
RendererBinding::draw_frame(constraints)
    ↓
PipelineOwner::build_frame(constraints)
    ├─ Phase 1: BUILD
    │   ├─ flush_rebuild_queue()  [FIRST FLUSH]
    │   ├─ flush_batch()
    │   ├─ rebuild_dirty_parallel() ✓
    │   └─ Loop until no dirty elements
    │
    ├─ Phase 2: LAYOUT
    │   ├─ Scan needs_layout flags
    │   ├─ compute_layout()
    │   └─ Mark for paint
    │
    └─ Phase 3: PAINT
        ├─ generate_layers()
        └─ Return CanvasLayer
    ↓
GpuRenderer::render()
    ↓
[frame_buffer] Present to screen
    ↓
WgpuEmbedder  [MISSING: scheduler.end_frame()]


IDEAL STATE (After Refactoring):
────────────────────────────────
User Input Event
    ↓
[event_loop] RedrawRequested (winit)
    ↓
WgpuEmbedder::render_frame()
    ├─ scheduler.begin_frame()  ✓ [ADDED]
    │   └─ [PERSISTENT CALLBACKS]
    │       └─ flush_rebuild_queue()  [SINGLE FLUSH]
    │   └─ [ONE-TIME CALLBACKS]
    │       └─ animations, timers, etc.
    │
    ├─ RendererBinding::draw_frame(constraints)  [without flush_rebuild_queue]
    │   └─ PipelineOwner::build_frame(constraints)
    │       ├─ Phase 1: BUILD (uses pre-flushed queue)
    │       ├─ Phase 2: LAYOUT
    │       └─ Phase 3: PAINT
    │
    ├─ GpuRenderer::render()
    │
    ├─ [frame_buffer] Present to screen
    │
    └─ scheduler.end_frame()  ✓ [ADDED]
        └─ Record timing, update budgets
```

## 2. Element Tree and Dirty Tracking

```
┌─────────────────────────────────────────────────────────────────────┐
│ ELEMENT TREE WITH DIRTY STATE                                       │
└─────────────────────────────────────────────────────────────────────┘

                        ┌──────────────┐
                        │  PipelineOwner
                        └──────────────┘
                              │
                    ┌─────────┼─────────┐
                    │         │         │
                    ↓         ↓         ↓
              FrameCoor  RootManager  RebuildQueue
              dinator                 (from signals)
                │
        ┌───────┼───────┐
        │       │       │
        ↓       ↓       ↓
      Build  Layout  Paint
     Pipeline Pipeline Pipeline
        │       │       │
        ↓       ↓       ↓
     dirty_  dirty_  dirty_
    elements set     set
               (lock-free atomic)

                    ElementTree
                    ───────────
                    Slab<Element>
                         │
        ┌────────────────┼────────────────┐
        │                │                │
        ↓                ↓                ↓
    Component       Render           Provider
    Element         Element          Element
    (dirty)         (RenderState)    (dirty)
        │                │
        │         ┌───────┴────────┐
        │         │                │
        │         ↓                ↓
        │     RenderState      RenderNode
        │     ──────────       ─────────
        │     • size           • leaf
        │     • offset         • single
        │     • needs_layout   • multi
        │     • needs_paint
        │
        └─→ [Dirty Flag 1]
            [BuildPipeline]
            
        └─→ RenderState flags
            [Layout/Paint]
```

## 3. Three-Phase Pipeline Detailed View

```
┌─────────────────────────────────────────────────────────────────────┐
│ PHASE 1: BUILD (Rebuild Components)                                 │
└─────────────────────────────────────────────────────────────────────┘

Input: RebuildQueue (populated by signals)
              │
              ↓
         [Scheduler Callback]
         flush_rebuild_queue()
              │
        ┌─────┴─────┐
        │           │
    [Batch]    [Direct]
        │           │
        └─────┬─────┘
              │
              ↓
    BuildPipeline.dirty_elements
    [Vec<(ElementId, depth)>]
              │
              ├─ Sort by depth (parents first)
              ├─ Deduplicate
              │
              ↓
    For each dirty ComponentElement:
    1. Extract hook context
    2. Build new child view
    3. Reconcile old → new child
    4. Clear dirty flag
    5. Recursively schedule child rebuilds


┌─────────────────────────────────────────────────────────────────────┐
│ PHASE 2: LAYOUT (Compute Sizes)                                     │
└─────────────────────────────────────────────────────────────────────┘

Input: needs_layout flags + LayoutPipeline.dirty set
              │
              ├─ Scan all RenderElements for needs_layout
              ├─ Add to dirty set
              │
              ↓
    LayoutPipeline.compute_layout()
    [Vec<ElementId> with dirty layout requests]
              │
    For each dirty RenderElement:
    1. Get constraints (from RenderState or provided)
    2. Call render.layout()
    3. Store computed size in RenderState
    4. Clear needs_layout flag
    5. Mark for paint
              │
              ↓
    RenderState:
    • size (from layout)
    • offset (from parent)
    • needs_paint = true


┌─────────────────────────────────────────────────────────────────────┐
│ PHASE 3: PAINT (Generate Layers)                                    │
└─────────────────────────────────────────────────────────────────────┘

Input: needs_paint flags + PaintPipeline.dirty set
              │
              ├─ Add all marked_for_paint from layout phase
              │
              ↓
    PaintPipeline.generate_layers()
    [Vec<ElementId> with dirty paint requests]
              │
    For each dirty RenderElement:
    1. Get offset from RenderState
    2. Call render.paint()
    3. Collect canvas/layers
    4. Clear needs_paint flag
              │
              ↓
    Output: CanvasLayer tree
    Ready for GPU rendering
```

## 4. Signal to Rebuild Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│ SIGNAL → REBUILD FLOW                                               │
└─────────────────────────────────────────────────────────────────────┘

User Code:
    count.set(42)  [Signal<i32>::set()]
         │
         ├─ Update internal value
         │
         ├─ Get rebuild_queue from BuildContext
         │
         └─ schedule rebuild
              │
              ↓
    RebuildQueue::push(element_id, depth)
         │
         ├─ Insert into HashSet
         │
         ├─ Deduplicate
         │
         └─ Mark for rebuild
              │
              ↓
    Frame Start:
    Scheduler callback → flush_rebuild_queue()
         │
         ↓
    BuildPipeline::flush_rebuild_queue()
         │
         ├─ Drain RebuildQueue
         │
         ├─ Add to dirty_elements
         │
         └─ Sort by depth
              │
              ↓
    BuildPipeline loop:
    1. rebuild_component() calls View::build()
    2. New child element created
    3. Old child reconciled
    4. New child marked for layout
         │
         ↓
    Layout Phase:
    Mark all children for layout
         │
         ↓
    Paint Phase:
    Generate new layers
         │
         ↓
    GPU Render:
    Screen updated!
```

## 5. Component Rebuild Detailed

```
┌─────────────────────────────────────────────────────────────────────┐
│ COMPONENT REBUILD IN DETAIL                                         │
└─────────────────────────────────────────────────────────────────────┘

Current Element: ComponentElement
         │
         ├─ [STAGE 1] Read phase (write lock)
         │      │
         │      ├─ Check is_dirty() flag
         │      │
         │      ├─ Get old_child_id
         │      │
         │      └─ Extract/Create HookContext
         │           (preserved across rebuilds!)
         │
         └─ Release write lock
              │
              ↓
         ├─ [STAGE 2] Build phase (no lock)
         │      │
         │      ├─ Setup thread-local BuildContext
         │      │
         │      ├─ with_build_context() setup
         │      │
         │      ├─ View::build() called
         │      │   (may trigger more rebuilds via signal.set())
         │      │
         │      └─ Returns new_element
         │
         └─ Acquire write lock again
              │
              ↓
         ├─ [STAGE 3] Reconcile phase (write lock)
         │      │
         │      ├─ Insert new_element (BEFORE removing old)
         │      │
         │      ├─ Update component's child reference
         │      │
         │      ├─ Remove old_element (if existed)
         │      │
         │      ├─ Clear dirty flag
         │      │
         │      └─ New element implicitly marked for layout
         │           (via request_layout in tree.insert())
         │
         └─ Release write lock

Benefits of Three-Stage Approach:
──────────────────────────────────
• Minimal lock holding time
• Expensive View::build() happens without locks
• Signal changes during build don't cause deadlocks
• New element's children are already valid (inserted before removal)
• HookContext persists across rebuilds (state preserved)
```

## 6. RenderState State Machine

```
┌─────────────────────────────────────────────────────────────────────┐
│ RENDERSTATE FLAGS STATE MACHINE                                     │
└─────────────────────────────────────────────────────────────────────┘

                     ┌──────────────────┐
                     │  RenderElement   │
                     │    Created       │
                     └──────────────────┘
                            │
                            │ (insert into tree)
                            ↓
                     ┌──────────────────┐
                     │  needs_layout=T  │
                     │  needs_paint=T   │
                     │  size=None       │
                     └──────────────────┘
                            │
                            │ (Layout Phase)
                            ├─ Scan for flag
                            ├─ compute_layout()
                            ↓
                     ┌──────────────────┐
                     │  needs_layout=F  │
                     │  needs_paint=T   │
                     │  size=Some(w, h) │
                     │  offset=Some(x,y)│
                     └──────────────────┘
                            │
                            │ (Paint Phase)
                            ├─ generate_layers()
                            ├─ paint()
                            ↓
                     ┌──────────────────┐
                     │  needs_layout=F  │
                     │  needs_paint=F   │
                     │  size=Some(w, h) │
                     │  offset=Some(x,y)│
                     └──────────────────┘
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
   [Layout Change]    [Paint Change]    [Unchanged]
        │                   │                   │
        ↓                   ↓                   │
   mark_needs        mark_needs_paint          │
   _layout()             ()                    │
        │                   ↓                  │
        └─────────────┬─────────────────────────┘
                      │
                      ↓ (Next Frame)
                   Layout Phase
                      │
                      ↓
                   Paint Phase
```

## 7. Binding Initialization Sequence

```
┌─────────────────────────────────────────────────────────────────────┐
│ APPBINDING INITIALIZATION SEQUENCE                                  │
└─────────────────────────────────────────────────────────────────────┘

run_app(root_widget)
    │
    ├─ 1. Initialize logging
    │
    ├─ 2. AppBinding::ensure_initialized()
    │      │
    │      ├─ Create GestureBinding
    │      │   ├─ EventRouter
    │      │   └─ init()
    │      │
    │      ├─ Create SchedulerBinding (60 FPS)
    │      │   ├─ flui_scheduler::Scheduler
    │      │   └─ init()
    │      │
    │      ├─ Create PipelineOwner
    │      │   ├─ ElementTree (empty)
    │      │   ├─ BuildPipeline
    │      │   ├─ LayoutPipeline
    │      │   ├─ PaintPipeline
    │      │   └─ RootManager
    │      │
    │      ├─ Create RendererBinding(pipeline_owner)
    │      │   └─ init()
    │      │
    │      ├─ Create PipelineBinding(pipeline_owner)
    │      │   └─ init()
    │      │
    │      ├─ wire_up() ← CRITICAL
    │      │   └─ Add persistent frame callback
    │      │       └─ flush_rebuild_queue()
    │      │
    │      └─ Return Arc<AppBinding>
    │
    ├─ 3. PipelineBinding::attach_root_widget(root_widget)
    │      │
    │      ├─ Create ComponentElement wrapper
    │      ├─ Insert into tree as root
    │      ├─ Schedule for build (mark dirty)
    │      └─ Request layout (mark needs_layout)
    │
    ├─ 4. Create EventLoop
    │
    ├─ 5. Wait for Resumed event (Android) or create window immediately (Desktop)
    │
    ├─ 6. Create WgpuEmbedder
    │      ├─ Create winit window
    │      ├─ Initialize GPU context
    │      └─ Start frame loop
    │
    ├─ 7. Frame loop running
    │      For each RedrawRequested:
    │      → render_frame()
    │          → scheduler.begin_frame()
    │          → draw_frame()
    │          → render()
    │          → scheduler.end_frame()
    │
    └─ ∞ Event loop running
```

## 8. Memory Layout: Pipeline Owner

```
┌─────────────────────────────────────────────────────────────────────┐
│ PIPELINEOWNER MEMORY LAYOUT                                         │
└─────────────────────────────────────────────────────────────────────┘

PipelineOwner (stack or heap)
├─ tree: Arc<RwLock<ElementTree>>  [8 bytes: ptr to Arc]
│
├─ coordinator: FrameCoordinator
│  ├─ build: BuildPipeline
│  │  ├─ dirty_elements: Vec<(ElementId, usize)>  [24 bytes: ptr, len, cap]
│  │  ├─ build_count: usize  [8 bytes]
│  │  ├─ in_build_scope: bool  [1 byte, + 7 padding]
│  │  ├─ build_locked: bool  [1 byte, + 7 padding]
│  │  ├─ batcher: Option<BuildBatcher>  [variable]
│  │  └─ rebuild_queue: RebuildQueue  [8 bytes: Arc ptr]
│  │
│  ├─ layout: LayoutPipeline
│  │  ├─ dirty: LockFreeDirtySet  [atomic + storage]
│  │  └─ parallel_enabled: bool
│  │
│  ├─ paint: PaintPipeline
│  │  ├─ dirty: LockFreeDirtySet
│  │  └─ optimize_layers: bool
│  │
│  └─ budget: Arc<Mutex<FrameBudget>>  [8 bytes]
│
├─ root_mgr: RootManager
│  └─ root_id: Option<ElementId>  [8 bytes]
│
├─ rebuild_queue: RebuildQueue
│  └─ inner: Arc<Mutex<HashSet<(ElementId, usize)>>>  [8 bytes]
│
├─ on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>  [16 bytes]
│
├─ frame_counter: u64  [8 bytes]
│
└─ features: PipelineFeatures
   ├─ metrics: Option<Arc<PipelineMetrics>>
   ├─ recovery: Option<Arc<ErrorRecovery>>
   ├─ cancellation: Option<Arc<CancellationToken>>
   ├─ frame_buffer: Option<Arc<TripleBuffer<...>>>
   └─ hit_test_cache: Option<Arc<HitTestCache>>

Total: ~250-300 bytes + feature storage
(small enough for stack allocation)
```

## 9. Data Flow: Signal Change to Screen

```
┌─────────────────────────────────────────────────────────────────────┐
│ END-TO-END DATA FLOW: SIGNAL CHANGE → SCREEN UPDATE                 │
└─────────────────────────────────────────────────────────────────────┘

APPLICATION CODE:
─────────────────
    count.set(42)
         │
         ↓
    SIGNAL LAYER (use_signal hook):
    ─────────────────────────────
    Signal::set(value)
         │
         ├─ Update Arc<Mutex<T>> value
         │
         ├─ Get rebuild_queue (from BuildContext)
         │
         ├─ Get element_id (component owning the signal)
         │
         └─ rebuild_queue.push(element_id, depth)
              │
              ↓
    REBUILD QUEUE (shared state):
    ──────────────────────────────
    RebuildQueue: Arc<Mutex<HashSet>>
         │
         ├─ Holds pending rebuilds across frames
         │
         └─ Signal can call from any thread ✓
              │
              ↓
    FRAME START:
    ────────────
    Scheduler.begin_frame()
         │
         ├─ Execute persistent callbacks
         │
         └─ flush_rebuild_queue()
              │
              ↓
    PIPELINE PHASE 1: BUILD
    ─────────────────────────
    BuildPipeline.flush_rebuild_queue()
         │
         ├─ drain() → Vec<(ElementId, depth)>
         │
         ├─ Sort by depth
         │
         ├─ rebuild_component(element_id)
         │   ├─ Extract HookContext (with signal state!)
         │   ├─ Call View::build()  [new element from signal.get()]
         │   ├─ Reconcile child
         │   └─ Mark for layout
         │
         └─ (loop until queue empty)
              │
              ↓
    PIPELINE PHASE 2: LAYOUT
    ────────────────────────
    LayoutPipeline.compute_layout()
         │
         ├─ Scan needs_layout flags
         │
         ├─ Layout all marked elements
         │
         └─ Mark for paint
              │
              ↓
    PIPELINE PHASE 3: PAINT
    ───────────────────────
    PaintPipeline.generate_layers()
         │
         ├─ Paint all marked elements
         │
         └─ Return CanvasLayer tree
              │
              ↓
    GPU RENDERING:
    ──────────────
    GpuRenderer::render(layer)
         │
         ├─ Tessellate shapes
         │
         ├─ Rasterize text
         │
         ├─ Submit to GPU
         │
         └─ Present to screen ✓
              │
              ↓
    FRAME END:
    ──────────
    Scheduler.end_frame()
         │
         └─ Record timing stats

RESULT: Screen updated with new value!
────────────────────────────────────
All within frame time budget (16.67ms @ 60fps)
```

## 10. Dirty Tracking: Built-in Deduplication

```
┌─────────────────────────────────────────────────────────────────────┐
│ DIRTY TRACKING DEDUPLICATION                                        │
└─────────────────────────────────────────────────────────────────────┘

Scenario: Signal changes multiple times in same frame

    count.set(1)
    count.set(2)
    count.set(3)  ← Same element, three rebuilds requested
         │
         ├─ rebuild_queue.push(elem_id, 0)  ✓ Added
         ├─ rebuild_queue.push(elem_id, 0)  → Deduplicated (HashSet)
         └─ rebuild_queue.push(elem_id, 0)  → Deduplicated (HashSet)
              │
              ↓
    rebuild_queue.len() = 1  [SAVED 2 builds!]
              │
              ↓
    Frame start: flush_rebuild_queue()
         │
         ├─ drain() → [(elem_id, 0)]  [only once]
         │
         └─ rebuild happens once  [not three times]
              │
              ↓
    Result: Signal state = 3, but rebuild only 1x

Benefit: O(1) deduplication vs O(n) filtering
──────────
    Without dedup:
    rebuild_queue: [elem_id, elem_id, elem_id, ...]
    process: rebuild, rebuild, rebuild
    
    With dedup (HashSet):
    rebuild_queue: {elem_id, ...}
    process: rebuild  [once]

Cost: HashSet insertion O(1) vs Vec push O(1) (same!)
```

## 11. Lock Contention Analysis

```
┌─────────────────────────────────────────────────────────────────────┐
│ LOCK CONTENTION ANALYSIS                                            │
└─────────────────────────────────────────────────────────────────────┘

SIGNAL CHANGE (from any thread):
────────────────────────────────
    signal.set(value)
        │
        ├─ Arc<Mutex<T>>.lock()  ← Very short: just update value
        │   └─ Hold time: 1 microsecond
        │
        └─ rebuild_queue.push(elem_id, depth)
            └─ Arc<Mutex<HashSet>>.lock()  ← Insert into set
                └─ Hold time: ~100 nanoseconds


FRAME START (scheduler callback):
─────────────────────────────────
    flush_rebuild_queue()
        │
        └─ rebuild_queue.drain()
            └─ Arc<Mutex<HashSet>>.lock()  ← Take ownership of set
                └─ Hold time: 1 microsecond


BUILD PHASE (single-threaded):
──────────────────────────────
    rebuild_component(elem_id)
        │
        ├─ tree.write()  ← RwLock (write lock)
        │   ├─ Extract component data
        │   └─ Release write lock (critical section ~10 microseconds)
        │
        ├─ View::build()  [NO LOCKS - can be expensive]
        │   └─ May take milliseconds
        │
        └─ tree.write()  ← RwLock again (write lock)
            └─ Reconcile child (critical section ~1 microsecond)


CONTENTION SUMMARY:
───────────────────
✓ Good: Signal.set() has minimal lock time
✓ Good: Build critical sections are small
✓ Good: Layout/Paint use lock-free dirty sets
✓ Good: HookContext Mutex isn't hot path

⚠️ Potential: tree.write() during rebuild (but short)
✓ Mitigated: RwLock allows parallel frame rendering (future)
```

---

## Summary

These diagrams show:

1. **Frame Lifecycle**: Current state vs ideal after refactoring
2. **Element Tree**: How dirty tracking works across phases
3. **Three-Phase Pipeline**: Build, Layout, Paint in detail
4. **Signal Flow**: Complete path from user code to screen
5. **Component Rebuild**: Three-stage locking strategy
6. **RenderState**: State machine for layout/paint flags
7. **Binding Initialization**: Startup sequence
8. **Memory Layout**: PipelineOwner structure
9. **Data Flow**: End-to-end example
10. **Deduplication**: How RebuildQueue saves work
11. **Lock Analysis**: Contention and performance

The diagrams clarify the architecture and help identify the issues documented in the main architecture document.
