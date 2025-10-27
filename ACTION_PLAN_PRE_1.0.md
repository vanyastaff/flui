# FLUI Pre-1.0 Action Plan

## 🎯 Mission Critical Changes

**Window: NOW - before 1.0 release**
**Impact: Breaking changes are OK now, painful later**

---

## ⚠️ PRIORITY 1: Performance Foundation (Week 1-2)

### Issue: BoxedWidget Everywhere
**Problem:** Every widget is heap-allocated via `Box<dyn Widget>`
- ❌ 10-100 allocations per frame
- ❌ Poor cache locality
- ❌ Unnecessary overhead

**Solution:** `impl Widget` + Enum for Dynamic Cases

```rust
// BEFORE (current)
pub trait StatelessWidget {
    fn build(&self) -> BoxedWidget;  // ← Every widget boxed!
}

pub fn column(children: Vec<BoxedWidget>) -> impl Widget {
    Column { children }
}

// AFTER (proposed)
pub trait StatelessWidget {
    type Output: Widget;  // ← Concrete type when possible
    fn build(&self) -> Self::Output;
}

// Static case - zero allocations
pub fn text(s: impl Into<String>) -> Text {
    Text { content: s.into() }
}

// Dynamic case - enum
pub enum AnyWidget {
    Text(Text),
    Button(Button),
    Column(Column<Vec<AnyWidget>>),
    Custom(Box<dyn Widget>),  // ← Box only truly dynamic
}

pub fn column<I>(children: I) -> Column<I>
where
    I: IntoIterator,
    I::Item: Widget,
{
    Column { children }
}
```

**Tasks:**
- [ ] Define `AnyWidget` enum with common widgets
- [ ] Change `StatelessWidget::build()` to return `impl Widget`
- [ ] Update `column!`, `row!` macros to work with iterators
- [ ] Benchmark: measure allocation reduction
- [ ] Update all examples
- [ ] Update documentation

**Expected Impact:**
- ✅ 10-50x fewer allocations
- ✅ Better cache locality
- ✅ Faster build times
- ⚠️ Breaking change - must do now!

---

## 🔧 PRIORITY 2: Signal Ergonomics (Week 2-3)

### Issue: Manual Clone Required

```rust
// CURRENT - annoying
button("+").on_press({
    let count = self.count.clone();  // ← Manual!
    move |_| count.update(|c| *c += 1)
})
```

**Solution:** Extension Methods + Macros

```rust
// PROPOSED - clean!
button("+").on_press_signal_inc(&self.count)

// Or with clone! macro
button("+").on_press(clone!(self.count => move |_| {
    count.increment()
}))
```

**Tasks:**
- [ ] Implement extension trait `SignalExt` with:
  - `increment()`, `decrement()`, `toggle()` for common types
- [ ] Implement extension traits for widgets:
  - `ButtonSignalExt::on_press_signal_*`
  - `TextFieldSignalExt::on_change_signal`
- [ ] Implement `clone!` macro
- [ ] Add comprehensive examples
- [ ] Update Chapter 11 documentation

**Expected Impact:**
- ✅ 50% less boilerplate
- ✅ Better developer experience
- ✅ Easier React/Vue migration

---

## 🏗️ PRIORITY 3: Effect System API (Week 3-4)

### Issue: Unclear Lifecycle

```rust
// CURRENT - when does this run? how to cleanup?
cx.use_effect(|| {
    // ???
});
```

**Solution:** Explicit Dependencies + Cleanup

```rust
// PROPOSED - clear!
pub trait EffectContext {
    // Run when deps change
    fn use_effect<D, F>(&self, deps: D, f: F)
    where
        D: PartialEq + 'static,
        F: Fn() -> Box<dyn FnOnce()>;  // Returns cleanup

    // Run once on mount
    fn use_effect_once<F>(&self, f: F)
    where
        F: FnOnce() -> Box<dyn FnOnce()>;

    // Run on every render (rare)
    fn use_effect_always<F>(&self, f: F)
    where
        F: Fn() -> Box<dyn FnOnce()>;
}

// Usage:
cx.use_effect(
    count.get(),  // ← Dependency
    || {
        println!("Count changed!");
        Box::new(|| println!("Cleanup!"))
    }
);
```

**Tasks:**
- [ ] Define `EffectContext` trait
- [ ] Implement dependency tracking
- [ ] Implement cleanup guarantees (RAII)
- [ ] Add examples for common patterns:
  - Timers
  - Event listeners
  - Subscriptions
  - Async tasks
- [ ] Update documentation

**Expected Impact:**
- ✅ Clear lifecycle semantics
- ✅ Guaranteed cleanup
- ✅ Easier to reason about

---

## 🎨 PRIORITY 4: Context System (Week 4-5)

### Issue: No Built-in Dependency Injection Pattern

**Solution:** Provider/Consumer Pattern (like React Context)

