# Proposal: Migrate Widgets to New View API

## Why

The flui_widgets crate contains 80+ Flutter-inspired widget implementations that provide essential UI building blocks (Container, Text, Row, Column, etc.). These widgets currently fail to compile due to API mismatch with the new View system introduced in flui_core v0.6.0.

**Business Value:**
- **Unblocks application development:** Developers cannot build UIs without working widgets
- **Validates architecture:** Demonstrates that the new View API works for real-world widgets
- **Accelerates adoption:** Comprehensive widget library is key differentiator vs other Rust UI frameworks
- **Reduces friction:** Three usage patterns (builder, struct, macro) lower barrier to entry

**User Impact:**
- **Cannot build UIs:** 94 compilation errors block all widget usage
- **No working examples:** Example apps cannot demonstrate framework capabilities
- **Documentation mismatch:** Guide shows patterns that don't compile
- **Lost productivity:** Developers cannot prototype or develop applications

**Technical Debt:**
- **Outdated API usage:** Widgets written for old API without adapter layer
- **Inconsistent patterns:** Mix of builder and struct literal without standardization
- **Missing macros:** No ergonomic declarative syntax for widget trees
- **No bon integration:** Manual builders instead of generated type-safe builders

This proposal addresses all issues by implementing an adapter layer and migrating all 80+ widgets to support three ergonomic usage patterns with bon-generated builders.

## Problem Statement

The `flui_widgets` crate contains 80+ widget implementations (Container, Text, Row, Column, etc.) that were written for an older API and currently fail to compile with 94 errors. The widgets use deprecated builder methods on RenderObjects (`.leaf()`, `.child()`, `.children()`) that don't exist in the current architecture.

**Current State:**
- ✅ Widget modules and exports enabled in lib.rs
- ✅ Core imports fixed (flui_core::view, flui_core::render, flui_core::element)
- ✅ BuildContext trait usage corrected (&dyn BuildContext)
- ❌ 94 compilation errors due to API mismatch
- ❌ Missing adapter layer for RenderObject → Element conversion
- ❌ Widgets cannot be used in applications

**Impact:**
- Blocks user-facing widget functionality
- Prevents testing of the widget system
- Comprehensive guide documentation exists but widgets don't compile

## Proposed Solution

Implement a two-phase migration:

### Phase 1: Adapter Layer (RenderBoxExt extension methods)
Add backward-compatible builder methods to convert RenderObjects to Elements, enabling existing widget code to work with minimal changes.

**Benefits:**
- Unblocks 80+ existing widgets immediately
- Maintains clean separation: widgets use high-level API, RenderView handles low-level details
- Incremental migration path - can be done widget-by-widget

### Phase 2: Widget-by-Widget Migration
Systematically fix each widget module using the adapter layer, prioritized by usage frequency.

**Priority Order:**
1. Basic widgets (24) - Container, SizedBox, Padding, Text, Center, Align
2. Layout widgets (22) - Row, Column, Stack, Flex, Positioned
3. Interaction widgets (3) - GestureDetector, MouseRegion, AbsorbPointer
4. Visual effects (13) - Opacity, Transform, ClipRRect, DecoratedBox
5. Remaining widgets - Scrolling, animations, material, navigation

## Scope

### In Scope
- ✅ Add RenderBoxExt extension methods (.leaf, .child, .children, .maybe_child)
- ✅ Fix compilation errors in all 80+ widgets
- ✅ Ensure StatelessView/StatefulView trait implementations are correct
- ✅ Add example demonstrating each widget category
- ✅ Update widget tests to pass

### Out of Scope
- ❌ New widget implementations beyond existing 80+
- ❌ Animation system overhaul
- ❌ Material design theme system
- ❌ Hot reload functionality
- ❌ Performance optimization (handled separately)

## Success Criteria

1. ✅ `cargo build -p flui_widgets` compiles without errors
2. ✅ All existing widget tests pass
3. ✅ Example app demonstrating basic, layout, and interaction widgets works
4. ✅ Documentation updated with working code examples

## Alternatives Considered

### Alternative 1: Rewrite all widgets to use RenderView directly
**Rejected:** Too time-consuming, high risk of errors, breaks existing patterns

### Alternative 2: Keep lib.rs commented out until full rewrite
**Rejected:** Wastes existing 80+ widget implementations, delays user-facing functionality

### Alternative 3: Partial enablement (only basic widgets)
**Rejected:** Inconsistent API, confusing for users, partial solution

## Dependencies

**Blocked by:**
- None (flui_core and flui_rendering APIs are stable)

**Blocks:**
- Widget gallery examples
- Application development
- End-to-end testing

## Implementation Plan

See `tasks.md` for detailed task breakdown.

**Estimated Effort:**
- Phase 1 (Adapter Layer): 2-4 hours
- Phase 2 (Widget Migration): 1-2 days
- Testing & Examples: 4 hours

**Total:** ~2-3 days

## Related Changes

- Complements: `migrate-renderobjects-to-new-api` (RenderObject modernization)
- Depends on: flui_core view/render/element module exports (already completed)
- Enables: Widget gallery examples, application development

## Open Questions

1. Should adapter methods be in flui_core::render or flui_rendering::prelude?
   - **Recommendation:** flui_core::render (better API surface for widgets)

2. Should we add deprecation warnings to guide future refactoring?
   - **Recommendation:** No - adapter layer is the correct long-term API

3. Should StatefulView widgets use signals or internal state?
   - **Recommendation:** Document both patterns, let widget author choose
