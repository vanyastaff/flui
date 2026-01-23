# FLUI Architecture Decisions (ADR)

> **Purpose**: Document key architectural decisions for FLUI framework  
> **Based on**: GPUI analysis, Flutter patterns, production requirements  
> **Date**: 2026-01-22

---

## Decision Summary Table

| Decision | Status | Impact | Rationale |
|----------|--------|--------|-----------|
| [ADR-001: Associated Types for Element State](#adr-001) | ✅ Accepted | High | Type safety + zero cost |
| [ADR-002: Three-Phase Element Lifecycle](#adr-002) | ✅ Accepted | High | Better hit testing |
| [ADR-003: Inline Interactivity](#adr-003) | ⚠️ Review | Medium | Locality vs complexity |
| [ADR-004: Pipeline Phase Tracking](#adr-004) | ✅ Accepted | Medium | Runtime safety |
| [ADR-005: Source Location Tracking](#adr-005) | ✅ Accepted | Low | Better debugging |
| [ADR-006: Slab vs SlotMap](#adr-006) | 🔄 Deferred | Low | Start with Slab |
| [ADR-007: RefCell vs RwLock](#adr-007) | 🔄 TBD | Medium | Performance vs threading |
| **V3 Reactive Patterns** | | | |
| [ADR-008: Lens Pattern for Data Access](#adr-008) | 🆕 Proposed | High | Composable state access |
| [ADR-009: Elm Architecture (Messages)](#adr-009) | 🆕 Proposed | High | Unidirectional data flow |
| [ADR-010: Adapt Nodes for Composition](#adr-010) | 🆕 Proposed | Medium | Component reuse |

---

<a name="adr-001"></a>
## ADR-001: Associated Types for Element State

### Status
✅ **Accepted** - Will implement in Phase 5 V2

### Context

Elements need to pass state between lifecycle phases (layout → prepaint → paint). Two approaches:

**Option A: Mutable Fields** (Flutter style)
```rust
pub struct MyElement {
    layout_data: Option<LayoutData>,
    prepaint_data: Option<PrepaintData>,
}

impl Element for MyElement {
    fn request_layout(&mut self) {
        self.layout_data = Some(compute_layout());
    }
    
    fn paint(&self) {
        let layout = self.layout_data.as_ref().unwrap();
        // Use layout
    }
}
```

**Pros**: Simple, familiar
**Cons**: Runtime unwrap, no type safety, memory overhead

**Option B: Associated Types** (GPUI style)
```rust
pub trait Element {
    type LayoutState: 'static;
    type PrepaintState: 'static;
    
    fn request_layout(&mut self) -> Self::LayoutState;
    fn prepaint(&mut self, layout: &mut Self::LayoutState) -> Self::PrepaintState;
    fn paint(&self, layout: &Self::LayoutState, prepaint: &Self::PrepaintState);
}
```

**Pros**: Type safe, zero cost, compiler-enforced
**Cons**: More complex API, harder to learn

### Decision

**Choose Option B: Associated Types**

### Rationale

1. **Type Safety**: Compiler prevents using wrong state in wrong phase
2. **Zero Cost**: No runtime checks, no Option overhead
3. **Better Errors**: Compile-time errors vs runtime panics
4. **GPUI Proven**: Works well in production (Zed editor)
5. **Future Proof**: Easier to add more phases if needed

### Consequences

**Positive**:
- Fewer runtime errors
- Better performance (no Option checks)
- Self-documenting code (types show what's needed)

**Negative**:
- Learning curve for contributors
- More verbose trait definition
- Migration needed from V1

**Mitigation**:
- Provide clear examples
- Document pattern well
- Create helper macros if needed

---

<a name="adr-002"></a>
## ADR-002: Three-Phase Element Lifecycle

### Status
✅ **Accepted** - Will implement in Phase 5 V2

### Context

Element rendering can be 2-phase or 3-phase:

**Option A: Two-Phase** (Flutter)
```rust
fn layout(&mut self) -> Size;
fn paint(&self, canvas: &mut Canvas);
```

**Option B: Three-Phase** (GPUI)
```rust
fn request_layout(&mut self) -> (LayoutId, LayoutState);
fn prepaint(&mut self, layout: &mut LayoutState) -> PrepaintState;
fn paint(&self, layout: &LayoutState, prepaint: &PrepaintState);
```

### Decision

**Choose Option B: Three-Phase**

### Rationale

1. **Better Hit Testing**: Prepaint computes hitboxes separately from layout
2. **Cleaner Separation**: Layout = sizes, Prepaint = bounds, Paint = rendering
3. **GPUI Proven**: Works well for interactive UIs
4. **Optimization Opportunities**: Can skip phases independently

### Consequences

**Positive**:
- More accurate hit testing (hitboxes computed with transforms)
- Better phase isolation
- Easier to optimize (skip prepaint if no interaction)

**Negative**:
- More complex than 2-phase
- Extra phase to maintain

**Mitigation**:
- Clear documentation of each phase
- Helper traits for simple cases
- Can skip prepaint for non-interactive elements

---

<a name="adr-003"></a>
## ADR-003: Inline Interactivity

### Status
⚠️ **Under Review** - Need team input

### Context

Where to store event listeners?

**Option A: Separate EventDispatcher Tree**
```rust
struct EventDispatcher {
    handlers: HashMap<ElementId, Vec<EventHandler>>,
}

// Register separately
dispatcher.register_handler(element_id, handler);
```

**Option B: Inline in Elements** (GPUI)
```rust
struct Element {
    interactivity: Interactivity,  // Listeners stored here
}

impl Element {
    fn on_click(&mut self, handler: impl Fn()) {
        self.interactivity.click_listeners.push(handler);
    }
}
```

### Decision

**Leaning towards Option B, but need team review**

### Rationale

**For Option B (Inline)**:
1. **Locality**: Event handling code near element
2. **Easier Cleanup**: Listeners die with element
3. **Better Performance**: No hash lookup for dispatch
4. **GPUI Proven**: Works in production

**Against Option B**:
1. **More Complex Elements**: Elements become larger
2. **Harder to Share Handlers**: Can't easily share across elements
3. **Learning Curve**: Different from Flutter

### Open Questions

1. How to handle global event handlers?
2. How to coordinate between elements (gesture arena)?
3. What about keyboard focus navigation?

### Next Steps

1. **Prototype both approaches** in Phase 3
2. **Benchmark performance** (hash lookup vs direct call)
3. **Get team feedback** on API ergonomics
4. **Final decision** before Phase 5 implementation

---

<a name="adr-004"></a>
## ADR-004: Pipeline Phase Tracking

### Status
✅ **Accepted** - Will implement in Phase 6 V2

### Context

Should we track which phase the pipeline is in?

**Option A: No Tracking**
```rust
// Any method can be called anytime
render_object.layout(constraints);
render_object.paint(canvas);
```

**Option B: Phase Tracking** (GPUI)
```rust
#[derive(PartialEq)]
enum PipelinePhase {
    Idle, Layout, Compositing, Paint
}

#[track_caller]
fn assert_layout_phase() {
    debug_assert!(phase == PipelinePhase::Layout);
}
```

### Decision

**Choose Option B: Phase Tracking**

### Rationale

1. **Catch Bugs Early**: Debug assertions catch wrong-phase calls
2. **Self-Documenting**: Code shows expected phase
3. **Low Overhead**: Only in debug builds
4. **GPUI Proven**: Caught many bugs in Zed

### Consequences

**Positive**:
- Fewer runtime bugs (caught in debug)
- Better error messages (#[track_caller])
- Clear API contracts

**Negative**:
- Small debug build overhead
- More code to maintain

**Mitigation**:
- Only enable in debug builds
- Clear documentation of phases
- Opt-out for performance-critical code

---

<a name="adr-005"></a>
## ADR-005: Source Location Tracking

### Status
✅ **Accepted** - Will implement in Phase 5 & 6 V2

### Context

Should we track where elements/render objects were created?

**Option A: No Tracking**
```rust
// Error: "Element not found"
```

**Option B: Source Tracking** (GPUI)
```rust
#[cfg(debug_assertions)]
#[track_caller]
pub fn new() -> Self {
    Self {
        source_location: Some(std::panic::Location::caller()),
        // ...
    }
}

// Error: "Element not found (created at src/app.rs:42)"
```

### Decision

**Choose Option B: Source Tracking (debug only)**

### Rationale

1. **Better Debugging**: Error messages show where element created
2. **Inspector Integration**: DevTools can show source locations
3. **Zero Cost in Release**: Only in debug builds
4. **GPUI Proven**: Very helpful in practice

### Consequences

**Positive**:
- Much better error messages
- Faster debugging
- Better DevTools

**Negative**:
- Small debug binary size increase
- Extra field in structs (debug only)

**Mitigation**:
- Only in debug builds
- Use #[cfg(debug_assertions)]
- Optional in release if needed

---

<a name="adr-006"></a>
## ADR-006: Slab vs SlotMap for Storage

### Status
🔄 **Deferred** - Start with Slab, consider later

### Context

How to store elements and render objects?

**Option A: Slab** (Current)
```rust
struct BuildOwner {
    elements: Slab<Box<dyn AnyElement>>,
}

// Manual ID management
let index = slab.insert(element);
let id = ElementId::new(index + 1); // +1 for NonZeroUsize
```

**Pros**: Simple, well-tested, fast
**Cons**: No generation tracking, manual +1/-1

**Option B: SlotMap**
```rust
struct BuildOwner {
    elements: SlotMap<ElementId, Box<dyn AnyElement>>,
}

// Automatic ID management
let id = slotmap.insert(element); // Returns versioned key
```

**Pros**: Generation tracking, detects dangling refs, cleaner API
**Cons**: Slightly slower, more complex

### Decision

**Start with Slab (Option A), migrate to SlotMap later if needed**

### Rationale

1. **Lower Risk**: Slab is proven, less to learn
2. **Performance**: Minimal difference for our use case
3. **Current Code**: Already using Slab
4. **Migration Path**: Can swap later without API changes

### Future Consideration

**Triggers to reconsider**:
- Dangling reference bugs in production
- Need for generation tracking
- After Phase 7 complete (lower risk time)

**Migration plan** (if needed):
1. Create SlotMap wrapper with Slab-like API
2. Benchmark performance
3. Gradual migration (one module at a time)
4. A/B test in production

---

<a name="adr-007"></a>
## ADR-007: RefCell vs RwLock for Interior Mutability

### Status
🔄 **To Be Decided** - Need performance data

### Context

How to implement interior mutability for App/BuildOwner?

**Option A: RefCell** (GPUI)
```rust
struct App {
    inner: RefCell<AppInner>,
}

impl App {
    fn borrow(&self) -> Ref<AppInner> {
        self.inner.borrow() // Runtime check, single-threaded
    }
}
```

**Pros**: Faster (no atomic ops), simpler
**Cons**: Panics on double borrow, single-threaded only

**Option B: RwLock** (Current FLUI)
```rust
struct App {
    inner: Arc<RwLock<AppInner>>,
}

impl App {
    fn read(&self) -> RwLockReadGuard<AppInner> {
        self.inner.read() // Atomic ops, multi-threaded safe
    }
}
```

**Pros**: Thread-safe, no panics (returns error)
**Cons**: Slower (atomic ops), more complex

### Decision

**TBD - Need benchmarks**

### Next Steps

1. **Benchmark both** in Phase 5
2. **Profile real workloads** (not microbenchmarks)
3. **Consider hybrid**: RefCell for hot paths, RwLock for shared

### Factors to Consider

- **UI is single-threaded**: RefCell might be fine
- **Background tasks**: Might need RwLock for some paths
- **Error handling**: Panics vs Results
- **Performance**: Measure, don't guess

---

<a name="adr-008"></a>
## ADR-008: Lens Pattern for Data Access (V3)

### Status
🆕 **Proposed** - For Phase 8 (V3)

### Context

Components need access to subset of parent state. Currently using InheritedView which exposes entire parent state.

**Problem**:
```rust
// Current: Component sees entire AppState
impl View for CounterView {
    fn build(&self, ctx: &BuildContext) -> Element {
        let app_state = ctx.depend_on::<AppState>();  // ❌ Whole state
        let counter = app_state.counter;  // Manual extraction
    }
}
```

**Desired**:
```rust
// Component only sees i32
impl View<State = i32> for CounterView {
    fn build(&self, count: &i32, ctx: &BuildContext) -> Element {
        // ✅ Just i32!
    }
}
```

### Options

**Option A: Continue with InheritedView**
- Manual extraction in each component
- Tight coupling to parent state structure
- Hard to test (needs full AppState)

**Option B: Lens Pattern** (Druid-inspired)
- Type-safe data transformation
- Composable (`lens1.then(lens2)`)
- Components isolated from parent

### Decision

✅ **Choose Option B: Lens Pattern**

**Rationale**:
1. ✅ **Type safety**: Compile-time guarantees
2. ✅ **Composability**: Chain transformations
3. ✅ **Testability**: Components need minimal state
4. ✅ **Reusability**: Same component, different parents
5. ✅ **Zero cost**: No runtime overhead

### Implementation

```rust
// Core trait
pub trait Lens<T, U>: Clone + 'static {
    fn get<'a>(&self, data: &'a T) -> &'a U;
    fn get_mut<'a>(&self, data: &'a mut T) -> &'a mut U;
    
    fn then<V>(self, other: impl Lens<U, V>) -> Then<Self, V>;
}

// Derive macro
#[derive(Lens)]
struct AppState {
    counter: i32,      // Generates AppState::counter lens
    settings: Settings,
}

// LensView widget
LensView::new(
    AppState::counter,  // Lens<AppState, i32>
    CounterView::new()  // View<State = i32>
)
```

### Consequences

**Positive**:
- ✅ Components testable in isolation
- ✅ Type-safe refactoring (compiler catches breaks)
- ✅ Composable data transformations
- ✅ Proven pattern (Druid, Haskell)

**Negative**:
- ⚠️ More boilerplate (derive macro helps)
- ⚠️ Learning curve (new concept)
- ⚠️ Complex error messages (generic types)

**Neutral**:
- Coexists with InheritedView (gradual migration)
- Can use both in same app

### Alternatives Considered

1. **Selector Functions**: Less type-safe than Lens
2. **Context Providers**: React-style, more boilerplate
3. **Global State**: Breaks encapsulation

### References
- [Druid Lens Documentation](https://github.com/linebender/druid)
- Haskell `lens` library
- See `OTHER_FRAMEWORKS_ANALYSIS.md` Section 3

---

<a name="adr-009"></a>
## ADR-009: Elm Architecture (Message-Based Updates) (V3)

### Status
🆕 **Proposed** - For Phase 9 (V3)

### Context

Current state management uses imperative callbacks with side effects. Hard to test, debug, and reason about.

**Problem**:
```rust
// Current: Side effects in closures
Button::new("+")
    .on_click(|| {
        // ❌ How to mutate state here?
        // ❌ Hard to test
        // ❌ Scattered update logic
    })
```

**Desired**:
```rust
// All updates in one place
fn update(&self, state: &mut State, msg: Message) -> UpdateResult {
    match msg {
        Message::Increment => state.value += 1,
        Message::Decrement => state.value -= 1,
    }
    UpdateResult::RequestRebuild
}

// Closures just create messages
Button::new("+")
    .on_press(Message::Increment)  // ✅ Clean!
```

### Options

**Option A: Keep Current (Imperative Callbacks)**
- Familiar to Flutter developers
- More flexible (any code in callback)
- Harder to test/debug

**Option B: Elm Architecture** (Iced-inspired)
- Unidirectional data flow
- Pure update functions (testable)
- Message logging (debuggable)

### Decision

✅ **Choose Option B: Elm Architecture**

**Rationale**:
1. ✅ **Testable**: `update()` is pure function
2. ✅ **Debuggable**: Log/replay messages (time-travel)
3. ✅ **Simple**: One pattern for all state
4. ✅ **Type-safe**: Exhaustive match on messages
5. ✅ **Proven**: Elm, Redux, Iced

### Implementation

```rust
pub trait MessageView: Sized + 'static {
    type State: 'static;
    type Message: Clone + 'static;
    
    fn create_state(&self) -> Self::State;
    
    fn update(
        &self,
        state: &mut Self::State,
        message: Self::Message,
    ) -> UpdateResult;
    
    fn view(
        &self,
        state: &Self::State,
    ) -> impl IntoView<Self::Message>;
}

pub enum UpdateResult {
    None,
    RequestRebuild,
    Command(Box<dyn Future<Output = Message>>),  // Async effects
}

// Message dispatcher in BuildOwner
pub struct MessageDispatcher<M> {
    sender: mpsc::UnboundedSender<M>,
    receiver: mpsc::UnboundedReceiver<M>,
}
```

### Consequences

**Positive**:
- ✅ Easy to test (just call `update()`)
- ✅ Time-travel debugging (record/replay)
- ✅ All state logic in one place
- ✅ Straightforward to understand

**Negative**:
- ⚠️ More boilerplate (Message enum)
- ⚠️ Different from Flutter (learning curve)
- ⚠️ Can't mutate state in closures (intentional)

**Neutral**:
- Coexists with StatefulView (gradual migration)
- Can mix both styles

### Alternatives Considered

1. **Signals/Reactive State**: More implicit, harder to debug
2. **Keep Current**: Familiar but less maintainable
3. **Redux-style Store**: Too heavyweight

### References
- [Elm Architecture Guide](https://guide.elm-lang.org/architecture/)
- [Iced Documentation](https://book.iced.rs/architecture.html)
- See `OTHER_FRAMEWORKS_ANALYSIS.md` Section 2

---

<a name="adr-010"></a>
## ADR-010: Adapt Nodes for Component Composition (V3)

### Status
🆕 **Proposed** - For Phase 10 (V3)

### Context

Components with different state types need to compose. Current approach uses GlobalKey or InheritedView (tight coupling).

**Problem**:
```rust
// Component needs to know parent structure
struct CounterView;

impl View for CounterView {
    fn build(&self, ctx: &BuildContext) -> Element {
        let app = ctx.depend_on::<AppState>();  // ❌ Knows about AppState
        let counter = app.counter;
    }
}
```

**Desired**:
```rust
// Component independent of parent
struct CounterView;

impl MessageView for CounterView {
    type State = i32;  // ✅ Just i32!
    type Message = CounterMessage;
    
    fn view(&self, count: &i32) -> impl View {
        // Component knows nothing about parent
    }
}

// Parent adapts
App::new()
    .child(
        AdaptView::new(
            CounterView::new(),
            |app| &mut app.counter,  // State transform
            |child_msg| AppMessage::Counter(child_msg),  // Message transform
        )
    )
```

### Options

**Option A: Keep Current (GlobalKey/InheritedView)**
- Components know parent structure
- Tight coupling
- Hard to reuse

**Option B: Adapt Nodes** (Xilem-inspired)
- Type-safe transformations
- Components independent
- Highly reusable

### Decision

✅ **Choose Option B: Adapt Nodes**

**Rationale**:
1. ✅ **Decoupling**: Components don't know parents
2. ✅ **Reusability**: Same component, different parents
3. ✅ **Type-safe**: Compile-time checking
4. ✅ **Composable**: Nest adapters
5. ✅ **Zero cost**: Inline in release builds

### Implementation

```rust
pub struct AdaptView<P, C, StateFn, MsgFn> {
    child: C,
    state_transform: StateFn,
    message_transform: MsgFn,
    _phantom: PhantomData<P>,
}

impl<P, C, StateFn, MsgFn> AdaptView<P, C, StateFn, MsgFn>
where
    C: MessageView,
    StateFn: Fn(&mut P) -> &mut C::State + Clone + 'static,
    MsgFn: Fn(C::Message) -> ParentMessage + Clone + 'static,
{
    pub fn new(
        child: C,
        state_transform: StateFn,
        message_transform: MsgFn,
    ) -> Self {
        Self {
            child,
            state_transform,
            message_transform,
            _phantom: PhantomData,
        }
    }
}

// Usage with Lens (Phase 8 + 10)
AdaptView::new(
    CounterView::new(),
    AppState::counter,  // Lens (Phase 8)
    |CounterMessage::Inc| AppMessage::CounterInc,  // Adapt (Phase 10)
)
```

### Consequences

**Positive**:
- ✅ Components truly reusable
- ✅ Type-safe composition
- ✅ Clear data flow
- ✅ Easy to test components

**Negative**:
- ⚠️ More boilerplate (closures)
- ⚠️ Complex type signatures
- ⚠️ Learning curve

**Neutral**:
- Works best with Lens (Phase 8)
- Works best with Messages (Phase 9)
- Complete reactive architecture

### Alternatives Considered

1. **Global Events**: Breaks encapsulation
2. **Dependency Injection**: Too complex
3. **Keep Current**: Works but tight coupling

### References
- [Xilem Architecture](https://raphlinus.github.io/rust/gui/2022/05/07/ui-architecture.html)
- Functional lensing concepts
- See `OTHER_FRAMEWORKS_ANALYSIS.md` Section 1

---

## V3 Roadmap Summary

### Phase 8: Lens Pattern
- **ADR-008**: Lens trait + derive macro + LensView
- **Duration**: 2 weeks
- **Priority**: High ⭐⭐⭐

### Phase 9: Elm Architecture
- **ADR-009**: MessageView trait + dispatcher
- **Duration**: 2 weeks
- **Priority**: High ⭐⭐⭐

### Phase 10: Adapt Nodes
- **ADR-010**: AdaptView for composition
- **Duration**: 1 week
- **Priority**: Medium ⭐⭐

### Combined Architecture

All three patterns work together:

```rust
#[derive(Lens)]  // ADR-008
struct AppState {
    counter: i32,
}

#[derive(Clone)]
enum AppMessage {  // ADR-009
    Increment,
}

impl MessageView for App {  // ADR-009
    fn view(&self, state: &AppState) -> impl View {
        AdaptView::new(  // ADR-010
            CounterView::new(),
            AppState::counter,  // ADR-008: Lens
            |msg| AppMessage::Increment,  // ADR-010: Adapt
        )
    }
}
```

**Result**: Type-safe, composable, testable, debuggable! 🎉

---

## Decision Process

### How We Make Decisions

1. **Research**: Study GPUI, Flutter, other frameworks
2. **Prototype**: Try both approaches if needed
3. **Benchmark**: Measure performance impact
4. **Team Review**: Get input from contributors
5. **Document**: Write ADR (this file)
6. **Implement**: Follow the decision
7. **Revise**: Can change if new info emerges

### When to Revisit

- **New data**: Performance benchmarks, user feedback
- **Phase milestones**: After Phase 5, 7, etc.
- **Production issues**: Bugs or performance problems
- **Team request**: Any contributor can propose revision

---

## Implementation Priority

### V2: Must Have (Phase 5 & 6)

1. ✅ ADR-001: Associated Types
2. ✅ ADR-002: Three-Phase Lifecycle
3. ✅ ADR-004: Pipeline Phase Tracking
4. ✅ ADR-005: Source Location Tracking

### V2: Should Have (Phase 7)

5. ⚠️ ADR-003: Inline Interactivity (decide by Phase 5)
6. 🔄 ADR-007: RefCell vs RwLock (benchmark in Phase 5)

### V2: Nice to Have (Future)

7. 🔄 ADR-006: SlotMap migration (after Phase 7)

### V3: High Priority (Phase 8-10)

8. 🆕 ADR-008: Lens Pattern (Phase 8, 2 weeks) ⭐⭐⭐
9. 🆕 ADR-009: Elm Architecture (Phase 9, 2 weeks) ⭐⭐⭐
10. 🆕 ADR-010: Adapt Nodes (Phase 10, 1 week) ⭐⭐

### V3: Nice to Have (Post-1.0)

- Command System (async effects)
- Subscription System (long-running listeners)
- Time-Travel Debugging
- State Inspector DevTools

---

## Summary for Implementation

### V2: Phase 5 Changes

- ✅ Add associated types to Element trait
- ✅ Implement three-phase lifecycle
- ✅ Add source location tracking (#[track_caller])
- ✅ Add draw phase tracking to BuildOwner
- ⚠️ **Decision needed**: Inline interactivity vs EventDispatcher

### V2: Phase 6 Changes

- ✅ Add pipeline phase tracking to PipelineOwner
- ✅ Add phase assertions (#[track_caller])
- ✅ Add source location to RenderObjects
- ✅ Implement Hitbox system (Bounds + ContentMask)
- 🔄 **Keep**: Slab (defer SlotMap to later)

### V3: Phase 8 Changes (Lens Pattern)

- 🆕 Add Lens trait to flui-view
- 🆕 Create #[derive(Lens)] proc macro
- 🆕 Implement LensView widget
- 🆕 Add lens composition (then, map)
- 🆕 Documentation + examples

### V3: Phase 9 Changes (Elm Architecture)

- 🆕 Add MessageView trait to flui-view
- 🆕 Implement MessageDispatcher in BuildOwner
- 🆕 Add UpdateResult enum (None, Rebuild, Command)
- 🆕 Integrate with Element tree
- 🆕 TodoMVC example + migration guide

### V3: Phase 10 Changes (Adapt Nodes)

- 🆕 Add AdaptView widget
- 🆕 Implement state/message transformation
- 🆕 Integration with Lens (Phase 8)
- 🆕 Integration with Messages (Phase 9)
- 🆕 Complete reactive example

### Benchmarks Needed

1. RefCell vs RwLock (Phase 5/V2)
2. Inline interactivity vs EventDispatcher (Phase 3/5)
3. Slab vs SlotMap (Phase 7+)
4. Lens overhead vs direct access (Phase 8/V3)
5. Message dispatch overhead (Phase 9/V3)

---

**Статус**: 📋 Living Document  
**Последнее обновление**: 2026-01-22  
**Автор**: Claude with team input  
**Рецензенты**: TBD

---

## Appendix: Comparison Tables

### Element State Management

| Approach | Type Safety | Performance | Complexity | Verdict |
|----------|-------------|-------------|------------|---------|
| Mutable Fields | ❌ Runtime | ⚠️ Option overhead | ✅ Simple | ❌ Not chosen |
| Associated Types | ✅ Compile-time | ✅ Zero cost | ⚠️ Complex | ✅ **Chosen** |

### Lifecycle Phases

| Approach | Hit Testing | Complexity | Optimization | Verdict |
|----------|-------------|------------|--------------|---------|
| 2-Phase | ⚠️ During layout | ✅ Simple | ❌ Limited | ❌ Not chosen |
| 3-Phase | ✅ Dedicated | ⚠️ Complex | ✅ Flexible | ✅ **Chosen** |

### Event Listeners

| Approach | Locality | Cleanup | Performance | Verdict |
|----------|----------|---------|-------------|---------|
| Separate Tree | ❌ Scattered | ⚠️ Manual | ⚠️ Hash lookup | ⚠️ Under review |
| Inline | ✅ Co-located | ✅ Automatic | ✅ Direct | ⚠️ **Leaning towards** |

### Storage

| Approach | Safety | Performance | API | Verdict |
|----------|--------|-------------|-----|---------|
| Slab | ⚠️ No generations | ✅ Fast | ⚠️ Manual IDs | ✅ **Phase 5-7** |
| SlotMap | ✅ Generations | ✅ Fast enough | ✅ Clean | 🔄 **Phase 8+** |
