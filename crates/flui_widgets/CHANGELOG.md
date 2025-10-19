# Changelog - flui_widgets

All notable changes to the `flui_widgets` crate will be documented in this file.

## [Unreleased]

### Added - Week 5 Day 1 (2025-01-XX)

- **Container widget** - First widget implementation with complete feature set:
  - Three creation syntaxes: struct literal, builder pattern, and declarative macro
  - bon builder integration with custom setters and finishing functions
  - 12 properties: key, alignment, padding, color, decoration, margin, width, height, constraints, child
  - Helper methods: `new()`, `set_child()`, `validate()`, `get_decoration()`
  - Comprehensive test suite (19 tests)
  - Full documentation with examples

- **Documentation**:
  - `WIDGET_GUIDELINES.md` - Complete guide for implementing widgets
  - `WIDGET_TEMPLATE.rs` - Ready-to-use template for new widgets
  - `README.md` - Crate overview and usage examples
  - `TODO_CONTAINER.md` - Future enhancement ideas for Container

- **Infrastructure**:
  - bon builder dependency for type-safe builders
  - Macro support for declarative widget creation
  - Standard testing patterns
  - Documentation standards

## Upcoming (Week 5-6)

### Week 5 - Basic Layout Widgets
- Row - Horizontal flex layout
- Column - Vertical flex layout
- SizedBox - Fixed size box
- Padding - Padding wrapper
- Center - Center alignment
- Align - Flexible alignment

### Week 6 - Advanced Layout Widgets
- Expanded - Flex child with flex factor
- Flexible - Flex child with fit
- Stack - Layered positioning
- Positioned - Absolute positioning in Stack
- Wrap - Flowing layout
- ListView - Scrollable list (basic)
- GridView - Scrollable grid (basic)
- AspectRatio - Aspect ratio constraint
- FittedBox - Scale and position child
- ConstrainedBox - Additional constraints

## Design Decisions

### Three Syntax Styles
We support three creation patterns to serve different use cases:

1. **Struct literal**: Quick prototyping, Flutter-like familiarity
2. **Builder pattern**: Type safety, IDE autocomplete, complex configurations
3. **Declarative macro**: Concise syntax, less boilerplate

### bon Builder Integration
- Type-safe builders with compile-time checks
- Automatic type conversions via `.into()`
- Custom setters for complex fields (like child widgets)
- Custom finishing functions for ergonomic APIs

### Validation
All widgets implement `validate()` to catch configuration errors early:
- Invalid numeric values (negative, NaN, infinite)
- Conflicting properties (e.g., width + min_width)
- Logical constraints (e.g., min > max)

### Testing
Every widget requires:
- Minimum 10-15 tests
- Coverage for all 3 syntax styles
- Validation tests (valid and invalid)
- Edge case tests

## Migration Notes

### Element System (Future)
Currently, `Widget::create_element()` uses `todo!()` placeholders. Once the Element system is fully implemented, widgets will:
1. Create appropriate Element types (ComponentElement, RenderObjectElement)
2. Build child widget trees
3. Integrate with the full three-tree architecture

No breaking changes to the widget API are expected.

## Version History

- **v0.1.0** (In Development) - Initial implementation with Container widget
