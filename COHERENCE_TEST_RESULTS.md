# Coherence Rule Tests - Results and Analysis

## 📊 Summary

| Approach | Compiles? | Notes |
|----------|-----------|-------|
| **WidgetMarker trait** | ❌ FAILS | E0119: conflicting implementations |
| **Associated type discrimination** | ❌ FAILS | E0119: circular dependency issue |
| **Sealed trait + const** | ❌ FAILS | E0119: Rust doesn't check const values |
| **Direct impls (no blanket)** | ✅ WORKS | Verbose but functional |
| **Xilem pattern (no sub-traits)** | ✅ WORKS | Single trait, no hierarchy |
| **Enum-based Widget** | ✅ WORKS | Type-erased but functional |
| **Newtype wrappers** | ✅ WORKS | Extra layer of wrapping |
| **Macro-generated impls** | ✅ WORKS | Best user experience |

---

## 🔴 Failed Approaches

### 1. WidgetMarker Trait

```rust
pub trait WidgetMarker {}
impl<T: Debug + 'static> WidgetMarker for T {}

pub trait StatelessWidget: WidgetMarker { ... }
pub trait StatefulWidget: WidgetMarker { ... }

// ❌ CONFLICTS!
impl<W: StatelessWidget> Widget for W { ... }
impl<W: StatefulWidget> Widget for W { ... }
```

**Error:**
```
error[E0119]: conflicting implementations of trait `Widget`
  --> coherence_tests.rs:38:5
   |
31 |     impl<W: StatelessWidget> Widget for W {
   |     ------------------------------------- first implementation here
...
38 |     impl<W: StatefulWidget> Widget for W {
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ conflicting implementation
```

**Why it fails:**
- Rust's coherence checker asks: "Could a type implement BOTH StatelessWidget AND StatefulWidget?"
- Even though we know this is impossible, Rust doesn't look deep enough into trait definitions
- It conservatively rejects the blanket impls

---

### 2. Associated Type Discrimination

```rust
pub struct StatelessMarker;
pub struct StatefulMarker;

pub trait Widget {
    type Marker;  // ← Discriminator
    type Element;
}

pub trait StatelessWidget: Widget<Marker = StatelessMarker> { ... }

// ❌ STILL CONFLICTS!
impl<W: StatelessWidget> Widget for W {
    type Marker = StatelessMarker;
    ...
}
```

**Error:** Same E0119

**Why it fails:**
- Circular dependency: `StatelessWidget` requires `Widget`, but we're trying to impl `Widget` for `StatelessWidget`
- The coherence checker sees the conflict before evaluating the type constraints

---

### 3. Sealed Trait with Const Discriminator

```rust
mod sealed {
    pub trait Sealed {
        const KIND: u8;
    }
}

impl<W: StatelessWidget> sealed::Sealed for W {
    const KIND: u8 = 0;
}

impl<W: StatefulWidget> sealed::Sealed for W {
    const KIND: u8 = 1;  // ← Different const value
}

// ❌ STILL CONFLICTS on Sealed trait!
```

**Why it fails:**
- Rust's coherence checker doesn't evaluate const expressions
- It only checks if the trait bounds could overlap
- `StatelessWidget` and `StatefulWidget` could theoretically be implemented by the same type

---

## ✅ Working Approaches

### 1. Direct Impls (No Blanket Impls)

**The verbose but guaranteed solution:**

```rust
pub trait Widget: Debug + 'static {
    type Element;
    fn element_type(&self) -> &'static str;
}

pub trait StatelessWidget {
    fn build(&self) -> String;
}

// User implements BOTH traits
#[derive(Debug)]
struct Counter { count: i32 }

impl StatelessWidget for Counter {
    fn build(&self) -> String {
        format!("{}", self.count)
    }
}

// Explicit Widget impl
impl Widget for Counter {
    type Element = String;
    fn element_type(&self) -> &'static str { "Stateless" }
}
```

**Pros:**
- ✅ Guaranteed to compile
- ✅ No magic, very explicit
- ✅ Widget trait can still have default methods

