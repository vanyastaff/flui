<!--
Sync Impact Report:
Version change: 2.0.0 → 1.2.0
Rationale: Rolling back overcomplex 2.0.0 to cleaner 1.2.0 focused on user's four key requirements:
  1. Code Quality Standards
  2. Testing Standards
  3. User Experience Consistency
  4. Performance Requirements

Modified principles:
  - Principle VI: Split Production Quality into Code Quality + Observability subsections
  - Principle VIII: NEW - User Experience Consistency (addresses user request)
  - Principle IX: NEW - Performance Requirements with quantified targets (addresses user request)
  - Architecture Standards: Added ID Offset Pattern documentation
  - Quality Gates: Enhanced Testing Standards with concrete coverage targets
Added sections:
  - User Experience Consistency principle with API patterns and accessibility
  - Performance Requirements with frame budgets and optimization rules
  - Testing Standards subsection with test pyramid and utilities
  - Observability Standards subsection with tracing patterns
Removed sections: None from original 1.0.0
Templates requiring updates:
  ✅ plan-template.md - Constitution Check references all updated principles
  ✅ spec-template.md - User scenarios align with UX consistency principle
  ✅ tasks-template.md - Test-first methodology and parallel execution patterns
Follow-up TODOs: None
-->

# FLUI Framework Constitution

## Core Principles

### I. Flutter-Inspired Architecture (NON-NEGOTIABLE)
FLUI adopts the proven three-tree architecture from Flutter: View Tree (immutable) → Element Tree (mutable) → Render Tree (layout/paint). This separation of concerns is mandatory for all UI components. Widget-style declarative API must be intuitive without requiring web development knowledge (HTML/CSS). All public APIs should mirror Flutter naming conventions (Container, Row, Column, BuildContext, etc.) while adapting to Rust idioms.

**API Style:**
- Builder pattern with bon for complex widgets: `Container::builder().padding(...).child(...).build()`
- Method chaining for fluent configuration
- NO CSS-like abbreviations (p-2, mx-4) - use explicit names: `padding()`, `margin()`

**Rationale**: 10+ years of production use in Flutter proves this architecture's scalability. Familiar API reduces learning curve for Flutter developers while remaining accessible to non-web developers.

### II. Type Safety First (NON-NEGOTIABLE)
Leverage Rust's type system to prevent bugs at compile time. Generic Unit system (Pixels, DevicePixels, ScaledPixels) prevents unit confusion. Arity system (Leaf, Single, Optional, Variable) enforces child count constraints. Typestate pattern (Mounted/Unmounted, Idle/Active) prevents invalid state transitions. Typed IDs (ElementId, RenderId, LayerId) cannot be mixed. All type safety mechanisms must be zero-cost abstractions.

**Foundation vs Application Types:**
- Foundation crates (flui_types, flui-tree) MAY use generics for reusability
- Application crates (flui_interaction, flui_app, flui_widgets) MUST use concrete types
- Monomorphic public APIs prevent type inference errors and improve error messages

**Example:**
```rust
// ✅ Foundation: Generic for reusability
pub struct Offset<T: Unit> { pub dx: T, pub dy: T }

// ✅ Application: Concrete types only
pub struct PointerEventData {
    pub position: Offset<Pixels>,       // Concrete!
    pub movement: Offset<PixelDelta>,   // Concrete!
}
```

**Rationale**: Runtime errors in UI frameworks are expensive and hard to debug. Compile-time guarantees eliminate entire classes of bugs. GPUI successfully uses monomorphic application types.

### III. Modular Architecture
Framework organized into 20+ specialized crates with clear dependency hierarchy: Foundation (types, tree, foundation) → Core (view, reactivity, scheduler) → Rendering (painting, engine, rendering) → Widget (widgets, animation, interaction) → Application (app, assets). Each crate must be independently testable and documented. Cross-cutting concerns (logging, platform abstraction) live in separate crates.

