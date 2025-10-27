# Enum Widget Migration Plan

## Goal
Add `AnyWidget` enum to eliminate 90% of heap allocations while keeping existing `DynWidget` system for dynamic cases.

---

## Phase 1: Add Enum Without Breaking Changes (Week 1)

### Step 1: Define AnyWidget Enum

```rust
// crates/flui_core/src/widget/any_widget.rs

use crate::widget::*;
use crate::element::*;

/// Enum wrapper for common widgets to avoid boxing
///
/// Use this for static widget trees where types are known at compile time.
/// Falls back to `Box<dyn DynWidget>` for truly dynamic cases.
#[derive(Debug, Clone)]
pub enum AnyWidget {
    // Basic widgets
    Text(Text),

    // Layout widgets (recursive)
    Container(Container<AnyWidget>),
    Row(Row<Vec<AnyWidget>>),
    Column(Column<Vec<AnyWidget>>),

    // Stateful widgets
    Button(Button),

    // Fallback to dynamic dispatch for unknown types
    Dynamic(BoxedWidget),
}

impl Widget for AnyWidget {
    type Element = AnyElement;

    fn key(&self) -> Option<&str> {
        match self {
            AnyWidget::Text(w) => w.key(),
            AnyWidget::Container(w) => w.key(),
            AnyWidget::Row(w) => w.key(),
            AnyWidget::Column(w) => w.key(),
            AnyWidget::Button(w) => w.key(),
            AnyWidget::Dynamic(w) => w.key(),
        }
    }

    fn into_element(self) -> Self::Element {
        match self {
            AnyWidget::Text(w) => AnyElement::Text(w.into_element()),
            AnyWidget::Container(w) => AnyElement::Container(w.into_element()),
            AnyWidget::Row(w) => AnyElement::Row(w.into_element()),
            AnyWidget::Column(w) => AnyElement::Column(w.into_element()),
            AnyWidget::Button(w) => AnyElement::Button(w.into_element()),
            AnyWidget::Dynamic(w) => AnyElement::Dynamic(w.into_element_boxed()),
        }
    }
}

// Implement DynWidget for compatibility
impl DynWidget for AnyWidget {
    fn type_name(&self) -> &'static str {
        match self {
            AnyWidget::Text(w) => w.type_name(),
            AnyWidget::Container(w) => w.type_name(),
            AnyWidget::Row(w) => w.type_name(),
            AnyWidget::Column(w) => w.type_name(),
            AnyWidget::Button(w) => w.type_name(),
            AnyWidget::Dynamic(w) => w.type_name(),
        }
    }

    fn can_update(&self, other: &dyn DynWidget) -> bool {
        // Try to downcast to AnyWidget first
        if let Some(other_any) = other.downcast_ref::<AnyWidget>() {
            std::mem::discriminant(self) == std::mem::discriminant(other_any)
        } else {
            false
        }
    }
}

// Convenience conversions
impl From<Text> for AnyWidget {
    fn from(w: Text) -> Self {
        AnyWidget::Text(w)
    }
}

impl From<Button> for AnyWidget {
    fn from(w: Button) -> Self {
        AnyWidget::Button(w)
    }
}

impl From<BoxedWidget> for AnyWidget {
    fn from(w: BoxedWidget) -> Self {
        AnyWidget::Dynamic(w)
    }
}

// ... more From implementations
```

### Step 2: Define AnyElement Enum

```rust
// crates/flui_core/src/element/any_element.rs

pub enum AnyElement {
    Text(TextElement),
    Container(ContainerElement<AnyElement>),
    Row(RowElement<Vec<AnyElement>>),
    Column(ColumnElement<Vec<AnyElement>>),
    Button(ButtonElement),
    Dynamic(BoxedElement),
}

impl Element for AnyElement {
    type Widget = AnyWidget;

    fn widget(&self) -> &Self::Widget {
        match self {
            AnyElement::Text(e) => /* ... */,
            // ...
        }
    }

    fn mount(&mut self, parent: &mut dyn Element) {
        match self {
            AnyElement::Text(e) => e.mount(parent),
            AnyElement::Container(e) => e.mount(parent),
            AnyElement::Row(e) => e.mount(parent),
            AnyElement::Column(e) => e.mount(parent),
            AnyElement::Button(e) => e.mount(parent),
            AnyElement::Dynamic(e) => e.mount(parent),
        }
    }

    fn update(&mut self, new_widget: Self::Widget) {
        match (self, new_widget) {
            (AnyElement::Text(e), AnyWidget::Text(w)) => e.update(w),
            (AnyElement::Button(e), AnyWidget::Button(w)) => e.update(w),
            // ... match all cases
            _ => panic!("Widget type changed"),
        }
    }

    // ... other Element methods
}

impl DynElement for AnyElement {
    // Delegate to inner element
}
```