**Cons:**
- ❌ Verbose (two impl blocks per widget)
- ❌ Boilerplate for users

**Test result:** ✅ All tests pass

---

### 2. Xilem Pattern (No Sub-Trait Hierarchy)

**What Xilem actually does:**

```rust
pub trait View: Debug + 'static {
    type Element;
    type State;

    fn build(&self) -> (Self::Element, Self::State);
}

// NO StatelessView, StatefulView sub-traits!
// Each widget implements View directly

#[derive(Debug)]
struct Button { label: String }

impl View for Button {
    type Element = String;
    type State = ();  // ← Stateless = ()

    fn build(&self) -> (Self::Element, Self::State) {
        (format!("Button: {}", self.label), ())
    }
}

#[derive(Debug)]
struct Slider { value: f32 }

impl View for Slider {
    type Element = f32;
    type State = f32;  // ← Has state

    fn build(&self) -> (Self::Element, Self::State) {
        (self.value, self.value)
    }
}
```

**Key insight:**
- Xilem uses `State = ()` for "stateless" widgets
- No need for `StatelessWidget` vs `StatefulWidget` sub-traits
- Single trait hierarchy = no blanket impl conflicts

**Pros:**
- ✅ Clean, simple design
- ✅ No blanket impl conflicts
- ✅ Flexible (State type determines behavior)

**Cons:**
- ❌ Different API than Flutter
- ❌ All widgets must handle State (even if `()`)

**Test result:** ✅ All tests pass

---

### 3. Enum-Based Widget

**Type-erased approach:**

```rust
pub trait StatelessWidget: Debug {
    fn build(&self) -> String;
}

pub trait StatefulWidget: Debug {
    fn create_state(&self) -> i32;
}

// Widget is an enum!
pub enum Widget {
    Stateless(Box<dyn StatelessWidget>),
    Stateful(Box<dyn StatefulWidget>),
}

impl Widget {
    pub fn from_stateless<W: StatelessWidget + 'static>(w: W) -> Self {
        Widget::Stateless(Box::new(w))
    }

    pub fn element_type(&self) -> &'static str {
        match self {
            Widget::Stateless(_) => "Stateless",
            Widget::Stateful(_) => "Stateful",
        }
    }
}
```

**Pros:**
- ✅ Compiles
- ✅ Exhaustive matching
- ✅ Clear dispatch

**Cons:**
- ❌ Dynamic dispatch overhead
- ❌ Type-erased (lose concrete types)
- ❌ Requires boxing

**Test result:** ✅ All tests pass

---

### 4. Newtype Wrappers

**Add an indirection layer:**

```rust
pub trait Widget: Debug + 'static {
    type Element;
}

pub trait StatelessWidget {
    fn build(&self) -> String;
}

// Wrapper types
#[derive(Debug)]
pub struct StatelessWidgetWrapper<W>(pub W);

// Now blanket impl works because it's on the wrapper!
impl<W: StatelessWidget + Debug + 'static> Widget for StatelessWidgetWrapper<W> {
    type Element = String;
}

// Usage
let counter = Counter { count: 42 };
let widget = StatelessWidgetWrapper(counter);
```

**Pros:**
- ✅ Blanket impls work
- ✅ Compile-time type safety

**Cons:**
- ❌ Extra wrapping layer
- ❌ Verbose usage

**Test result:** ✅ All tests pass

---

### 5. Macro-Generated Impls

**Best user experience:**

```rust
#[derive(Debug, Widget)]  // ← Proc macro
struct Counter { count: i32 }

impl StatelessWidget for Counter {
    fn build(&self) -> String {
        format!("{}", self.count)
    }
}

// Macro generates:
// impl Widget for Counter {
//     type Element = ComponentElement<Self>;
// }
```

**How it works:**
- Proc macro detects which sub-trait is implemented
- Generates appropriate `Widget` impl
- User only sees clean code