**Dependency Rules:**
- Lower layers NEVER depend on upper layers
- Optional features via Cargo feature flags
- Single responsibility per crate

**Rationale**: Modularity enables independent development, testing, and evolution of components. Users can depend on only what they need.

### IV. Test-First for Public APIs (NON-NEGOTIABLE)
All public trait methods and APIs must have tests written before implementation. Tests define the contract. Red-Green-Refactor cycle strictly enforced for new features. Integration tests required for: new crate interactions, API contract changes, cross-crate communication, and shared types. Internal implementation details may be tested after implementation.

**Tests MUST fail before implementation** - verify red state before proceeding to green.

**Coverage Requirements:**
- Core crates (foundation, view, rendering): ≥80%
- Platform crates: ≥70% (native APIs harder to test)
- Widget crates: ≥85% (user-facing, critical)

**Rationale**: Public APIs are difficult to change once released. Test-first ensures APIs are usable and correct before users depend on them.

### V. Explicit Over Implicit
Lifecycle methods (mount, update, unmount) must be explicit and visible. No hidden state mutations or magic behavior. Interior mutability (RwLock, Mutex) preferred over RefCell for multi-threaded safety. Platform abstraction through explicit traits (Platform, PlatformWindow) rather than conditional compilation. Source location tracking via #[track_caller] for debugging, not hidden behavior.

**Rationale**: Rust philosophy values explicitness. Hidden behavior makes debugging difficult and violates principle of least surprise.

### VI. Code Quality Standards (NON-NEGOTIABLE)

All code must meet production readiness criteria.

**Logging (MANDATORY):**
```rust
// ✅ ALWAYS use tracing
use tracing::{debug, info, warn, error, instrument};

#[instrument]
fn render_frame(frame_num: u32) {
    info!("Starting frame render");
    debug!(frame_num, "Frame details");
}

// ❌ NEVER use println! or eprintln!
```

**Observability Requirements:**
- #[tracing::instrument] on all public methods
- Structured fields for context (count=N, size=XxY, duration_ms=T)
- Error paths must emit tracing::error! with context
- Performance-critical paths emit tracing::debug! with timing

**Error Handling:**
```rust
// ✅ Descriptive errors with context (use thiserror)
#[derive(Error, Debug)]
pub enum PlatformError {
    #[error("Failed to create window: {0}")]
    WindowCreationFailed(String),
}
```

**Thread Safety:**
- parking_lot::RwLock for shared state (2-3x faster than std)
- DashMap for concurrent collections
- Atomic types for flags and counters (lock-free dirty tracking)
- Arc for shared ownership, never raw pointers

**Frame Scheduling:**
```rust
// ✅ On-demand rendering (ControlFlow::Wait)
event_loop.set_control_flow(ControlFlow::Wait);
// Request frames only when needed: state changes, animations, resize

// ❌ NOT constant 60 FPS loop (wastes CPU/battery)
```

**Rationale**: Production applications require observable, debuggable, and performant code. These standards are proven in the existing codebase.

### VII. Incremental Development
Features delivered as independently testable user stories with priorities (P1, P2, P3). Each story must be deployable and demonstrable on its own. Parallel development enabled through [P] task markers for independent work. Phase gates ensure foundation complete before user story work begins. Version numbering follows semantic versioning: MAJOR (breaking changes), MINOR (new features), PATCH (bug fixes).

**User Story Structure:**
- P1: Critical MVP functionality (implement first)
- P2: Important enhancements (implement after P1)
- P3: Nice-to-have features (implement after P2)
- Each story: independently testable, deliverable, demonstrable

**Rationale**: Incremental delivery provides value early and reduces integration risk. Priority-driven development focuses effort on most valuable features first.

### VIII. User Experience Consistency (NON-NEGOTIABLE)

All widgets must follow Flutter's mental model: declarative configuration, immutable properties, reactive updates. Layout behavior must be predictable and match Flutter semantics (constraints go down, sizes go up, parent sets position). Animation and interaction patterns must be consistent across widgets.

