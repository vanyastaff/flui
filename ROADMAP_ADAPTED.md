# FLUI Development Roadmap - Adapted to Current Architecture

## 🎯 Current Architecture

**Your Widget Types:**
```rust
Widget (trait)
├─ StatelessWidget  - Immutable, build() -> BoxedWidget
├─ StatefulWidget   - Mutable state, State::build() -> BoxedWidget
├─ RenderObjectWidget - Layout & Paint (Leaf/Single/Multi Arity)
├─ InheritedWidget  - Data propagation
├─ ParentDataWidget - Layout metadata (Expanded, Flexible)
└─ ProxyWidget      - Base for wrappers
```

**Current Boxing:** `Box<dyn DynWidget>` everywhere

---

## 📅 Roadmap: Pre-1.0 (12 weeks)

### Phase 1: Foundation Optimizations (Weeks 1-4)

#### Week 1-2: Smart Boxing Strategy ⚠️ CRITICAL

**Problem:** `Box<dyn DynWidget>` везде = many heap allocations

**Solution:** Optimization на уровне компилятора + runtime

```rust
// ❌ ТЕКУЩЕЕ: всегда boxing
fn build(&self) -> BoxedWidget {
    Box::new(Text::new("Hello"))  // ← Heap allocation!
}

// ✅ УЛУЧШЕНИЕ 1: Inline small widgets
// Для StatelessWidget можно кэшировать результат build()
pub struct ComponentElement<W: StatelessWidget> {
    widget: W,
    cached_child: Option<BoxedWidget>,  // ← Cache build result
}

// ✅ УЛУЧШЕНИЕ 2: Stack allocation где возможно
// Для RenderObjectWidget - элемент владеет typed RenderObject
pub struct LeafRenderObjectElement<W: LeafRenderObjectWidget> {
    widget: W,
    render_object: W::Render,  // ← Не боксим RenderObject!
}

// ✅ УЛУЧШЕНИЕ 3: SmallBox optimization
// Для маленьких виджетов используем stack storage
enum WidgetStorage {
    Inline([u8; 64]),  // Маленькие виджеты на стеке
    Boxed(BoxedWidget), // Большие на куче
}
```

**Tasks:**

**Day 1-3: Analysis & Design**
- [ ] Measure current allocation patterns
  - Сколько allocations на frame?
  - Какие виджеты самые частые? (Text, Container, Row, Column)
  - Profile в реальном примере (flex_layout_simple.rs)
- [ ] Design optimization strategy:
  - Option A: Caching в ComponentElement
  - Option B: SmallBox optimization
  - Option C: Better RenderObject ownership
  - Decision: Prioritize based on measurements

**Day 4-6: Implementation (Phased)**
- [ ] **Phase A:** ComponentElement caching
  ```rust
  impl<W: StatelessWidget> ComponentElement<W> {
      fn rebuild(&mut self) {
          // ✅ Rebuild только если widget changed
          if self.needs_rebuild {
              self.cached_child = Some(self.widget.build());
              self.needs_rebuild = false;
          }
      }
  }
  ```
- [ ] **Phase B:** RenderObject ownership improvements
  ```rust
  // Instead of Box<dyn DynRenderObject>
  pub struct LeafRenderObjectElement<W: LeafRenderObjectWidget> {
      widget: W,
      render_object: W::Render,  // ← Direct ownership
  }
  ```
- [ ] Add benchmarks

**Day 7-10: Testing & Validation**
- [ ] Update all examples
- [ ] Benchmark comparison (before/after)
- [ ] Profile memory usage
- [ ] Document performance improvements

**Success Criteria:**
- ✅ Измеримое reduction в allocations (target: 30-50%)
- ✅ No performance regressions
- ✅ All examples work
- ⚠️ Non-breaking changes (keep BoxedWidget API)

---

#### Week 3: Signal System Design

**Goal:** Add reactive signals to State trait