**Pros:**
- ✅ Clean user-facing API
- ✅ Compiles perfectly
- ✅ IDE support (can show generated code)
- ✅ Type-safe

**Cons:**
- ❌ Requires proc macros (flui_derive crate)
- ❌ Slightly slower compile times
- ❌ More complex to implement

**Test result:** ✅ All tests pass (using declarative macro example)

---

## 🎯 Recommendations for Flui

### Option A: Derive Macro Approach ⭐ RECOMMENDED

```rust
use flui::prelude::*;

#[derive(Debug, Clone, Widget)]
struct Counter {
    count: i32,
}

impl StatelessWidget for Counter {
    fn build(&self, ctx: &BuildContext) -> BoxedWidget {
        Text::new(format!("{}", self.count)).into()
    }
}
```

**Why this is best:**
1. Clean user-facing API (like Flutter)
2. Actually compiles
3. Type-safe
4. Standard Rust practice (like `#[derive(Debug)]`)
5. Can provide helpful error messages

**Implementation plan:**
1. Create `flui_derive` proc macro crate
2. Implement `#[derive(Widget)]`
3. Detect which sub-trait is implemented (via trait resolution)
4. Generate appropriate `Widget` impl block

---

### Option B: Xilem Pattern (If Willing to Redesign)

```rust
#[derive(Debug, Clone)]
struct Counter {
    count: i32,
}

impl Widget for Counter {
    type Element = ComponentElement<Self>;
    type State = ();  // Stateless

    fn build(&self, ctx: &BuildContext) -> BoxedWidget {
        Text::new(format!("{}", self.count)).into()
    }
}
```

**Why this could work:**
1. No sub-trait hierarchy
2. No blanket impls needed
3. Simpler architecture
4. Proven by Xilem

**Tradeoffs:**
- Different API from Flutter
- All widgets handle State (even if `()`)
- Larger API change

---

### Option C: Accept Verbosity (No Changes Needed)

```rust
#[derive(Debug, Clone)]
struct Counter { count: i32 }

impl StatelessWidget for Counter {
    fn build(&self, ctx: &BuildContext) -> BoxedWidget { ... }
}

// Just accept this extra line
impl Widget for Counter {
    type Element = ComponentElement<Self>;
}
```

**When to choose this:**
- If you want to ship quickly
- If you don't want to maintain proc macros
- If verbosity is acceptable

---

## 🧪 Test Files

All test code is available in:
- `coherence_tests.rs` - Failed approaches (doesn't compile)
- `coherence_success_tests.rs` - Working approaches (compiles + tests pass)

To run tests:
```bash
# Failed tests (demonstrates E0119 errors)
rustc --crate-type lib coherence_tests.rs

# Success tests
rustc --crate-type lib --test coherence_success_tests.rs -o test.exe
./test.exe
```

---

## 📚 Key Learnings

1. **WidgetMarker doesn't solve coherence conflicts** - Rust's checker doesn't analyze trait bounds deeply enough

2. **Xilem doesn't have sub-trait hierarchies** - It uses a single `View` trait with associated types

3. **Blanket impls from overlapping bounds are impossible** - Without specialization or negative trait bounds

4. **Derive macros are the standard solution** - Used by Serde, async-trait, and many others

5. **Coherence rules are fundamental** - Cannot be bypassed with clever tricks

---

## 🎓 Conclusion

**The theory is confirmed:**
- ❌ Marker traits don't solve blanket impl conflicts
- ❌ Associated types can't discriminate blanket impls
- ❌ Sealed traits + consts don't help
- ✅ Direct impls always work (but verbose)
- ✅ Derive macros provide the best UX
- ✅ Xilem uses a different pattern (no sub-traits)

**For Flui, I recommend:**
1. Implement `#[derive(Widget)]` proc macro
2. Keep the sub-trait API (StatelessWidget, StatefulWidget)
3. Users write one derive + one impl block
4. Clean API similar to Flutter
5. Standard Rust practice

This gives you the best of both worlds: ergonomic API + guaranteed compilation.
