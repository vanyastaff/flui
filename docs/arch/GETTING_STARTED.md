# Getting Started with FLUI Architecture Documentation

**Quick Start Guide** - Find your path through FLUI's documentation

---

## 🎯 Choose Your Path

Select the path that best matches your goal:

### 1. 👨‍💻 "I want to build UI with FLUI"

**You are:** Widget Developer, App Developer

**Start here:**
1. **[WIDGETS_ARCHITECTURE.md](WIDGETS_ARCHITECTURE.md)** (15 min)
   - Understand the widget system
   - Learn stateless vs stateful widgets
   - See common widget patterns

2. **[PATTERNS.md](PATTERNS.md#unified-view-trait)** (10 min)
   - How to create custom widgets
   - Hook usage patterns
   - State management basics

3. **[INTEGRATION.md](INTEGRATION.md#scenario-1-adding-a-new-widget)** (5 min)
   - Step-by-step widget creation
   - How to integrate with existing widgets

**Next steps:**
- Browse `crates/flui_widgets/` for examples
- Read `crates/flui_core/examples/simplified_view.rs`
- Check [GESTURES_ARCHITECTURE.md](GESTURES_ARCHITECTURE.md) for input handling

**Time investment:** ~30 minutes to start building

---

### 2. 🔧 "I want to understand how FLUI works internally"

**You are:** Core Contributor, Framework Developer

**Start here:**
1. **[README.md](README.md)** (5 min)
   - Get the big picture
   - Understand the 12-crate structure
   - See the 5-layer dependency hierarchy

2. **[INTEGRATION.md](INTEGRATION.md)** (20 min)
   - Flow 1: Widget → Element → Render (build/layout/paint pipeline)
   - Flow 2: State Update → Rebuild (reactive updates)
   - Understand how all pieces fit together

3. **[decisions/](decisions/)** - Read ADRs (30 min)
   - **[ADR-002](decisions/ADR-002-three-tree-architecture.md)** - Why three trees?
   - **[ADR-001](decisions/ADR-001-unified-render-trait.md)** - Why unified Render trait?
   - **[ADR-003](decisions/ADR-003-enum-vs-trait-objects.md)** - Performance decisions

4. **[PATTERNS.md](PATTERNS.md)** (20 min)
   - Study all architectural patterns
   - Understand thread-safety design
   - Learn performance optimizations

**Next steps:**
- Deep dive into specific crate documentation
- Read [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md)
- Study [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md)

**Time investment:** ~1.5 hours to understand the system

---

### 3. 🎨 "I want to create custom layouts/rendering"

**You are:** RenderObject Developer, Custom Widget Creator

**Start here:**
1. **[RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md)** (25 min)
   - Understand the Render trait
   - Learn layout and paint phases
   - See RenderObject examples

2. **[PATTERNS.md](PATTERNS.md#unified-render-trait)** (15 min)
   - Unified Render Trait pattern
   - Context Pattern (LayoutContext, PaintContext)
   - ParentData Metadata pattern

3. **[decisions/ADR-001-unified-render-trait.md](decisions/ADR-001-unified-render-trait.md)** (10 min)
   - Why single trait instead of 3 mixins
   - Arity system explained
   - Performance characteristics

4. **[INTEGRATION.md](INTEGRATION.md#scenario-2-implementing-custom-layout)** (10 min)
   - Step-by-step custom RenderObject creation
   - Integration with Element tree

**Next steps:**
- Study `crates/flui_rendering/src/objects/` for examples
- Read [PAINTING_ARCHITECTURE.md](PAINTING_ARCHITECTURE.md)
- Explore [ENGINE_ARCHITECTURE.md](ENGINE_ARCHITECTURE.md) for GPU details

**Time investment:** ~1 hour to create custom layouts

---

### 4. 📊 "I want to understand performance/benchmarks"

**You are:** Performance Engineer, Optimizer

**Start here:**
1. **[decisions/ADR-003-enum-vs-trait-objects.md](decisions/ADR-003-enum-vs-trait-objects.md)** (10 min)
   - **3.75x faster** element access (benchmarks included)
   - Cache hit rate: 80% vs 40%
   - Memory footprint analysis

2. **[decisions/ADR-004-thread-safety-design.md](decisions/ADR-004-thread-safety-design.md)** (10 min)
   - **parking_lot**: 3x faster than std::sync::Mutex
   - <5% overhead for thread-safety
   - Parallel build: 2.5x speedup on 4 cores

3. **[decisions/ADR-005-wgpu-only-backend.md](decisions/ADR-005-wgpu-only-backend.md)** (10 min)
   - **5.6x faster** complex UIs vs software rendering
   - **80x faster** blur effects
   - Frame time analysis

4. **[DEPENDENCIES.md](DEPENDENCIES.md#performance-characteristics)** (15 min)
   - All dependency performance impacts
   - Binary size breakdown
   - Build time analysis

**Next steps:**
- Run benchmarks: `cargo bench -p flui_core`
- Profile with Tracy: `cargo run --features tracy`
- Study [PATTERNS.md](PATTERNS.md#performance-patterns)

**Time investment:** ~45 minutes to understand performance

---

### 5. 🔍 "I need to understand a specific decision"

**You are:** Anyone questioning "Why did they do it this way?"

**Start here:**
1. **Check [decisions/](decisions/)** for relevant ADR
   - ADR-001: Why unified Render trait?
   - ADR-002: Why three trees (View/Element/Render)?
   - ADR-003: Why enum instead of trait objects?
   - ADR-004: Why Arc/Mutex everywhere?
   - ADR-005: Why GPU-only (wgpu)?

2. **Search [PATTERNS.md](PATTERNS.md)** for pattern explanation

3. **Check [DEPENDENCIES.md](DEPENDENCIES.md)** for dependency choices

**Each ADR contains:**
- ✅ Context and problem statement
- ✅ Options considered (with pros/cons)
- ✅ Decision and rationale
- ✅ Benchmarks and validation
- ✅ Trade-offs and consequences

**Time investment:** ~10 minutes per ADR

---

### 6. 🆕 "I'm completely new to FLUI"

**You are:** First-time FLUI user

**Start here:**
1. **[../../README.md](../../README.md)** (5 min)
   - Project overview
   - What is FLUI?
   - Quick example

2. **[README.md](README.md)** (10 min)
   - Architecture documentation overview
   - Navigate by role (choose your path above)

3. **[INTEGRATION.md](INTEGRATION.md#dependency-overview)** (15 min)
   - Understand the 12 crates
   - See how they fit together
   - 5-layer dependency hierarchy

4. **[PATTERNS.md](PATTERNS.md#three-tree-architecture)** (15 min)
   - Core concept: View → Element → Render
   - Understand the mental model
   - Basic patterns

**Next steps:**
- Choose a specific path from above based on your goal
- Run examples: `cargo run --example simplified_view`
- Read [../../CLAUDE.md](../../CLAUDE.md) for development setup

**Time investment:** ~45 minutes to get oriented

---

## 🗺️ Documentation Map by Topic

### Core Concepts

| Concept | Primary Source | Read Time |
|---------|---------------|-----------|
| **Three-Tree Architecture** | [ADR-002](decisions/ADR-002-three-tree-architecture.md) | 15 min |
| **Unified Render Trait** | [ADR-001](decisions/ADR-001-unified-render-trait.md) | 10 min |
| **Element Storage** | [ADR-003](decisions/ADR-003-enum-vs-trait-objects.md) | 12 min |
| **Thread-Safety** | [ADR-004](decisions/ADR-004-thread-safety-design.md) | 12 min |
| **GPU Rendering** | [ADR-005](decisions/ADR-005-wgpu-only-backend.md) | 15 min |

### System Understanding

| Topic | Primary Source | Read Time |
|-------|---------------|-----------|
| **How crates integrate** | [INTEGRATION.md](INTEGRATION.md) | 20 min |
| **All patterns** | [PATTERNS.md](PATTERNS.md) | 25 min |
| **Dependencies** | [DEPENDENCIES.md](DEPENDENCIES.md) | 30 min |
| **System overview** | [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md) | 40 min |

### Specific Crates

| Crate | Documentation | Read Time |
|-------|--------------|-----------|
| **flui_widgets** | [WIDGETS_ARCHITECTURE.md](WIDGETS_ARCHITECTURE.md) | 30 min |
| **flui_rendering** | [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md) | 35 min |
| **flui_engine** | [ENGINE_ARCHITECTURE.md](ENGINE_ARCHITECTURE.md) | 25 min |
| **flui_core** | [CORE_FEATURES_ROADMAP.md](CORE_FEATURES_ROADMAP.md) | 40 min |
| **flui_gestures** | [GESTURES_ARCHITECTURE.md](GESTURES_ARCHITECTURE.md) | 30 min |
| **flui_assets** | [ASSETS_ARCHITECTURE.md](ASSETS_ARCHITECTURE.md) | 30 min |

---

## 📋 Common Questions: Quick Answers

### "How do I create a custom widget?"

**Answer:** 3-step process
1. Implement `View` trait with `build()` method
2. Return `impl IntoElement` (RenderObject + children)
3. Use hooks for state (e.g., `use_signal`)

**Read:** [PATTERNS.md](PATTERNS.md#unified-view-trait) (5 min)

---

### "How does state management work?"

**Answer:** Copy-based Signals
- 8-byte handles (cheap to clone)
- Thread-safe (Arc/Mutex internally)
- Automatic rebuild scheduling

**Read:** [PATTERNS.md](PATTERNS.md#copy-based-signals) (5 min)

---

### "Why is FLUI faster than other frameworks?"

**Answer:** Multiple optimizations
- **3.75x** faster enum dispatch vs trait objects
- **80%** cache hit rate for element access
- **GPU-only** rendering (5.6x faster on complex UIs)
- Incremental updates at every layer

**Read:** [decisions/ADR-003](decisions/ADR-003-enum-vs-trait-objects.md), [ADR-005](decisions/ADR-005-wgpu-only-backend.md) (20 min)

---

### "How do I debug layout issues?"

**Answer:** Use tracing and devtools
1. Enable tracing: `RUST_LOG=debug cargo run`
2. Check RenderObject `layout()` calls
3. Verify constraints and sizes
4. Use DevTools (future)

**Read:** [RENDERING_ARCHITECTURE.md](RENDERING_ARCHITECTURE.md#debugging) (10 min)

---

### "What dependencies does FLUI use and why?"

**Answer:** Critical dependencies
- **wgpu** - GPU rendering (5.6x faster)
- **parking_lot** - Sync primitives (3x faster than std)
- **lyon** - Path tessellation
- **glyphon** - GPU text rendering

**Read:** [DEPENDENCIES.md](DEPENDENCIES.md) (30 min)

---

### "How do I contribute to FLUI?"

**Answer:** Start small
1. Read [../../CLAUDE.md](../../CLAUDE.md) for setup
2. Pick a good first issue
3. Study relevant architecture doc
4. Follow patterns in [PATTERNS.md](PATTERNS.md)

**Read:** [README.md](README.md#new-contributor-getting-started) (10 min)

---

## ⏱️ Time Investment by Goal

| Goal | Documents to Read | Time | What You'll Learn |
|------|------------------|------|-------------------|
| **Quick Overview** | README.md | 10 min | Big picture, navigation |
| **Build Simple UI** | WIDGETS + PATTERNS | 30 min | Create widgets, use hooks |
| **Understand System** | INTEGRATION + ADRs | 1.5 hours | How everything works |
| **Custom Rendering** | RENDERING + ADR-001 | 1 hour | Create RenderObjects |
| **Performance Tuning** | ADR-003, ADR-005, DEPS | 45 min | All optimizations |
| **Deep Dive** | All docs | 4-6 hours | Complete understanding |

---

## 🎓 Learning Paths Summary

### Path 1: Widget Developer (Fast)
```
README.md (5 min)
    ↓
WIDGETS_ARCHITECTURE.md (15 min)
    ↓
PATTERNS.md#unified-view-trait (10 min)
    ↓
Start building! (30 min total)
```

### Path 2: Core Developer (Comprehensive)
```
README.md (5 min)
    ↓
INTEGRATION.md (20 min)
    ↓
Read all 5 ADRs (1 hour)
    ↓
PATTERNS.md (25 min)
    ↓
Deep understanding (1.5 hours total)
```

### Path 3: Performance Engineer (Focused)
```
ADR-003: Enum vs Trait Objects (10 min)
    ↓
ADR-004: Thread-Safety (10 min)
    ↓
ADR-005: GPU Rendering (10 min)
    ↓
DEPENDENCIES.md#performance (15 min)
    ↓
Performance mastery (45 min total)
```

---

## 🚀 Next Steps After Reading

1. **Run Examples**
   ```bash
   cargo run --example simplified_view
   cargo run --example thread_safe_hooks
   ```

2. **Explore Source Code**
   - `crates/flui_widgets/` - Widget implementations
   - `crates/flui_rendering/src/objects/` - RenderObjects
   - `crates/flui_core/src/hooks/` - Hook system

3. **Read Development Guide**
   - [../../CLAUDE.md](../../CLAUDE.md) - Development setup
   - Build commands, testing, profiling

4. **Join Community**
   - GitHub Issues: Report bugs, suggest features
   - Discussions: Ask questions, share knowledge

---

## 📚 Additional Resources

### External References
- **Flutter Documentation** - FLUI is inspired by Flutter's architecture
- **wgpu Book** - GPU rendering backend
- **Rust Book** - Rust fundamentals

### Related Documentation
- [../../API_GUIDE.md](../../API_GUIDE.md) - API reference
- [../../PIPELINE_ARCHITECTURE.md](../../PIPELINE_ARCHITECTURE.md) - Detailed pipeline
- [../../GLOSSARY_TYPES_MAPPING.md](../../GLOSSARY_TYPES_MAPPING.md) - Flutter → FLUI mapping

---

## 💡 Pro Tips

1. **Don't read everything at once** - Pick a path that matches your goal
2. **ADRs are gold** - They explain WHY, not just WHAT
3. **PATTERNS.md is your friend** - Quick reference for everything
4. **Run examples early** - Seeing code in action helps understanding
5. **Use tracing** - `RUST_LOG=debug` shows what's happening internally

---

**Ready to start?** Choose your path above and dive in! 🚀

**Questions?** Check [README.md](README.md#common-questions) or open an issue.