```rust
// Provide value down the tree
pub fn app() -> Widget {
    Provider::new(Theme::dark())
        .child(Provider::new(User::current())
            .child(my_app()))
}

// Consume anywhere in subtree
pub fn themed_button(cx: &BuildContext) -> Widget {
    let theme = cx.use_context::<Theme>()?;
    let user = cx.use_context::<User>()?;

    button("Click")
        .color(theme.primary_color)
        .tooltip(format!("Hello, {}", user.name))
}
```

**Tasks:**
- [ ] Implement `Provider<T>` widget
- [ ] Add `BuildContext::use_context<T>()` method
- [ ] Add `BuildContext::provide<T>()` internal API
- [ ] Create examples:
  - Theme system
  - i18n/localization
  - User session
  - Feature flags
- [ ] Update documentation

**Expected Impact:**
- ✅ Clean dependency injection
- ✅ No prop drilling
- ✅ Familiar to React developers

---

## 📚 PRIORITY 5: API Consistency Review (Week 5-6)

### Issue: Inconsistent Naming

```rust
// CURRENT - inconsistent
container().child(widget)       // ← .child (singular)
column().children(vec![...])    // ← .children (plural)
button().on_press(|_| {})       // ← on_press
text_field().on_change(|_| {})  // ← on_change (different naming)
```

**Solution:** Standardize Naming Conventions

```rust
// PROPOSED - consistent

// Rule 1: Single child → .child()
container().child(widget)
opacity().child(widget)

// Rule 2: Multiple children → .children()
column().children([...])
row().children([...])

// Rule 3: Events → .on_<event>()
button().on_press(|_| {})
button().on_hover(|_| {})
text_field().on_change(|text| {})
text_field().on_submit(|text| {})

// Rule 4: Properties → descriptive names
container().width(100.0)        // Not .w()
container().padding(16.0)       // Not .p()
container().background(Color)   // Not .bg()

// Rule 5: Conversions → impl Into<T>
container().padding(16.0)                    // f32 → EdgeInsets::all
container().padding(EdgeInsets::all(16.0))   // Explicit
container().width(Length::px(100.0))         // Length enum
container().width(100.0)                     // f32 → Length::px
```

**Tasks:**
- [ ] Audit all widget APIs
- [ ] Create naming convention document
- [ ] Refactor inconsistent APIs
- [ ] Update all examples
- [ ] Update documentation
- [ ] Create migration guide

**Expected Impact:**
- ✅ Easier to learn
- ✅ Better autocomplete
- ✅ Fewer surprises

---

## 🚀 PRIORITY 6: Core Widget Library (Week 6-8)

### Essential Widgets for 1.0

**Layout:**
- [x] Container
- [x] Row, Column
- [ ] Stack (z-index layering)
- [ ] Flex (flexible sizing)
- [ ] Padding, Margin
- [ ] SizedBox
- [ ] Spacer

**Basic:**
- [x] Text
- [ ] Image
- [ ] Icon
- [x] Button
- [ ] IconButton
- [ ] TextButton

**Input:**
- [ ] TextField
- [ ] Checkbox
- [ ] Radio
- [ ] Switch
- [ ] Slider

**Scrolling:**
- [ ] ScrollView
- [ ] ListView
- [ ] GridView

**Advanced:**
- [ ] Opacity
- [ ] Transform (rotate, scale, translate)
- [ ] ClipRect, ClipRRect
- [ ] GestureDetector

**Tasks:**
- [ ] Implement missing widgets
- [ ] Write comprehensive tests
- [ ] Create examples for each
- [ ] Benchmark performance
- [ ] Document best practices

---

## 📊 Testing & Benchmarking (Ongoing)

### Performance Benchmarks

```rust
// Critical benchmarks for 1.0

#[bench]
fn layout_1000_widgets(b: &mut Bencher) {
    // Target: <5ms (vs Flutter ~15ms)
}

#[bench]
fn rebuild_fine_grained(b: &mut Bencher) {
    // Only changed widget rebuilds
}

#[bench]
fn signal_updates(b: &mut Bencher) {
    // Signal set + dependent rebuilds
}

#[bench]
fn memory_allocations(b: &mut Bencher) {
    // After BoxedWidget → impl Widget change
    // Target: 10x fewer allocations
}
```

**Tasks:**
- [ ] Set up criterion benchmarks
- [ ] Establish baseline metrics
- [ ] Track performance over time
- [ ] Compare with Flutter
- [ ] Publish results

### Testing Coverage

- [ ] Unit tests for all widgets
- [ ] Integration tests for framework
- [ ] Property-based tests (proptest)
- [ ] Fuzz testing for layout
- [ ] Visual regression tests

---

## 📖 Documentation (Week 8-10)

### Essential Documentation for 1.0

