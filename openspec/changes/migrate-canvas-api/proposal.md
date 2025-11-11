# Proposal: Migrate to Canvas API from flui-painting

**Change ID:** `migrate-canvas-api`
**Status:** Implemented
**Type:** Architecture Refactoring
**Date:** 2025-01-10

## Summary

This change migrates all RenderObjects from returning `BoxedLayer` to returning `Canvas` from the `paint()` method. The Canvas API, provided by `flui_painting` crate, offers a Flutter-compatible drawing interface that records commands into a DisplayList for later execution by the GPU backend.

## Why

The original architecture created tight coupling between rendering logic and the GPU engine through direct creation of layer objects. This circular dependency prevented clean separation of concerns, made testing difficult without GPU context, and created API inconsistency by mixing abstraction levels. By introducing the Canvas API as an intermediate recording layer, we achieve proper architectural separation, enable testing without GPU dependencies, and provide a familiar Flutter-compatible interface that records drawing commands for deferred execution.

## Motivation

### Problems Addressed

1. **Tight Coupling to Engine**: RenderObjects directly created `PictureLayer` and `ContainerLayer` from `flui_engine`, creating circular dependencies
2. **Backend Dependency**: Rendering logic was coupled to GPU implementation details (wgpu, lyon, glyphon)
3. **API Inconsistency**: Mixed abstraction levels - some code used high-level concepts, others used low-level layer primitives
4. **Testing Difficulty**: Hard to test rendering without GPU context

### Goals

1. **Abstraction**: Decouple rendering logic from GPU implementation
2. **Flutter Compatibility**: Provide familiar Canvas API matching Flutter's interface
3. **Clean Architecture**: Follow command pattern - record now, execute later
4. **Testability**: Enable testing without GPU backend

## Solution Overview

### Architecture Change

**Before:**
```
RenderObject → PictureLayer/ContainerLayer → WgpuPainter
```

**After:**
```
RenderObject → Canvas → DisplayList → PictureLayer → WgpuPainter
```

### API Change

**Old Signature:**
```rust
fn paint(&self, ctx: &PaintContext) -> BoxedLayer
```

**New Signature:**
```rust
fn paint(&self, ctx: &PaintContext) -> Canvas
```

### Key Changes

1. **Canvas Recording**: RenderObjects create `Canvas` and record drawing commands
2. **DisplayList**: Commands stored in `DisplayList` for later execution
3. **Composition**: Use `canvas.append_canvas()` instead of layer containers
4. **Import Changes**: Remove `flui_engine` imports, use `flui_painting::Canvas`

## Implementation Status

✅ **COMPLETED** - All changes have been implemented and merged

### Completed Work

- ✅ Canvas API implementation in `flui_painting` (commit 36f520f)
- ✅ DisplayList command recording (commit 36f520f)
- ✅ Core trait signature change: `Render::paint()` → `Canvas` (commit 8bf4db3)
- ✅ Migration of all RenderObjects in `flui_rendering` (commits d0aaadf, 98fe9cf, b5e871d, 6b67eb9, 843bad8)
- ✅ Complete Canvas API migration (commit 0b48009)
- ✅ Removed `flui_engine` dependency from `flui_rendering` (commit 0b48009)
- ✅ Migration guide created (`CANVAS_MIGRATION_GUIDE.md`)

### Commit History

```
0b48009 refactor(flui_rendering): Complete Canvas API migration and remove flui_engine dependency
843bad8 refactor(rendering): Complete Canvas API migration for all RenderObjects
6b67eb9 refactor(rendering): Migrate clipping and layout RenderObjects to Canvas API
b5e871d refactor(rendering): Update 6 more RenderObjects to use Canvas API
76b74b4 feat(painting): Add Canvas::append_canvas() for composing canvases
8bf4db3 refactor(core+engine): Change Render::paint() to return Canvas instead of BoxedLayer
98fe9cf feat(flui_rendering): Migrate text and scrollbar rendering to Canvas API
d0aaadf feat(flui_rendering): Migrate RenderObjects to Canvas API and fix compilation errors
36f520f feat(flui_painting): Add Canvas API and DisplayList for command recording
```

## Impact Assessment

### Affected Crates