**API Consistency Rules:**
- Builder pattern for complex widgets (bon crate)
- Method chaining for fluent configuration
- `child`/`children` naming for single/multiple children
- Edge cases documented with examples (empty lists, null-like states)
- Panic conditions documented explicitly

**Layout Semantics (Flutter Model):**
```
1. Constraints go down (parent constrains child)
2. Sizes go up (child reports size to parent)
3. Parent sets position (child doesn't position itself)
```

**Error Messages:**
- Actionable with clear guidance
- Include context (what failed, why, how to fix)
- Reference documentation where applicable

**Documentation:**
- Visual examples for complex widgets
- Common patterns and recipes
- Pitfalls and gotchas sections

**Accessibility Requirements:**
- Semantic labels for all interactive widgets
- Keyboard navigation support where applicable
- Screen reader descriptions via semantics tree
- Focus management follows predictable patterns

**Rationale**: Consistent UX reduces cognitive load and learning curve. Users should predict behavior from widget names without reading documentation. Flutter's success validates this approach.

### IX. Performance Requirements (NON-NEGOTIABLE)

**Frame Budget: 16.67ms for 60fps (target: <12ms for margin)**

**Phase Budgets:**
- Layout phase: <5ms for typical widget tree (<1000 nodes)
- Paint phase: <8ms for typical scene
- Build phase: <3ms for incremental rebuilds

**Memory Targets:**
- <100MB for typical application state
- Startup: <2s cold start, <500ms warm start

**Hot Path Requirements (Layout/Paint):**
- Zero allocations where possible (use stack buffers, pool reuse)
- Inline small functions (<10 LOC)
- Prefer iteration over recursion
- Cache computed values (intrinsic sizes, paint bounds)
- Profile before optimizing (no premature optimization)

**Lock-Free Operations:**
```rust
// ✅ Atomic dirty flags (lock-free)
pub struct RenderObject {
    needs_layout: AtomicBool,
    needs_paint: AtomicBool,
}

impl RenderObject {
    pub fn mark_needs_layout(&mut self) {
        if self.needs_layout.load(Ordering::Relaxed) {
            return;  // Already marked
        }
        self.needs_layout.store(true, Ordering::Relaxed);
        self.pipeline_owner.add_to_layout_list(self.id);
    }
}
```

**Memory Management:**
- Slab allocators for tree nodes (amortized O(1) insert/remove)
- Object pooling for transient objects (paint contexts, constraints)
- Weak references to break cycles (parent/child relationships)
- Drop impls for cleanup (detach from owner, release resources)

**Benchmark Requirements:**
- criterion benchmarks for performance-critical code
- Regression tests with ±5% tolerance
- Profile reports in CI for major changes
- Flame graphs for investigating slowdowns

**Profiling Tools:**
```bash
# Microbenchmarks
cargo bench -p flui_rendering

# Hotspot analysis
cargo flamegraph --example complex_layout

# Tracing hierarchical timing
RUST_LOG=trace cargo run --example stress_test
```

**Rationale**: UI frameworks are performance-critical. Users expect smooth 60fps with low memory usage. Quantified targets enable objective optimization decisions. Flutter's architecture enables this performance.

## Architecture Standards

### Three-Tree Implementation
- **View Tree**: Immutable configurations implementing View trait with single build() method
- **Element Tree**: Mutable state stored in Slab, lifecycle managed by BuildOwner, ElementId uses NonZeroUsize (Option<ElementId> = 8 bytes)
- **Render Tree**: Layout/paint logic, arity-based type safety, PipelineOwner manages dirty tracking and flush phases

**ID Offset Pattern (CRITICAL):**
```rust
// Slab uses 0-based indices, IDs use 1-based (NonZeroUsize for niche optimization)

// Insert: slab_index + 1 = ID
let slab_index = self.nodes.insert(node);
let id = ElementId::new(slab_index + 1);

// Access: ID - 1 = slab_index
self.nodes.get(element_id.get() - 1)
```