**Current State Management:**
```rust
#[derive(Debug)]
struct CounterState {
    count: i32,  // ← Plain field, no reactivity
}

impl State for CounterState {
    fn build(&mut self) -> BoxedWidget {
        // ❌ Manually rebuilds everything
        Box::new(Text::new(format!("Count: {}", self.count)))
    }
}
```

**Improved with Signals:**
```rust
use flui_core::reactive::Signal;

#[derive(Debug)]
struct CounterState {
    count: Signal<i32>,  // ← Reactive!
}

impl State for CounterState {
    type Widget = Counter;

    fn init_state(&mut self) {
        // Initialize signals
        self.count = Signal::new(0);
    }

    fn build(&mut self) -> BoxedWidget {
        // ✅ Automatically tracks dependencies
        Box::new(Column::builder()
            .children(vec![
                Box::new(Text::new(format!("Count: {}", self.count.get()))),
                Box::new(button_widget()),
            ])
            .build())
    }
}
```

**Tasks:**

**Day 1-2: Design Signal API**
- [ ] Design Signal<T> implementation (from Chapter 11)
- [ ] Design integration with State trait
- [ ] Write examples

**Day 3-5: Implementation**
- [ ] Implement Signal<T> with Rc
- [ ] Implement reactive scope tracking
- [ ] Add to flui_core
- [ ] Integration tests

**Day 6-7: Ergonomics**
- [ ] Extension traits for Signal
- [ ] Helper macros (clone!)
- [ ] Update counter example
- [ ] Documentation

**Success Criteria:**
- ✅ Signals work in State
- ✅ Automatic dependency tracking
- ✅ Clean API

---

#### Week 4: BuildContext Enhancements

**Goal:** Rich BuildContext API для State::build()

**Current:**
```rust
fn build(&mut self) -> BoxedWidget {
    // ❌ No context, no DI, no effects
}
```

**Improved:**
```rust
fn build(&mut self, cx: &mut BuildContext) -> BoxedWidget {
    // ✅ Access to context
    let theme = cx.get::<Theme>()?;

    // ✅ Effects with cleanup
    cx.use_effect(
        self.count.get(),
        || {
            println!("Count changed!");
            Box::new(|| println!("Cleanup"))
        }
    );

    // ✅ Signals via context
    let (value, set_value) = cx.use_signal(0);

    Box::new(Text::new(format!("Count: {}", self.count.get())))
}
```

**Tasks:**

**Day 1-2: Design BuildContext**
- [ ] Design BuildContext struct
  ```rust
  pub struct BuildContext<'a> {
      element_id: ElementId,
      tree: &'a ElementTree,
      services: &'a ServiceContainer,
      effects: &'a mut EffectRegistry,
  }
  ```
- [ ] Design methods: `get<T>()`, `use_effect()`, `use_signal()`

**Day 3-5: Implementation**
- [ ] Implement BuildContext
- [ ] Update State trait signature
- [ ] Implement effect system
- [ ] Add service container

**Day 6-7: Migration**
- [ ] Update all State implementations
- [ ] Update examples
- [ ] Migration guide for users

**Success Criteria:**
- ✅ BuildContext provides needed APIs
- ✅ Effects work correctly
- ✅ Clean migration path

---

### Phase 2: Core Features (Weeks 5-8)

#### Week 5: Context/Provider System

**Goal:** InheritedWidget improvements для DI

**Current InheritedWidget:**
```rust
pub trait InheritedWidget {
    fn update_should_notify(&self, old: &Self) -> bool;
    fn child(&self) -> BoxedWidget;
}
```

