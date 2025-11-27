# Migrate Widgets to New View API

## Status

**Stage:** Proposed
**Priority:** High - Blocks application development
**Estimated Effort:** 2-3 days

## Quick Summary

Enable all 80+ widgets in flui_widgets by adding an adapter layer (RenderBoxExt) and migrating widgets to support three usage patterns:

1. **Bon Builder** - Type-safe, chainable API
2. **Struct Literal** - Flutter-like initialization
3. **Declarative Macros** - Ergonomic widget trees

**Current:** 94 compilation errors, widgets unusable
**After:** All widgets compile, three usage patterns, working examples

## What Changed

### Phase 1: Adapter Layer
- Add `RenderBoxExt` trait with `.leaf()`, `.child()`, `.children()`, `.maybe_child()` methods
- Location: `crates/flui_core/src/render.rs`
- Enables RenderObject → Element conversion with type-safe arity checking

### Phase 2: Widget Migration (80+ widgets)
- **Basic widgets** (24): Container, Text, SizedBox, Padding, etc.
- **Layout widgets** (22): Row, Column, Stack, Flex, etc.
- **Interaction widgets** (3): GestureDetector, MouseRegion, AbsorbPointer
- **Visual effects** (13): Opacity, Transform, ClipRRect, etc.
- **Remaining** (18): Scrolling, animations, material, navigation

Each widget updated to:
- Use bon-generated builders
- Support struct literal with Default
- Provide declarative macros
- Include convenience constructors

## Usage Examples

### Before (Doesn't compile)
```rust
// ❌ Error: no method named `leaf` found
let text = Text::new("Hello");  // Doesn't work
```

### After (Three Patterns)

**Pattern 1: Bon Builder**
```rust
let container = Container::builder()
    .padding(EdgeInsets::all(16.0))
    .color(Color::BLUE)
    .width(200.0)
    .child(Text::new("Hello"))
    .build();
```

**Pattern 2: Struct Literal**
```rust
let container = Container {
    padding: Some(EdgeInsets::all(16.0)),
    color: Some(Color::BLUE),
    width: Some(200.0),
    child: Child::new(Text::new("Hello")),
    ..Default::default()
};
```

**Pattern 3: Declarative Macros**
```rust
let layout = column![
    text!("Welcome"),
    container! {
        padding: EdgeInsets::all(16.0),
        color: Color::BLUE,
        child: text!("Content")
    },
    row![
        button!("Cancel"),
        button!("OK"),
    ],
];
```

## Key Files

- **proposal.md** - Full proposal with rationale
- **design.md** - Architecture and implementation patterns
- **tasks.md** - 17 tasks across 7 phases
- **specs/widget-api/spec.md** - Formal requirements with scenarios

## How to Review

1. Read `proposal.md` for context and motivation
2. Read `design.md` for technical architecture
3. Review `specs/widget-api/spec.md` for formal requirements
4. Check `tasks.md` for implementation plan

## How to Implement

See `tasks.md` for detailed steps. High-level flow:

1. **Implement adapter layer** (Task 1.1-1.2)
   - Add RenderBoxExt to flui_core/src/render.rs
   - Test with simple widget (Text)

2. **Migrate widgets** (Task 2.1-5.1)
   - Start with basic widgets (highest priority)
   - Update build() methods to use adapter
   - Add bon builders, macros
   - Run tests after each module

3. **Create examples** (Task 6.1-6.2)
   - Widget gallery showcase
   - Update documentation

4. **Validate** (Task 7.1-7.2)
   - All tests pass
   - Zero compilation errors

## Success Metrics

- ✅ `cargo build -p flui_widgets` → 0 errors (currently 94)
- ✅ `cargo test -p flui_widgets` → All pass
- ✅ `cargo run --example widget_gallery` → Renders correctly
- ✅ All 80+ widgets support three patterns

## Dependencies

**Requires:**
- ✅ flui_core v0.6.0+ (already available)
- ✅ flui_rendering with RenderBox<A> trait (already available)
- ✅ bon crate for builder generation (already in Cargo.toml)

**Enables:**
- Application development
- Widget gallery examples
- End-to-end testing
- Tutorial/documentation with working examples

## Related Work

- **migrate-renderobjects-to-new-api** - Companion change for RenderObjects
- **flui-rendering spec** - Rendering layer already complete
- **compositor-layers spec** - Layer system already complete

## Timeline

**Week 1:**
- Day 1-2: Adapter layer + basic widgets (Container, Text, SizedBox, etc.)
- Day 3: Layout widgets (Row, Column, Stack)
- Day 4: Interaction + visual effects
- Day 5: Examples, docs, testing

**Week 2:**
- Polish, bug fixes, comprehensive testing

## Questions?

- Open an issue in the repository
- Check `design.md` for architectural decisions
- Review `specs/widget-api/spec.md` for formal requirements