Applies to: ViewId, ElementId, RenderId, LayerId, SemanticsId.

### Pipeline Phases (Flutter-Compatible)
Build (WidgetsBinding) → Layout (PipelineOwner) → Compositing (layer tree) → Paint (display lists) → Semantics (accessibility). Each phase processes dirty flags from previous phase. Phases must not be mixed (enforced via phase tracking in V2).

**Dirty Tracking:**
- needs_build flag set by state changes
- needs_layout flag set by constraint/size changes
- needs_paint flag set by visual property changes
- needs_compositing_bits_update flag set by layer changes

### Platform Abstraction
Platform trait defines lifecycle (run, quit), window management (open, close), display queries, executors (async runtime), text system (fonts), and clipboard. Implementations: WindowsPlatform (native Win32), WinitPlatform (cross-platform), HeadlessPlatform (testing). Callback registry pattern (GPUI-inspired) for decoupling framework from platform.

**Event Architecture (W3C Standard):**
```rust
// Use ui-events crate throughout
use ui_events::pointer::PointerEvent;
use ui_events::keyboard::KeyboardEvent;
use keyboard_types::{Key, Modifiers};
use cursor_icon::CursorIcon;

// Platform layer converts OS → W3C
// Application layer uses W3C types directly
```

### Reactive Patterns (V3 Roadmap)
- **Lens Pattern** (Druid): Type-safe data access via #[derive(Lens)]
- **Elm Architecture** (Iced): Message-based updates with update() method
- **Adapt Nodes** (Xilem): Component composition and reuse
- **Command System**: Async effects decoupled from UI updates
- **Subscriptions**: External event listeners (timers, websockets)

**Current State (V1):** Basic reactive signals via flui-reactivity with Copy-based Signal<T> and computed values. Full Elm/Lens patterns planned for V3 after rendering stabilization.

## Quality Gates

### Compilation Gate
All workspace crates must compile with zero errors. Warnings must be addressed or explicitly allowed with justification. Clippy must pass with -D warnings. Feature flags must be documented and tested in CI.

**Build Order (dependency-aware):**
```bash
cargo build -p flui_types
cargo build -p flui-foundation
cargo build -p flui-tree
cargo build -p flui_rendering
cargo build --workspace
```

### Testing Standards

**Test Pyramid:**
1. **Unit Tests** (fast, isolated, many) - All public functions and methods
2. **Integration Tests** (cross-crate, moderate) - API interactions
3. **Contract Tests** (public APIs, critical) - Platform trait implementations
4. **End-to-End** (full stack, few) - Complete user flows

**Test Organization:**
```rust
// Unit tests: tests/ directory (preferred) or #[cfg(test)] mod tests
#[test]
fn test_container_padding() { /* ... */ }

// Integration tests: tests/ directory only
#[test]
fn test_view_element_render_integration() { /* ... */ }
```

**Test Naming Convention (MANDATORY):**
```rust
// ✅ CORRECT: Descriptive names and comments without task references
#[test]
fn test_displays_enumeration() {
    // Verify platform returns all connected displays with valid properties
}

#[test]
fn test_primary_display_detection() {
    // Ensure exactly one display is marked as primary by the OS
}

#[test]
fn test_high_dpi_scale_factor() {
    // Validate HiDPI/Retina displays report scale factor >= 1.5
}

// ❌ WRONG: Task numbers in function names or comments
#[test]
fn test_t073_displays_enumeration() { /* BAD: T073 in name */ }

#[test]
fn test_displays_enumeration() {
    // T073: Test display enumeration  /* BAD: T073 in comment */
}

#[test]
fn test_displays_enumeration() { /* T073 */ /* BAD: will become orphaned */ }
```