**Enhanced with Provider API:**
```rust
// Provider - удобная обертка над InheritedWidget
pub struct Provider<T: Clone + 'static> {
    value: T,
    child: BoxedWidget,
}

impl<T: Clone + 'static> Provider<T> {
    pub fn new(value: T) -> ProviderBuilder<T> {
        ProviderBuilder { value, child: None }
    }
}

impl<T: Clone + 'static> InheritedWidget for Provider<T> {
    fn update_should_notify(&self, old: &Self) -> bool {
        // Use PartialEq if available
        true  // Conservative default
    }

    fn child(&self) -> BoxedWidget {
        self.child.clone()
    }
}

// BuildContext integration
impl BuildContext {
    pub fn get<T: 'static>(&self) -> Option<&T> {
        // Walk up tree to find Provider<T>
        self.tree.find_inherited::<Provider<T>>()
            .map(|provider| &provider.value)
    }
}
```

**Usage:**
```rust
fn app() -> BoxedWidget {
    Box::new(
        Provider::new(Theme::dark())
            .child(Box::new(
                Provider::new(User::current())
                    .child(Box::new(MyApp))
            ))
    )
}

// In State::build()
fn build(&mut self, cx: &BuildContext) -> BoxedWidget {
    let theme = cx.get::<Theme>().unwrap();
    let user = cx.get::<User>().unwrap();

    Box::new(Text::new(format!("Hello, {}!", user.name))
        .color(theme.primary_color))
}
```

**Tasks:**
- [ ] Implement Provider<T> wrapper
- [ ] Add BuildContext::get<T>()
- [ ] Examples (Theme, User, etc.)
- [ ] Tests

---

#### Week 6: Widget API Consistency

**Current Issues:**
```rust
// Inconsistent child/children
Container { child: Some(Box::new(widget)) }
Row { children: vec![Box::new(w1), Box::new(w2)] }

// Builder patterns inconsistent
Text::builder().data("Hello").size(24.0).build()
Container::builder().child(widget).color(Color::RED).build()
```

**Standardization:**

**Rule 1: Constructors**
```rust
// ✅ Simple cases: new()
Text::new("Hello")
Container::new()

// ✅ With main param: new(param)
Padding::new(EdgeInsets::all(16.0))
Opacity::new(0.5)

// ✅ Complex: builder()
Container::builder()
    .width(100.0)
    .height(200.0)
    .color(Color::RED)
    .build()
```

**Rule 2: Children**
```rust
// ✅ Single child: .child()
Container::new().child(Box::new(widget))
Padding::new(16.0).child(Box::new(widget))

// ✅ Multiple: .children()
Row::new().children(vec![Box::new(w1), Box::new(w2)])
Column::new().children(vec![...])
```

**Rule 3: Properties**
```rust
// ✅ Full names (no abbreviations)
.width(100.0)     // Not .w()
.padding(16.0)    // Not .p()
.background(...)  // Not .bg()
```

**Tasks:**
- [ ] Audit all widget APIs
- [ ] Create consistency doc
- [ ] Refactor widgets
- [ ] Update examples

---

#### Weeks 7-8: Complete Widget Library

**Essential Widgets for 1.0:**

**Layout (Week 7):**
- [x] Container ✅
- [x] Row, Column ✅
- [x] Padding ✅
- [ ] Stack (z-axis layering)
- [ ] Flex (improved)
- [ ] SizedBox
- [ ] Spacer
- [ ] Align, Center

**Basic (Week 7):**
- [x] Text ✅
- [ ] Image
- [ ] Icon
- [ ] Button (clickable container)
- [ ] IconButton

**Input (Week 8):**
- [ ] TextField (with cursor, selection)
- [ ] Checkbox
- [ ] Radio
- [ ] Switch
- [ ] Slider

**Scrolling (Week 8):**
- [ ] ScrollView
- [ ] ListView
- [ ] ListView.builder (virtualized)
- [ ] GridView

**Advanced (Week 8):**
- [x] Opacity ✅
- [ ] Transform (rotate, scale, translate)
- [ ] ClipRect, ClipRRect, ClipPath
- [ ] GestureDetector (basic)

