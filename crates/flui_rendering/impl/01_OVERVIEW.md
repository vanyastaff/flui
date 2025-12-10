# Flutter RenderObject Architecture Overview

This document provides a comprehensive overview of Flutter's rendering architecture based on `object.dart`.

## Key Components

```
┌────────────────────────────────────────────────────────────────────┐
│                        Pipeline Owner                               │
│  (Manages rendering pipeline phases: layout, paint, semantics)     │
└────────────────────┬───────────────────────────────────────────────┘
                     │ owns
                     ▼
┌────────────────────────────────────────────────────────────────────┐
│                        RenderObject                                 │
│  (Base class for all render tree nodes)                            │
│  - ParentData (parent-specific data on children)                   │
│  - Constraints (layout input)                                      │
│  - Layer (compositing)                                             │
└────────────────────┬───────────────────────────────────────────────┘
                     │ uses
                     ▼
┌────────────────────────────────────────────────────────────────────┐
│                      PaintingContext                                │
│  (Canvas wrapper with child painting support)                      │
│  - Manages PictureLayers                                           │
│  - Handles repaint boundaries                                      │
│  - Provides clipping, transform, opacity                           │
└────────────────────────────────────────────────────────────────────────┘
```

## Pipeline Phases

The rendering pipeline consists of four main phases executed in order:

```
┌─────────────┐    ┌──────────────────────┐    ┌─────────────┐    ┌──────────────┐
│   LAYOUT    │ -> │ COMPOSITING BITS     │ -> │    PAINT    │ -> │  SEMANTICS   │
│  flushLayout│    │ flushCompositingBits │    │ flushPaint  │    │flushSemantics│
└─────────────┘    └──────────────────────┘    └─────────────┘    └──────────────┘
       │                    │                        │                   │
       ▼                    ▼                        ▼                   ▼
  Compute size        Update needs-         Record paint         Update a11y
  and position        compositing           commands into        tree for
  of render           bits for each         Picture/Layers       assistive
  objects             node                                       technology
```

## Core Classes

| Class | Purpose |
|-------|---------|
| `RenderObject` | Abstract base for render tree nodes |
| `PipelineOwner` | Orchestrates rendering phases |
| `PaintingContext` | Canvas and layer management during paint |
| `Constraints` | Abstract layout input |
| `ParentData` | Child-specific data stored by parent |

## Architecture Diagram

```
                              ┌─────────────────────┐
                              │   PipelineManifold  │
                              │   (global config)   │
                              └──────────┬──────────┘
                                         │ attach
                                         ▼
    ┌────────────────────────────────────────────────────────────────┐
    │                      PipelineOwner (root)                      │
    │  _nodesNeedingLayout: List<RenderObject>                       │
    │  _nodesNeedingCompositingBitsUpdate: List<RenderObject>        │
    │  _nodesNeedingPaint: List<RenderObject>                        │
    │  _nodesNeedingSemantics: Set<RenderObject>                     │
    └────────────────────────────────┬───────────────────────────────┘
                                     │ rootNode
                                     ▼
    ┌────────────────────────────────────────────────────────────────┐
    │                      RenderObject Tree                         │
    │                                                                │
    │     ┌─────────────┐                                           │
    │     │ RenderView  │ (root, repaint boundary)                  │
    │     └──────┬──────┘                                           │
    │            │                                                   │
    │     ┌──────┴──────┐                                           │
    │     ▼             ▼                                           │
    │  ┌──────┐     ┌──────┐                                        │
    │  │RBox 1│     │RBox 2│ (repaint boundary)                     │
    │  └──┬───┘     └──┬───┘                                        │
    │     │            │                                             │
    │   ┌─┴─┐        ┌─┴─┐                                          │
    │   ▼   ▼        ▼   ▼                                          │
    │  ┌─┐ ┌─┐      ┌─┐ ┌─┐                                         │
    │  │ │ │ │      │ │ │ │  (leaf nodes)                           │
    │  └─┘ └─┘      └─┘ └─┘                                         │
    └────────────────────────────────────────────────────────────────┘
```

## Dirty Flags and Marking

Each `RenderObject` maintains several dirty flags:

| Flag | Marked By | Cleared By |
|------|-----------|------------|
| `_needsLayout` | `markNeedsLayout()` | `layout()` / `_layoutWithoutResize()` |
| `_needsCompositingBitsUpdate` | `markNeedsCompositingBitsUpdate()` | `_updateCompositingBits()` |
| `_needsPaint` | `markNeedsPaint()` | `_paintWithContext()` |
| `_needsCompositedLayerUpdate` | `markNeedsCompositedLayerUpdate()` | `_paintWithContext()` |

## Key Design Patterns

### 1. Relayout Boundaries
- Isolate layout changes to subtrees
- Determined by `parentUsesSize`, `sizedByParent`, `constraints.isTight`

### 2. Repaint Boundaries  
- Isolate paint changes to subtrees
- Controlled by `isRepaintBoundary` property

### 3. Deferred Operations
- Dirty nodes collected into lists
- Processed during flush phases
- Batches work for efficiency

### 4. Compositing Layers
- Enable hardware acceleration
- Support clip, transform, opacity effects
- Managed through `LayerHandle`

## File Structure Recommendation

For FLUI implementation:

```
flui_rendering/
├── core/
│   ├── render_object.rs      # Base RenderObject trait
│   ├── pipeline_owner.rs     # Pipeline coordination
│   ├── constraints.rs        # Constraint system
│   └── parent_data.rs        # ParentData trait
├── context/
│   ├── painting_context.rs   # Paint context
│   └── clip_context.rs       # Clipping helpers
├── layers/
│   ├── layer.rs              # Layer types
│   └── layer_handle.rs       # Layer management
├── mixins/
│   ├── single_child.rs       # RenderObjectWithChildMixin
│   └── multi_child.rs        # ContainerRenderObjectMixin
└── semantics/
    ├── semantics_node.rs     # Accessibility nodes
    └── semantics_config.rs   # Configuration
```