---

## Phase 2: Update Layout Widgets to Accept Generic Children (Week 1)

### Current Problem:
```rust
// crates/flui_widgets/src/layout/column.rs

pub struct Column {
    children: Vec<BoxedWidget>,  // ← Forces boxing
}
```

### Solution: Make Generic
```rust
pub struct Column<C = Vec<AnyWidget>> {
    pub children: C,
}

// Still works with BoxedWidget for backward compatibility
impl Column<Vec<BoxedWidget>> {
    pub fn new(children: Vec<BoxedWidget>) -> Self {
        Column { children }
    }
}

// New zero-cost API
impl Column<Vec<AnyWidget>> {
    pub fn new_static(children: Vec<AnyWidget>) -> Self {
        Column { children }
    }
}

// Generic implementation
impl<C> RenderObjectWidget for Column<C>
where
    C: IntoIterator,
    C::Item: Widget,
    C: Clone + fmt::Debug + Send + Sync + 'static,
{
    type Arity = MultiArity;
    type Render = RenderFlex;

    fn create_render_object(&self) -> Self::Render {
        RenderFlex::new(Axis::Vertical, MainAxisAlignment::Start, CrossAxisAlignment::Center)
    }
}
```

---

## Phase 3: Create Convenient Macros (Week 1)

```rust
// crates/flui_macros/src/lib.rs

#[macro_export]
macro_rules! column {
    // Zero-allocation version
    ($($child:expr),* $(,)?) => {
        $crate::AnyWidget::Column(
            $crate::Column::new_static(vec![
                $(($child).into()),*
            ])
        )
    };
}

#[macro_export]
macro_rules! row {
    ($($child:expr),* $(,)?) => {
        $crate::AnyWidget::Row(
            $crate::Row::new_static(vec![
                $(($child).into()),*
            ])
        )
    };
}
```

**Usage:**
```rust
// BEFORE: Boxing everywhere
fn build(&self) -> BoxedWidget {
    Box::new(Column::new(vec![
        Box::new(Text::new("Hello")),
        Box::new(Button::new("Click")),
    ]))
}

// AFTER: Zero allocations (except Vec)
fn build(&self) -> AnyWidget {
    column![
        Text::new("Hello"),
        Button::new("Click"),
    ]
}
```

---

## Phase 4: Update StatelessWidget Trait (Week 2)

### Add Associated Type
```rust
pub trait StatelessWidget: fmt::Debug + Clone + Send + Sync + DynWidget + 'static {
    /// The concrete output type (can be AnyWidget or BoxedWidget)
    type Output: Widget = AnyWidget;

    /// Build method now returns concrete type
    fn build(&self) -> Self::Output;

    /// Legacy support - converts to BoxedWidget
    fn build_boxed(&self) -> BoxedWidget {
        Box::new(self.build())
    }
}
```

### Backward Compatibility
```rust
// Old widgets still work
impl StatelessWidget for OldWidget {
    type Output = BoxedWidget;  // ← Opt-in to old behavior

    fn build(&self) -> BoxedWidget {
        Box::new(Text::new("Legacy"))
    }
}

// New widgets get zero-cost
impl StatelessWidget for NewWidget {
    type Output = AnyWidget;  // ← Default

    fn build(&self) -> AnyWidget {
        column![
            Text::new("Fast"),
            Button::new("Click"),
        ]
    }
}
```

---

## Phase 5: Migration Examples

### Before (Current):
```rust
// examples/counter.rs

#[derive(Debug, Clone)]
struct CounterApp {
    count: Signal<i32>,
}

impl StatelessWidget for CounterApp {
    fn build(&self) -> BoxedWidget {
        let count_val = self.count.get();
        Box::new(Column::new(vec![
            Box::new(Text::new(format!("Count: {}", count_val))),
            Box::new(Row::new(vec![
                Box::new({
                    let count = self.count.clone();
                    Button::new("-").on_press(move |_| count.update(|c| *c -= 1))
                }),
                Box::new({
                    let count = self.count.clone();
                    Button::new("+").on_press(move |_| count.update(|c| *c += 1))
                }),
            ])),
        ]))
    }
}
```

### After (Zero-Cost):
```rust
impl StatelessWidget for CounterApp {
    type Output = AnyWidget;  // ← Explicit

    fn build(&self) -> AnyWidget {
        let count_val = self.count.get();
        column![
            Text::new(format!("Count: {}", count_val)),
            row![
                {
                    let count = self.count.clone();
                    Button::new("-").on_press(move |_| count.update(|c| *c -= 1))
                },
                {
                    let count = self.count.clone();
                    Button::new("+").on_press(move |_| count.update(|c| *c += 1))
                },
            ],
        ]
    }
}
```