**Tasks per widget:**
- [ ] Implement widget + builder
- [ ] Write tests
- [ ] Create example
- [ ] Document API
- [ ] Add to widget gallery

---

### Phase 3: Production Ready (Weeks 9-12)

#### Week 9: Testing & Benchmarking

**Test Categories:**

**1. Unit Tests**
```rust
#[test]
fn test_text_widget_updates() {
    let text1 = Text::new("Hello");
    let text2 = Text::new("World");
    assert!(text1.can_update(&text2));  // Same type
}

#[test]
fn test_row_layout() {
    // Test that Row lays out children correctly
}
```

**2. Integration Tests**
```rust
#[test]
fn test_stateful_widget_lifecycle() {
    // Test create_state -> init_state -> build -> dispose
}
```

**3. Property-Based Tests**
```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn layout_respects_constraints(
        width in 0.0f32..1000.0,
        height in 0.0f32..1000.0
    ) {
        let constraints = BoxConstraints::tight(Size::new(width, height));
        let size = container().layout(constraints);
        assert!(size.width <= width);
        assert!(size.height <= height);
    }
}
```

**4. Benchmarks**
```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_layout_1000_widgets(c: &mut Criterion) {
    c.bench_function("layout_1000", |b| {
        b.iter(|| {
            // Layout tree with 1000 widgets
            layout_tree(black_box(1000))
        });
    });
}

criterion_group!(benches, bench_layout_1000_widgets);
criterion_main!(benches);
```

**Tasks:**
- [ ] Unit tests for all widgets (target: 80%+ coverage)
- [ ] Integration tests for framework
- [ ] Property tests for layout
- [ ] Benchmarks for critical paths
- [ ] CI/CD setup

**Success Metrics:**
- ✅ Coverage >80%
- ✅ Layout <5ms for 1000 widgets
- ✅ No memory leaks
- ✅ All benchmarks passing

---

#### Week 10: Documentation

**Documentation Structure:**

**1. Getting Started**
```markdown
# Getting Started with FLUI

## Installation
[dependencies]
flui = "0.1"

## Your First Widget
...

## Understanding State
...
```

**2. API Reference (cargo doc)**
```rust
/// Text widget displays a string of text with a single style.
///
/// # Examples
///
/// ```
/// use flui::widgets::Text;
///
/// let text = Text::new("Hello, World!")
///     .size(24.0)
///     .color(Color::rgb(255, 0, 0));
/// ```
///
/// # See Also
/// - [RichText] for multi-style text
/// - [TextStyle] for styling options
pub struct Text {
    pub data: String,
    pub size: f32,
    pub color: Color,
    // ...
}
```

**3. Examples**
- [ ] hello_world (simplest)
- [ ] counter (stateful)
- [ ] todo_app (signals + state)
- [ ] dashboard (complex layout)
- [ ] custom_widget (RenderObject)

**4. Guides**
- [ ] Widget Types Explained
- [ ] State Management
- [ ] Layout System
- [ ] Performance Optimization
- [ ] Migration from Flutter

**Tasks:**
- [ ] Write all guides
- [ ] Complete API docs
- [ ] Update all examples
- [ ] Create example gallery website

---

#### Week 11: Migration Guides & Tooling

**Flutter Migration Guide:**

```markdown
| Flutter | FLUI |
|---------|------|
| `StatelessWidget` | `impl StatelessWidget` |
| `StatefulWidget` | `impl StatefulWidget + State` |
| `setState(() => ...)` | `Signal::set()` + auto-rebuild |
| `BuildContext.of<T>()` | `BuildContext.get<T>()` |
| `InheritedWidget` | `InheritedWidget` (same!) |
| `Container(child: ...)` | `Container::new().child(...)` |
| `Row(children: [...])` | `Row::new().children(vec![...])` |

# Example Migration

