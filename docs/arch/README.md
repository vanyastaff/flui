# FLUI Architecture Documentation

**Navigation Hub & Quick Reference**

> Welcome to FLUI's architecture documentation. This directory contains comprehensive design documents for all FLUI crates and subsystems.

---

## Quick Start

### New to FLUI?
1. Start with [System Overview](#by-topic) below
2. Read [PATTERNS.md](PATTERNS.md) for common patterns
3. Review [INTEGRATION.md](INTEGRATION.md) to see how crates fit together
4. Dive into specific crate documentation as needed

### Contributing?
1. Review [PATTERNS.md](PATTERNS.md) for coding patterns
2. Check [INTEGRATION.md](INTEGRATION.md) for integration points
3. Read relevant crate architecture docs
4. See [../../CLAUDE.md](../../CLAUDE.md) for development guidelines

---

## Documentation Structure

```
docs/arch/
├── README.md                      ← You are here (Navigation hub)
├── PATTERNS.md                    ← Common architectural patterns
├── INTEGRATION.md                 ← How crates work together
├── DEPENDENCIES.md                ← Dependency rationale & guidelines
│
├── decisions/                     ← Architecture Decision Records (ADRs)
│   ├── ADR-001-unified-render-trait.md
│   ├── ADR-002-three-tree-architecture.md
│   ├── ADR-003-enum-vs-trait-objects.md
│   ├── ADR-004-thread-safety-design.md
│   └── ADR-005-wgpu-only-backend.md
│
├── Core Framework Crates:
│   ├── CORE_FEATURES_ROADMAP.md   ← System overview & roadmap
│   ├── RENDERING_ARCHITECTURE.md  ← RenderObject system
│   ├── WIDGETS_ARCHITECTURE.md    ← Widget layer
│   ├── PAINTING_ARCHITECTURE.md   ← Canvas & DisplayList
│   └── ENGINE_ARCHITECTURE.md     ← GPU rendering (wgpu)
│
├── Extension Crates:
│   ├── ANIMATION_ARCHITECTURE.md  ← Animation system
│   ├── GESTURES_ARCHITECTURE.md   ← Gesture recognition
│   ├── ASSETS_ARCHITECTURE.md     ← Asset management
│   └── DEVTOOLS_ARCHITECTURE.md   ← Developer tools
│
└── Application Layer:
    ├── APP_ARCHITECTURE.md        ← Application framework
    └── CLI_ARCHITECTURE.md        ← CLI tooling
```

---

## By Topic

### System Architecture

| Topic | Document | Description |
|-------|----------|-------------|
| **Three-Tree Architecture** | [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md#three-tree-architecture) | View → Element → Render pattern |
| **Pipeline System** | [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md#pipeline-system) | Build → Layout → Paint phases |
| **Common Patterns** | [PATTERNS.md](PATTERNS.md) | Quick reference for all patterns |
| **Crate Integration** | [INTEGRATION.md](INTEGRATION.md) | How crates work together |
| **Dependencies** | [DEPENDENCIES.md](DEPENDENCIES.md) | Why each dependency was chosen |
| **Architecture Decisions** | [decisions/](decisions/) | ADRs explaining key design choices |

### Core Framework

| Crate | Document | Status | Description |
|-------|----------|--------|-------------|
| **flui_types** | - | ✅ Production | Base types (Size, Rect, Color) |
| **flui_core** | [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md) | ✅ Production | Element tree, pipeline, hooks |
| **flui_rendering** | [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md) | ✅ Production | 81+ RenderObjects |
| **flui_painting** | [PAINTING_ARCHITECTURE.md](PAINTING_ARCHITECTURE.md) | 📋 Design | Canvas API, DisplayList |
| **flui_engine** | [ENGINE_ARCHITECTURE.md](ENGINE_ARCHITECTURE.md) | 🚧 In Progress | GPU rendering (wgpu) |

### UI Layer

| Crate | Document | Status | Description |
|-------|----------|--------|-------------|
| **flui_widgets** | [WIDGETS_ARCHITECTURE.md](WIDGETS_ARCHITECTURE.md) | ✅ Production | 60+ high-level widgets |
| **flui_app** | [APP_ARCHITECTURE.md](APP_ARCHITECTURE.md) | 🚧 In Progress | Application framework |

### Extensions

| Crate | Document | Status | Description |
|-------|----------|--------|-------------|
| **flui_animation** | [ANIMATION_ARCHITECTURE.md](ANIMATION_ARCHITECTURE.md) | 📋 Design | Tween & curve animations |
| **flui_gestures** | [GESTURES_ARCHITECTURE.md](GESTURES_ARCHITECTURE.md) | ✅ Production | Gesture recognition |
| **flui_assets** | [ASSETS_ARCHITECTURE.md](ASSETS_ARCHITECTURE.md) | ✅ Production | Image/font loading |

### Developer Tools

| Tool | Document | Status | Description |
|------|----------|--------|-------------|
| **flui_devtools** | [DEVTOOLS_ARCHITECTURE.md](DEVTOOLS_ARCHITECTURE.md) | 📋 Design | Profiling & debugging |
| **flui_cli** | [CLI_ARCHITECTURE.md](CLI_ARCHITECTURE.md) | 📋 Design | CLI tooling |

**Status Legend:**
- ✅ Production Ready - Stable, used in production
- 🚧 In Progress - Under active development
- 📋 Design - Documented design, not yet implemented

---

## By Role

### Widget Developer (Building UI)

**Learning Path:**
1. [WIDGETS_ARCHITECTURE.md](WIDGETS_ARCHITECTURE.md) - High-level widget API
2. [PATTERNS.md](PATTERNS.md#unified-view-trait) - View trait pattern
3. [INTEGRATION.md](INTEGRATION.md#scenario-1-adding-a-new-widget) - Adding new widgets

**Common Tasks:**
- Composing widgets → [WIDGETS_ARCHITECTURE.md](WIDGETS_ARCHITECTURE.md#stateless-vs-renderobject-widgets)
- Using hooks → [PATTERNS.md](PATTERNS.md#copy-based-signals)
- Handling input → [GESTURES_ARCHITECTURE.md](GESTURES_ARCHITECTURE.md)

### Core Developer (Framework Internals)

**Learning Path:**
1. [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md) - System overview
2. [decisions/](decisions/) - Architecture Decision Records (why we chose this design)
3. [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md) - Render system
4. [ENGINE_ARCHITECTURE.md](ENGINE_ARCHITECTURE.md) - GPU layer
5. [INTEGRATION.md](INTEGRATION.md) - Integration flows

**Common Tasks:**
- Adding RenderObject → [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md)
- Pipeline optimization → [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md#pipeline-system)
- GPU rendering → [ENGINE_ARCHITECTURE.md](ENGINE_ARCHITECTURE.md)
- Understanding design decisions → [decisions/](decisions/)

### New Contributor (Getting Started)

**Onboarding Path:**
1. **Start here**: [INTEGRATION.md](INTEGRATION.md#dependency-overview) - See the big picture
2. **Learn patterns**: [PATTERNS.md](PATTERNS.md) - Common patterns
3. **Pick a topic**: Choose from [By Topic](#by-topic) above
4. **Read guidelines**: [../../CLAUDE.md](../../CLAUDE.md) - Development setup

**First Contributions:**
- Documentation improvements
- Example code
- Widget development (easiest entry point)

---

## Finding Information

### I want to understand...

**...how widgets become rendered pixels**
1. [INTEGRATION.md](INTEGRATION.md#flow-1-widget--element--render) - Complete flow
2. [WIDGETS_ARCHITECTURE.md](WIDGETS_ARCHITECTURE.md) - Widget layer
3. [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md) - Render layer
4. [ENGINE_ARCHITECTURE.md](ENGINE_ARCHITECTURE.md) - GPU layer

**...how state updates trigger rebuilds**
1. [PATTERNS.md](PATTERNS.md#copy-based-signals) - Signal pattern
2. [INTEGRATION.md](INTEGRATION.md#flow-2-state-update--rebuild) - Rebuild flow
3. [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md#hooks-and-reactive-state) - Hook system

**...how input events work**
1. [INTEGRATION.md](INTEGRATION.md#flow-3-input-event--widget-handler) - Event flow
2. [GESTURES_ARCHITECTURE.md](GESTURES_ARCHITECTURE.md) - Gesture system
3. [APP_ARCHITECTURE.md](APP_ARCHITECTURE.md) - Platform integration

**...how assets are loaded**
1. [INTEGRATION.md](INTEGRATION.md#flow-4-asset-loading--image-display) - Asset flow
2. [ASSETS_ARCHITECTURE.md](ASSETS_ARCHITECTURE.md) - Asset system
3. [PAINTING_ARCHITECTURE.md](PAINTING_ARCHITECTURE.md) - Image rendering

**...why a design decision was made**
1. Check [decisions/](decisions/) for Architecture Decision Records (ADRs)
2. Search architecture docs for "Design Rationale" sections
3. Check [PATTERNS.md](PATTERNS.md) for pattern explanations
4. See [DEPENDENCIES.md](DEPENDENCIES.md) for dependency choices

---

## Common Questions

### Q: What's the difference between View, Element, and RenderObject?

**A:** FLUI uses a three-tree architecture:
- **View**: Immutable UI description (what you write)
- **Element**: Mutable tree node (lifecycle management)
- **RenderObject**: Layout and paint implementation

See: [PATTERNS.md](PATTERNS.md#three-tree-architecture)

### Q: How do I add state to my widget?

**A:** Use hooks (signals, memos, effects):
```rust
let count = use_signal(ctx, 0);
```

See: [PATTERNS.md](PATTERNS.md#copy-based-signals)

### Q: How do I create a custom layout?

**A:** Implement the `Render` trait:
1. Define `layout()` - compute size
2. Define `paint()` - generate layers
3. Wrap in a widget

See: [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md#unified-render-trait)

### Q: Why is FLUI thread-safe?

**A:** All shared state uses `Arc<Mutex<T>>` (parking_lot):
- Enables parallel build pipeline
- Supports multi-threaded UI
- Uses fast parking_lot (2-3x faster than std)

See: [PATTERNS.md](PATTERNS.md#arcmutex-for-shared-state)

### Q: How do crates depend on each other?

**A:** Strict 5-layer hierarchy:
```
Layer 0: flui_types (foundation)
Layer 1: flui_painting, flui_engine, flui_assets
Layer 2: flui_core
Layer 3: flui_rendering, flui_gestures, flui_animation
Layer 4: flui_widgets
Layer 5: flui_app, flui_devtools
```

See: [INTEGRATION.md](INTEGRATION.md#dependency-overview)

---

## External References

### Related Documentation
- [Main README](../../README.md) - Project overview
- [CLAUDE.md](../../CLAUDE.md) - Development guidelines
- [API Guide](../API_GUIDE.md) - API reference
- [Pipeline Architecture](../PIPELINE_ARCHITECTURE.md) - Detailed pipeline design

### Comparisons
- [Flutter Documentation](https://flutter.dev/docs) - FLUI is inspired by Flutter
- [GLOSSARY_TYPES_MAPPING.md](../GLOSSARY_TYPES_MAPPING.md) - Flutter → FLUI mapping

### Community
- [GitHub Repository](https://github.com/your-org/flui)
- [Issue Tracker](https://github.com/your-org/flui/issues)

---

## Contributing to Documentation

### Adding New Architecture Docs

1. Use consistent structure (see existing docs)
2. Include Executive Summary
3. Add to this README navigation
4. Cross-reference with related docs

### Updating Existing Docs

1. Keep "Last Updated" date current
2. Update status when implementation changes
3. Remove outdated code examples (link to source instead)
4. Maintain cross-references

### Documentation Principles

- **Separation**: Architecture docs explain WHAT and WHY, not HOW
- **DRY**: Common patterns in PATTERNS.md, integration flows in INTEGRATION.md
- **Linking**: Reference source code, don't duplicate it
- **Navigation**: Keep this README updated for discoverability

---

## Maintenance

**Last Review**: 2025-01-10
**Next Review**: 2025-04-10 (Quarterly)
**Maintainers**: Core team

**Quick Stats:**
- 11 architecture documents
- 3 reference guides (PATTERNS, INTEGRATION, DEPENDENCIES)
- 5 Architecture Decision Records (ADRs)
- 12 crates documented
- Coverage: ~95% of codebase

---

**Ready to dive in?** Pick a document from [By Topic](#by-topic) or [By Role](#by-role) above!