1. **flui_painting** (New API)
   - Added `Canvas`, `DisplayList`, `DrawCommand`
   - No breaking changes (new crate functionality)

2. **flui_core** (Breaking Change)
   - `Render::paint()` signature changed
   - `PaintContext::paint_child()` returns `Canvas` instead of `BoxedLayer`

3. **flui_rendering** (Breaking Change)
   - All RenderObject implementations updated
   - Removed `flui_engine` dependency
   - All `paint()` methods return `Canvas`

4. **flui_engine** (Internal Change)
   - Now consumes `Canvas` instead of being imported by RenderObjects
   - Layer creation happens in engine, not in rendering code

### Breaking Changes

✅ **All breaking changes handled** - Migration complete

- Changed: `Render::paint()` return type
- Changed: `ElementTree::paint_child()` return type
- Changed: All RenderObject `paint()` implementations
- Removed: Direct layer creation in RenderObjects
- Removed: `flui_engine` dependency from `flui_rendering`

### Migration Path

All existing RenderObjects have been migrated following three patterns:

1. **Leaf Render**: Create Canvas, draw primitives, return canvas
2. **Single Child**: Create Canvas, draw parent, append child canvas
3. **Multi Child**: Create Canvas, append all child canvases

See `CANVAS_MIGRATION_GUIDE.md` for detailed patterns.

## Benefits Achieved

### 1. Clean Architecture ✅
- Clear separation: Painting API (flui_painting) vs GPU execution (flui_engine)
- No circular dependencies
- Command pattern properly implemented

### 2. Flutter Compatibility ✅
- Familiar API for Flutter developers
- Same method names and semantics
- Easier onboarding and documentation

### 3. Testability ✅
- Can test Canvas recording without GPU
- DisplayList can be inspected and verified
- No GPU context required for unit tests

### 4. Maintainability ✅
- Single responsibility: RenderObjects describe what to draw, not how
- Engine can be swapped without changing RenderObjects
- Easier to add new drawing primitives

## Documentation

### Added Documentation

1. **CANVAS_MIGRATION_GUIDE.md** - Migration patterns for RenderObjects
2. **docs/arch/PAINTING_ARCHITECTURE.md** - Architecture design document
3. **crates/flui_painting/src/canvas.rs** - Extensive inline documentation
4. **crates/flui_painting/src/display_list.rs** - Command recording documentation

### Updated Documentation

1. **CLAUDE.md** - Updated with Canvas API references
2. **crates/flui_core/src/render/render.rs** - Updated trait documentation
3. **crates/flui_core/src/render/context.rs** - Updated PaintContext docs

## Validation

### Testing

✅ **All tests passing**
- Build: `cargo build --workspace` ✅
- Tests: `cargo test --workspace` ✅
- Clippy: `cargo clippy --workspace -- -D warnings` ✅
- Examples: All examples compile and run ✅

### Code Quality

- All clippy warnings resolved
- Consistent coding style
- Comprehensive documentation
- Migration guide for future reference

## Risks and Mitigations

### Identified Risks

1. **Performance Impact**: Canvas recording adds abstraction overhead
   - **Mitigation**: ✅ DisplayList is lightweight, GPU execution unchanged
   - **Status**: No measurable performance degradation observed

2. **Learning Curve**: New API for contributors
   - **Mitigation**: ✅ Migration guide and extensive documentation
   - **Status**: API is Flutter-compatible, familiar to many developers

3. **Incomplete Migration**: Some code might still use old pattern
   - **Mitigation**: ✅ Removed `flui_engine` dependency from `flui_rendering`
   - **Status**: Migration complete, compiler enforces new API

## Future Work

### Potential Enhancements

1. **Canvas Optimization**: Cache frequently used DisplayLists
2. **Command Batching**: Merge similar commands for efficiency
3. **Advanced Features**: Shaders, image filters, blur effects
4. **Debug Tools**: DisplayList inspection and visualization

### Related Work

- Painter V2 Architecture (planned for v0.7.0)
- Layer system optimization
- GPU backend improvements

## Approval

This change has been **implemented and merged** into the main branch. This proposal serves as documentation of the completed migration.

**Implementation Completed:** January 2025
**Documentation Status:** Complete
**Migration Guide:** Available (`CANVAS_MIGRATION_GUIDE.md`)