Flutter:
\`\`\`dart
class Counter extends StatefulWidget {
  @override
  State<Counter> createState() => _CounterState();
}

class _CounterState extends State<Counter> {
  int count = 0;

  @override
  Widget build(BuildContext context) {
    return Text('Count: $count');
  }
}
\`\`\`

FLUI:
\`\`\`rust
#[derive(Debug, Clone)]
struct Counter;

impl StatefulWidget for Counter {
    type State = CounterState;
    fn create_state(&self) -> Self::State {
        CounterState { count: Signal::new(0) }
    }
}

#[derive(Debug)]
struct CounterState {
    count: Signal<i32>,
}

impl State for CounterState {
    type Widget = Counter;

    fn build(&mut self, cx: &BuildContext) -> BoxedWidget {
        Box::new(Text::new(format!("Count: {}", self.count.get())))
    }
}
\`\`\`
```

**Tasks:**
- [ ] Flutter migration guide
- [ ] React migration examples
- [ ] Common patterns translation
- [ ] Troubleshooting FAQ

---

#### Week 12: Polish & Release

**Pre-Release Checklist:**

**Code Quality:**
- [ ] All public APIs documented
- [ ] No `todo!()` or `unimplemented!()` in public code
- [ ] All tests passing
- [ ] Benchmarks meet targets
- [ ] No clippy warnings
- [ ] cargo fmt applied

**Documentation:**
- [ ] README.md complete
- [ ] CHANGELOG.md from commits
- [ ] API docs complete (cargo doc)
- [ ] Examples all work
- [ ] Guides complete

**Infrastructure:**
- [ ] CI/CD working
- [ ] crates.io metadata correct
- [ ] GitHub releases setup
- [ ] docs.rs builds successfully

**Release Tasks:**
- [ ] Version bump to 1.0.0
- [ ] Tag release
- [ ] Publish to crates.io
- [ ] GitHub release with notes
- [ ] Announce on:
  - [ ] Reddit r/rust
  - [ ] Hacker News
  - [ ] Twitter/X
  - [ ] Discord
  - [ ] This Week in Rust

**Success:** 🎉 FLUI 1.0 is live!

---

## 📊 Key Differences from Generic Plan

### Your Architecture Strengths:
1. ✅ **Already have DynWidget** - object-safe trait working
2. ✅ **Clean separation** - Widget types well-defined
3. ✅ **Arity system** - type-safe child counts
4. ✅ **Element lifecycle** - solid foundation

### Focus Areas:
1. ⚠️ **Optimize boxing** - reduce `Box<dyn DynWidget>` overhead
2. ⚠️ **Add reactivity** - Signal system for State
3. ⚠️ **Enhance BuildContext** - effects, DI, utilities
4. ⚠️ **Complete widgets** - full library for 1.0
5. ⚠️ **Polish API** - consistency across widgets

### Non-Goals (Keep Current):
- ❌ **Don't** replace DynWidget (works fine!)
- ❌ **Don't** change Widget trait (solid!)
- ❌ **Don't** change Element system (good architecture!)
- ✅ **Do** optimize on top of current foundation

---

## 🎯 Success Criteria for 1.0

### Technical:
- ✅ 30-50% fewer allocations (via caching + optimizations)
- ✅ Layout <5ms for 1000 widgets
- ✅ Signals working in State
- ✅ Complete widget library (30+ widgets)
- ✅ Test coverage >80%

### Developer Experience:
- ✅ Clear widget types (Stateless, Stateful, Render, etc.)
- ✅ Ergonomic signal API
- ✅ Rich BuildContext
- ✅ Consistent widget APIs
- ✅ Great documentation

### Production Ready:
- ✅ No known critical bugs
- ✅ Stable API (semver from 1.0)
- ✅ Migration guides
- ✅ Examples for all use cases

---

**Next Steps:**

1. Review this adapted plan
2. Agree on priorities (Week 1-2 focus?)
3. Set up project board
4. Start implementation!

**Вопросы? Что думаете о плане?** 🚀