**Test Naming and Documentation Rules:**
- Use descriptive names that explain WHAT is being tested (behavior/feature)
- NEVER include task numbers (T073, T080) in function names
- NEVER reference task numbers in comments (specs/tasks get deleted, comments remain)
- Use underscores to separate words (snake_case)
- Start with `test_` prefix for test discovery
- Add category prefix for large test suites: `test_window_`, `test_display_`, `test_event_`
- Comments should describe the test's PURPOSE, not reference external tracking
- Names and comments should be self-documenting without referencing task lists or specs

**Test Infrastructure:**
```rust
// Headless platform for CI (no GPU required)
FLUI_HEADLESS=1 cargo test --workspace

// Gesture testing utilities
use flui_interaction::testing::{GestureRecorder, GesturePlayer};

let mut recorder = GestureRecorder::new();
recorder.record_tap(position);
recorder.record_drag(start, end);
let sequence = recorder.finish();

// Replay in tests (deterministic)
let mut player = GesturePlayer::new(sequence);
player.play_next();
```

**Coverage Targets:**
- Foundation crates (types, tree, foundation): ≥80%
- Core framework (view, scheduler, rendering): ≥80%
- Widget library: ≥85%
- Platform integration: ≥70% (native APIs harder to test)

### Documentation Gate
Every public type, trait, method, and module must have doc comments. Examples required for non-trivial APIs. Each crate must have README.md with purpose, usage, and examples. Architecture decisions documented as ADRs in docs/plans/. CLAUDE.md maintained with development guidelines.

**Documentation Requirements:**
```rust
/// Canvas provides a 2D drawing API for recording graphical operations.
///
/// # Example
/// ```
/// let mut canvas = Canvas::new();
/// canvas.draw_rect(rect, &Paint::fill(Color::RED));
/// let picture = canvas.finish();
/// ```
///
/// # Panics
/// Panics if operations called after `finish()`.
pub struct Canvas { /* ... */ }
```

### Performance Gate
No allocations in hot paths (layout, paint). Lock contention measured and minimized. Frame budget tracked (16.67ms for 60fps). Benchmarks required for performance-critical code. Profile before optimizing (no premature optimization).

**Performance Verification:**
```bash
# Run benchmarks
cargo bench --package flui_rendering

# Profile example
cargo flamegraph --example complex_layout

# Memory analysis
RUST_LOG=trace cargo run --example stress_test
```

**Optimization Rules:**
- **NO allocations** in: layout(), paint(), hit_test()
- **Lock-free** dirty tracking (AtomicBool)
- **Batch** similar GPU operations
- **Cache** expensive computations (Pictures, tessellated paths)
- **Profile first** before optimizing (no premature optimization)

## Governance

**Authority**: This constitution supersedes all other development practices and documentation. In case of conflict, constitution takes precedence.

**Amendment Process**: Constitution changes require: written proposal with rationale, review by project maintainers, version bump per semantic versioning rules, documentation of impacts on templates and dependent files, sync report prepended to this file.

**Compliance Review**: All PRs must verify alignment with constitution principles. Code reviews must check: type safety (no unsafe unless justified), test coverage (APIs tested first), documentation completeness, performance considerations, and explicit behavior (no magic).

**Complexity Justification**: Any violation of principles (e.g., introducing unsafe, skipping tests, hidden behavior) must be explicitly justified in PR description and reviewed by maintainers. Use plan-template.md "Complexity Tracking" section to document exceptions.

**Version Management**: Constitution versioned semantically. MAJOR bump for incompatible changes (removing principles, redefining non-negotiables). MINOR bump for additions (new principles, expanded guidance). PATCH bump for clarifications and typo fixes.

**Runtime Guidance**: For practical development instructions, see CLAUDE.md. For architectural context, see docs/ARCHITECTURE_OVERVIEW.md and docs/PROJECT_PHILOSOPHY.md.

**Version**: 1.2.0 | **Ratified**: 2026-01-26 | **Last Amended**: 2026-01-26