**Result:**
- **Before:** 7 heap allocations (Column box + 2 children boxes + Row box + 2 button boxes)
- **After:** 2 heap allocations (Vec for column children + Vec for row children)
- **Improvement:** 71% fewer allocations!

---

## Performance Benchmarks (Week 2)

```rust
// benches/widget_allocations.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_boxed_widgets(c: &mut Criterion) {
    c.bench_function("boxed_1000_widgets", |b| {
        b.iter(|| {
            let mut widgets = Vec::new();
            for i in 0..1000 {
                widgets.push(Box::new(Text::new(format!("Item {}", i))) as BoxedWidget);
            }
            black_box(widgets);
        });
    });
}

fn bench_enum_widgets(c: &mut Criterion) {
    c.bench_function("enum_1000_widgets", |b| {
        b.iter(|| {
            let mut widgets = Vec::new();
            for i in 0..1000 {
                widgets.push(AnyWidget::Text(Text::new(format!("Item {}", i))));
            }
            black_box(widgets);
        });
    });
}

criterion_group!(benches, bench_boxed_widgets, bench_enum_widgets);
criterion_main!(benches);
```

**Expected Results:**
```
boxed_1000_widgets    time: [450 µs 455 µs 460 µs]
enum_1000_widgets     time: [15 µs 16 µs 17 µs]

Improvement: ~28x faster
```

---

## Migration Strategy

### Week 1: Foundation
- [ ] Day 1-2: Implement `AnyWidget` and `AnyElement` enums
- [ ] Day 3-4: Make `Column`/`Row` generic over children
- [ ] Day 5: Create `column!`/`row!` macros
- [ ] Day 6-7: Test with examples

### Week 2: API Migration
- [ ] Day 1-2: Add `StatelessWidget::Output` associated type
- [ ] Day 3-4: Update core widgets to use `AnyWidget`
- [ ] Day 5-6: Update examples
- [ ] Day 7: Benchmark and document improvements

### Backward Compatibility
- ✅ Old code keeps working (default to `BoxedWidget`)
- ✅ Gradual migration (widget by widget)
- ✅ No breaking changes to public API

---

## Decision Matrix

| Approach | Allocations | Speed | Complexity | Backward Compat |
|----------|-------------|-------|------------|-----------------|
| **Current (BoxedWidget only)** | 100% | Baseline | Low | N/A |
| **Enum only** | 10% | 10-30x faster | Medium | ❌ Breaking |
| **Hybrid (enum + dynamic)** | 10-20% | 10-20x faster | Medium-High | ✅ Compatible |

**Recommendation:** Hybrid approach (Phase 1-5 plan above)

---

## Risks and Mitigations

### Risk 1: Code Duplication
- **Problem:** Enum requires matching on every operation
- **Mitigation:** Use macros to generate boilerplate
```rust
macro_rules! delegate_widget_method {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            AnyWidget::Text(w) => w.$method($($arg),*),
            AnyWidget::Button(w) => w.$method($($arg),*),
            // ... auto-generated for all variants
        }
    };
}
```

### Risk 2: Enum Growth
- **Problem:** Adding new widget = update enum
- **Mitigation:**
  - Keep `Dynamic(BoxedWidget)` variant for extensibility
  - Only add common widgets to enum (80/20 rule)
  - Third-party widgets use `Dynamic` variant

### Risk 3: Breaking Changes
- **Problem:** Changing `StatelessWidget::build()` signature
- **Mitigation:**
  - Add `type Output` with default = `AnyWidget`
  - Keep `build_boxed()` for legacy code
  - Deprecation period (6 months) before removing

---

## Success Criteria

✅ **Performance:**
- 10-50x fewer allocations in benchmarks
- Layout time <5ms for 1000 widgets
- Memory usage reduced by 30%+

✅ **Developer Experience:**
- Less boilerplate (no manual `Box::new()`)
- Clear migration path
- Examples work with both approaches

✅ **Stability:**
- All existing tests pass
- No breaking changes to public API
- Backward compatibility maintained

---

## Conclusion

**Yes, enum Widget is needed!** It solves the fundamental performance issue while maintaining compatibility with your existing `DynWidget` system.

The hybrid approach gives you:
- ✅ Zero-cost abstractions for static widgets (90% of cases)
- ✅ Flexibility of dynamic dispatch when needed (10% of cases)
- ✅ Smooth migration path without breaking changes
- ✅ Best-in-class performance vs Flutter

This is the **#1 priority for Week 1-2** in the roadmap.