- [x] Architecture overview
- [x] Widget system guide
- [x] Reactive system guide
- [x] Why FLUI (10x thesis)
- [x] Lessons from frameworks
- [ ] Getting started tutorial
- [ ] API reference (cargo doc)
- [ ] Migration from Flutter
- [ ] Performance guide
- [ ] Best practices
- [ ] FAQ

**Tasks:**
- [ ] Write getting started guide
- [ ] Create video tutorials (optional)
- [ ] Set up docs website
- [ ] Add inline documentation
- [ ] Create example gallery

---

## 🎯 Release Criteria for 1.0

### Must Have (Blockers)

- [ ] ✅ Zero `BoxedWidget` in hot paths (impl Widget)
- [ ] ✅ Signal ergonomics finalized
- [ ] ✅ Effect system API stable
- [ ] ✅ Context system working
- [ ] ✅ API naming consistent
- [ ] ✅ Core widgets complete
- [ ] ✅ Performance benchmarks passing
- [ ] ✅ Test coverage >80%
- [ ] ✅ Documentation complete
- [ ] ✅ Examples working

### Nice to Have (Post-1.0)

- [ ] Hot reload
- [ ] DevTools integration
- [ ] Advanced animations
- [ ] Gesture recognizers
- [ ] Platform channels
- [ ] WASM support

---

## 📅 Timeline

| Week | Focus | Deliverable |
|------|-------|-------------|
| 1-2 | BoxedWidget → impl Widget | Performance boost |
| 2-3 | Signal ergonomics | Clean API |
| 3-4 | Effect system | Stable lifecycle |
| 4-5 | Context system | DI pattern |
| 5-6 | API consistency | Unified naming |
| 6-8 | Core widgets | Complete library |
| 8-10 | Documentation | User guides |
| 10 | Testing & Polish | Release candidate |
| 11 | Release 1.0! | 🎉 |

**Total: ~3 months to 1.0**

---

## 🚨 Breaking Changes Policy

### Before 1.0 (Current Phase)
- ✅ **Breaking changes OK**
- ✅ **No migration pain for users (no users yet!)**
- ✅ **Time to get architecture right**

### After 1.0
- ⚠️ **Semantic versioning strict**
- ⚠️ **Breaking changes only in major versions**
- ⚠️ **Migration guides required**
- ⚠️ **Deprecation period (6+ months)**

**Conclusion: Make breaking changes NOW!**

---

## 🎓 Learning from Others

### React's Mistake
- ❌ Class components → Hooks migration was painful
- ✅ We learn: Get API right before 1.0

### Vue's Success
- ✅ Options → Composition both supported
- ✅ We learn: Support multiple styles

### Svelte's Win
- ✅ Compiler-first from day one
- ✅ We learn: Use Rust's compile-time power

### Flutter's Pain
- ❌ Null safety migration took years
- ✅ We learn: Safety from day one (Rust gives us this!)

---

## 💡 Key Principles

1. **Performance First**
   - Every abstraction must be zero-cost
   - Benchmark everything
   - Compare with Flutter

2. **Developer Experience**
   - Clean APIs
   - Great error messages
   - Excellent documentation

3. **Type Safety**
   - Leverage Rust's type system
   - Compile-time guarantees
   - No runtime surprises

4. **Composability**
   - Small, focused components
   - Easy to combine
   - Reusable patterns

5. **Migration Path**
   - Easy from Flutter
   - Easy from React
   - Clear documentation

---

## 🤝 Community Involvement

### Before 1.0 Beta
- [ ] Internal team review
- [ ] Architecture validation
- [ ] Performance testing

### 1.0 Beta Release
- [ ] Public announcement
- [ ] Gather feedback
- [ ] Fix critical issues
- [ ] Iterate on API

### 1.0 Release
- [ ] Stable API
- [ ] Production ready
- [ ] Marketing push
- [ ] Community building

---

## 📈 Success Metrics

### Technical Metrics
- Layout time: **<5ms** for 1000 widgets
- Memory: **<50MB** for medium app
- Build time: **<5s** incremental
- Test coverage: **>80%**

### Adoption Metrics
- GitHub stars: **1000+** (year 1)
- Production apps: **10+** (year 1)
- Contributors: **50+** (year 1)

### Community Metrics
- Discord members: **500+**
- Documentation views: **10k+/month**
- Tutorial completions: **1000+**

---

## 🎉 Conclusion

**We have a unique window NOW - before 1.0 - to get FLUI's architecture right.**

The frameworks we analyzed spent years fixing architectural mistakes. We can **learn from their pain** and **build it right the first time**.

Let's make FLUI the UI framework Rust deserves! 🚀

---

**Next Steps:**
1. Review this plan with team
2. Prioritize tasks
3. Start with Priority 1 (BoxedWidget)
4. Iterate fast
5. Ship 1.0 in ~3 months

**Questions? Concerns?** Let's discuss in Discord or GitHub Discussions.
