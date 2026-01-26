# Feature Specification: flui-scheduler - Task Scheduling and Prioritization System

**Feature Branch**: `001-flui-scheduler`  
**Created**: 2026-01-26  
**Status**: Draft  
**Input**: User description: "flui-scheduler crate for task scheduling and prioritization system ensuring UI responsiveness through intelligent scheduling of work"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Immediate User Input Response (Priority: P1)

Users interacting with the UI (clicking, typing, touching) receive instant visual feedback without any perceptible delay, even when background work is running.

**Why this priority**: This is the most critical requirement. Input lag directly impacts perceived app quality and user satisfaction. Without this, the app feels "broken" or "slow" regardless of other features.

**Independent Test**: Can be fully tested by measuring input-to-response latency while background tasks run. Delivers immediate value: responsive UI that feels instant to users.

**Acceptance Scenarios**:

1. **Given** the app is idle, **When** user taps a button, **Then** visual feedback appears within 100ms
2. **Given** background image loading is in progress, **When** user types in a text field, **Then** each character appears within 50ms with no dropped keystrokes
3. **Given** multiple low-priority tasks are queued, **When** user initiates touch/click event, **Then** event handler executes immediately, pausing lower-priority work
4. **Given** a long-running calculation is executing, **When** user interacts with UI, **Then** interaction is processed within 100ms (calculation yields)

---

### User Story 2 - Smooth 60fps Animations (Priority: P1)

Users viewing animations or scrolling content see smooth, consistent motion at 60fps without stuttering or frame drops during normal operation.

**Why this priority**: Smooth animations are the second most visible quality indicator. Janky scrolling or stuttering animations make the app feel unpolished and unprofessional, directly impacting user trust.

**Independent Test**: Can be tested by measuring frame timing during animations with various background loads. Delivers value: polished, professional-feeling interface.

**Acceptance Scenarios**:

1. **Given** an animation is playing, **When** measuring frame intervals, **Then** 95% of frames are rendered within 16.67ms (60fps)
2. **Given** user is scrolling a list, **When** images are loading in background, **Then** scroll animation maintains 60fps throughout
3. **Given** multiple animations are active, **When** layout calculations are needed, **Then** all work completes within single frame budget (16.67ms)
4. **Given** display refresh rate is 120Hz, **When** system detects capabilities, **Then** scheduler adapts to 8.33ms frame budget automatically

---

### User Story 3 - Non-Blocking Background Work (Priority: P2)

Developers can schedule background tasks (data loading, image processing, network requests) that automatically yield to UI work, keeping the interface responsive.

**Why this priority**: Essential for real-world apps that load data. Without this, developers would need to manually implement yielding logic, which is error-prone and often skipped, leading to poor user experience.

**Independent Test**: Can be tested by running long background tasks and measuring UI responsiveness. Delivers value: apps can load data without blocking users.

**Acceptance Scenarios**:

1. **Given** developer schedules 1000 image decode operations, **When** tasks execute, **Then** each task yields after 16ms maximum, allowing UI work to interrupt
2. **Given** background task is running, **When** user input arrives, **Then** background task pauses within 16ms and input is processed
3. **Given** multiple background tasks are queued, **When** developer cancels the operation, **Then** all pending tasks are immediately removed from queue
4. **Given** background task reports progress, **When** task is paused for UI work, **Then** progress tracking remains accurate across pause/resume cycles

---

### User Story 4 - Frame-Synchronized Rendering (Priority: P1)

The rendering system schedules layout, paint, and commit operations in sync with display refresh to avoid wasted work and visual tearing.

**Why this priority**: Core to efficient rendering. Without proper frame synchronization, the system wastes CPU (redundant calculations) or drops frames (missed deadlines), both degrading user experience.

**Independent Test**: Can be tested by instrumenting frame phases and measuring timing. Delivers value: efficient rendering with no wasted cycles.

**Acceptance Scenarios**:

1. **Given** display sends vsync signal, **When** scheduler receives it, **Then** frame phases execute in order: BeginFrame → Update → Layout → Paint → Commit
2. **Given** layout calculations are needed, **When** frame begins, **Then** layout runs exactly once per frame (not zero, not multiple times)
3. **Given** frame work exceeds budget, **When** deadline approaches, **Then** scheduler detects missed frame, logs it, and skips to next frame
4. **Given** no UI updates are needed, **When** vsync arrives, **Then** scheduler skips unnecessary work and sleeps until next event

---

### User Story 5 - Idle-Time Optimization (Priority: P2)

The framework can schedule cleanup, garbage collection, and preparation work during idle periods without interfering with user interaction or preventing the app from sleeping.

**Why this priority**: Important for efficiency and battery life, but not critical for MVP. Can be added after core scheduling works.

**Independent Test**: Can be tested by scheduling idle work and measuring when it runs vs. when UI work arrives. Delivers value: efficient resource usage without impacting responsiveness.

**Acceptance Scenarios**:

1. **Given** all priority queues are empty, **When** scheduler detects idle state, **Then** idle callbacks run with time budget until next frame deadline
2. **Given** idle callback is running, **When** user input arrives, **Then** callback pauses immediately (within 1ms) and input is processed
3. **Given** idle work needs 50ms to complete, **When** only 16ms available before next frame, **Then** work runs incrementally across multiple idle periods
4. **Given** no work is pending and no idle callbacks scheduled, **When** system is idle, **Then** scheduler sleeps to conserve battery

---

### User Story 6 - Priority-Based Task Ordering (Priority: P2)

Developers can explicitly specify task priority when scheduling work, ensuring critical tasks always run before non-critical tasks.

**Why this priority**: Necessary for developers to control scheduling behavior, but the default priorities handle most cases automatically.

**Independent Test**: Can be tested by scheduling tasks with different priorities and verifying execution order. Delivers value: predictable task execution order.

**Acceptance Scenarios**:

1. **Given** tasks with priorities Immediate, High, Normal, Low, Idle are queued, **When** scheduler executes, **Then** tasks run in strict priority order
2. **Given** multiple tasks with equal priority, **When** scheduler executes them, **Then** tasks run in FIFO order (submission order preserved)
3. **Given** a Normal priority task is pending, **When** developer upgrades it to High priority, **Then** task runs before other Normal tasks in next scheduling cycle
4. **Given** task has deadline approaching, **When** scheduler evaluates priorities, **Then** task receives automatic priority boost as deadline nears

---

### User Story 7 - Task Cancellation (Priority: P2)

Developers can cancel pending tasks that are no longer needed, freeing resources immediately and preventing wasted CPU cycles.

**Why this priority**: Important for resource management, especially when users navigate away from content. Not critical for initial functionality.

**Independent Test**: Can be tested by cancelling tasks and verifying they don't execute. Delivers value: efficient resource usage.

**Acceptance Scenarios**:

1. **Given** 50 image load tasks are pending, **When** developer calls cancel with task handle, **Then** specific task is removed and doesn't execute
2. **Given** user navigates away from screen, **When** developer cancels all Low priority tasks, **Then** all matching tasks removed immediately
3. **Given** task is cancelled, **When** scheduler checks queue, **Then** task resources (memory, handles) are freed immediately
4. **Given** task is currently executing, **When** cancel is called, **Then** task completes current execution but doesn't run again if recurring

---

### User Story 8 - Deadline-Based Scheduling (Priority: P3)

Developers can specify deadlines for time-sensitive work, and the scheduler prioritizes tasks approaching their deadlines to meet timing requirements.

**Why this priority**: Nice to have for advanced use cases, but most work is either immediate (user input) or frame-based (animation). Can be added later.

**Independent Test**: Can be tested by scheduling tasks with deadlines and measuring execution timing. Delivers value: better control over time-sensitive operations.

**Acceptance Scenarios**:

1. **Given** task has absolute deadline of T+100ms, **When** current time is T+80ms, **Then** task priority is boosted to run soon
2. **Given** task has relative deadline of 50ms from submission, **When** scheduler evaluates priorities, **Then** task runs before lower-priority work
3. **Given** task misses its deadline, **When** scheduler detects this, **Then** deadline miss is logged with task details and execution continues
4. **Given** multiple tasks with approaching deadlines, **When** scheduler executes, **Then** tasks with soonest deadlines run first

---

### User Story 9 - Platform Integration (Priority: P1)

The scheduler integrates with platform-specific event loops (Win32 message loop, NSRunLoop, Wayland event loop, browser event loop) and vsync signals without busy-waiting or fighting the OS.

**Why this priority**: Core requirement for cross-platform functionality. Without proper platform integration, scheduler won't work correctly on all target platforms.

**Independent Test**: Can be tested on each platform by verifying event loop integration and vsync synchronization. Delivers value: works correctly on all platforms.

**Acceptance Scenarios**:

1. **Given** application runs on Windows, **When** scheduler integrates with Win32 message loop, **Then** tasks execute in response to messages without blocking
2. **Given** application runs on macOS, **When** scheduler receives vsync signal from display link, **Then** frame work is triggered at display refresh rate
3. **Given** no work is pending, **When** scheduler is idle, **Then** system sleeps using platform blocking API (not busy-wait)
4. **Given** application runs on 120Hz ProMotion display, **When** scheduler detects refresh rate, **Then** frame budget adjusts to 8.33ms automatically

---

### User Story 10 - Observable Performance (Priority: P3)

Developers can measure task execution time, track frame timing, and identify performance bottlenecks using built-in metrics.

**Why this priority**: Very useful for debugging and optimization, but not required for basic functionality. Can be added incrementally.

**Independent Test**: Can be tested by enabling metrics and verifying data collection. Delivers value: visibility into scheduler behavior for optimization.

**Acceptance Scenarios**:

1. **Given** developer enables metrics, **When** tasks execute, **Then** execution time is recorded per task priority level
2. **Given** frames are rendering, **When** developer queries metrics, **Then** timing for each phase (Layout, Paint, Commit) is available
3. **Given** frame is dropped, **When** developer checks metrics, **Then** dropped frame count increments and cause is logged
4. **Given** metrics are enabled, **When** developer exports data, **Then** metrics are available in format compatible with profiling tools

---

### Edge Cases

- What happens when a High priority task takes longer than its frame budget (16ms)?
  - Task is broken into time slices if possible, or frame is dropped and logged
- How does system handle task that panics during execution?
  - Panic is caught, logged with context, task marked failed, scheduler continues
- What happens if 10,000 tasks are submitted instantly?
  - Queue enforces maximum size, excess tasks rejected with error or throttled based on configuration
- How does system behave when deadline is already passed at task submission?
  - Task runs immediately at boosted priority, deadline miss is logged
- What happens if idle callback takes longer than available time budget?
  - Callback is paused mid-execution, resumed in next idle period with remaining budget
- How does system handle platform without vsync signal (headless environment)?
  - Scheduler uses timer-based frame triggering at target frame rate (60fps default)
- What happens when all priority queues are full?
  - New task submission fails with error, or oldest lowest-priority task is dropped (configurable)
- How does system handle recursive task submission (task submits another task)?
  - Allowed, newly submitted task queued normally, depth limit prevents infinite recursion

## Requirements *(mandatory)*

### Functional Requirements

**Task Scheduling and Execution**

- **FR-001**: System MUST support five distinct priority levels: Immediate (user input), High (animations), Normal (standard work), Low (background), Idle (opportunistic)
- **FR-002**: System MUST execute higher priority tasks before lower priority tasks without exception
- **FR-003**: System MUST preserve FIFO ordering for tasks of equal priority
- **FR-004**: System MUST support task submission from any thread (thread-safe submission)
- **FR-005**: System MUST execute tasks only on main thread (UI thread)
- **FR-006**: System MUST support recurring tasks that repeat at specified intervals
- **FR-007**: System MUST support one-shot tasks that execute exactly once
- **FR-008**: System MUST return task handle upon submission for later cancellation or status queries

**Frame Scheduling**

- **FR-009**: System MUST organize work into five sequential frame phases: BeginFrame, Update, Layout, Paint, Commit
- **FR-010**: System MUST ensure each phase completes before the next phase begins within a frame
- **FR-011**: System MUST synchronize frame execution with display vsync signal from platform
- **FR-012**: System MUST detect when frame work exceeds budget and mark frame as dropped
- **FR-013**: System MUST support adaptive frame rate for variable refresh displays (48Hz, 60Hz, 90Hz, 120Hz, 144Hz)
- **FR-014**: System MUST track time spent in each frame phase for performance monitoring
- **FR-015**: System MUST trigger frame work only when UI updates are pending (no unnecessary frames)

**Time Management**

- **FR-016**: System MUST prevent any single task from blocking for more than 16ms (configurable)
- **FR-017**: System MUST support time slicing for long-running tasks (break into chunks)
- **FR-018**: System MUST check for higher-priority work between time slices
- **FR-019**: System MUST support absolute deadlines (complete before specific timestamp)
- **FR-020**: System MUST support relative deadlines (complete within duration from submission)
- **FR-021**: System MUST automatically boost priority for tasks approaching deadline

**Idle Callbacks**

- **FR-022**: System MUST detect idle state when all priority queues (Immediate through Low) are empty
- **FR-023**: System MUST run idle callbacks with time budget parameter (deadline)
- **FR-024**: System MUST pause idle callback immediately when higher-priority work arrives
- **FR-025**: System MUST support resuming paused idle callbacks in subsequent idle periods
- **FR-026**: System MUST support priority levels for idle callbacks (relative to each other)

**Task Cancellation**

- **FR-027**: System MUST support cancelling individual tasks by handle
- **FR-028**: System MUST support cancelling all tasks matching criteria (priority, type, tag)
- **FR-029**: System MUST ensure cancelled tasks never execute after cancellation call returns
- **FR-030**: System MUST free resources (memory, handles) immediately upon cancellation

**Platform Integration**

- **FR-031**: System MUST integrate with Win32 message loop on Windows
- **FR-032**: System MUST integrate with NSRunLoop/CFRunLoop on macOS
- **FR-033**: System MUST integrate with event loops on Linux (libxcb, Wayland)
- **FR-034**: System MUST integrate with browser event loop (requestAnimationFrame) on WASM
- **FR-035**: System MUST use platform blocking APIs (no busy-wait loops)
- **FR-036**: System MUST sleep when idle to conserve battery

**Error Handling**

- **FR-037**: System MUST catch task panics and prevent scheduler crash
- **FR-038**: System MUST log panics with full context (task priority, type, backtrace)
- **FR-039**: System MUST continue scheduling after task panic
- **FR-040**: System MUST detect and log deadline misses
- **FR-041**: System MUST detect and log frame drops with cause (which phase overran)
- **FR-042**: System MUST enforce resource limits (maximum pending tasks, memory budget)

**Performance Observability**

- **FR-043**: System MUST track tasks executed per second by priority level
- **FR-044**: System MUST track average task execution time by priority level
- **FR-045**: System MUST track frame timing (total time and time per phase)
- **FR-046**: System MUST count dropped frames and identify causes
- **FR-047**: System MUST calculate idle time percentage
- **FR-048**: System MUST measure scheduler overhead (time spent scheduling vs executing)

### Key Entities

- **Task**: A unit of work with priority level, optional deadline, cancellation handle, and execution state (pending, running, completed, cancelled)
- **TaskHandle**: Opaque handle for cancelling tasks or querying status; can upgrade/downgrade priority if task still pending
- **Priority**: Enumeration of five levels - Immediate (user input), High (animations), Normal (standard), Low (background), Idle (opportunistic)
- **Frame**: Represents a single rendering frame with frame number, frame budget (16.67ms for 60fps), time spent per phase, dropped frame flag
- **FramePhase**: Enumeration of five phases - BeginFrame (input), Update (state), Layout (sizes), Paint (commands), Commit (GPU submission)
- **IdleCallback**: Callback that runs during idle with time budget, can be paused/resumed, has priority relative to other idle callbacks
- **SchedulerMetrics**: Performance data including tasks executed by priority, average timing, frame timing by phase, dropped frame count, idle percentage

## Success Criteria *(mandatory)*

### Measurable Outcomes

**Responsiveness**
- **SC-001**: User input events (touch, click, keyboard) are processed within 100ms in 99% of cases (p99 latency < 100ms)
- **SC-002**: Character display latency after keypress is under 50ms in 95% of cases
- **SC-003**: UI remains responsive (no frozen interface) even when 100 background tasks are queued

**Animation Performance**
- **SC-004**: Animations maintain consistent 60fps (frame interval 16.67ms ± 2ms) for 95% of frames during normal operation (< 50 tasks per frame)
- **SC-005**: Frame drops occur in less than 1% of frames under normal load
- **SC-006**: Scheduler adapts to 120Hz displays automatically and maintains 120fps when capable

**Efficiency**
- **SC-007**: Scheduler overhead is less than 5% of total CPU time (95%+ spent in actual task execution)
- **SC-008**: Idle callbacks don't prevent app from sleeping (battery drain < 2% per hour when idle)
- **SC-009**: Task submission and cancellation complete in under 10μs (microseconds) per operation

**Background Work**
- **SC-010**: Background tasks yield control within 16ms maximum, allowing UI work to interrupt
- **SC-011**: Task cancellation takes effect immediately (cancelled tasks never execute after cancel returns)
- **SC-012**: System handles 1000+ pending tasks without memory exhaustion or performance degradation

**Platform Support**
- **SC-013**: Scheduler works correctly on Windows, macOS, Linux, and WASM with platform-appropriate integration
- **SC-014**: Vsync synchronization achieves < 1ms timing drift from display refresh on all platforms

**Developer Experience**
- **SC-015**: Common scheduling patterns (priority, deadline, recurring) are implementable in under 10 lines of code
- **SC-016**: Performance metrics are accessible via simple API with < 1% overhead when enabled
- **SC-017**: All scheduling failures provide clear error messages indicating cause and resolution

**Robustness**
- **SC-018**: Scheduler continues functioning after task panics with no crashes (100% crash-free under task failures)
- **SC-019**: Stress testing with 10,000 concurrent task submissions completes without errors or deadlocks
- **SC-020**: 24-hour fuzz testing produces zero scheduler panics or undefined behavior

## Assumptions *(mandatory)*

### Frame Rate and Timing
- Default target frame rate is 60fps (16.67ms frame budget)
- Variable refresh rate displays (48Hz, 90Hz, 120Hz) are supported through platform APIs
- Frame budget is calculated as 1 / refresh_rate
- Vsync signal or equivalent is available on all target platforms

### Platform Capabilities
- All platforms provide event loop integration mechanisms
- Platform APIs for blocking (WaitForSingleObject, kevent, epoll) are available
- Main thread is dedicated to UI work (not shared with unrelated blocking operations)
- Background worker threads are optionally available for future enhancements

### Task Characteristics
- Tasks are relatively short-lived (< 100ms typical execution time)
- Long-running tasks can be chunked by developer or support time slicing
- Tasks are pure Rust code (no FFI calls in critical scheduling path)
- Tasks can be safely cancelled without undefined behavior

### Resource Constraints
- Task queue supports up to 10,000 pending tasks as reasonable upper bound
- Task closures are small (< 1KB per task on average)
- Memory available for task queue bookkeeping is within expected application limits

### Threading Model
- Main thread is available for UI work and task execution
- Task submission can occur from any thread (cross-thread submission supported)
- Task execution always happens on main thread (single-threaded execution model)
- Thread-safe primitives (mutexes, channels) are available for cross-thread coordination

## Scope Limitations *(mandatory)*

### Out of Scope for V1

**Parallel Task Execution**
- V1 uses single-threaded execution on main thread only
- Work-stealing schedulers and parallel task execution deferred to V2
- Multi-threaded layout or paint operations not included in V1

**Advanced Scheduling Algorithms**
- V1 uses priority queue-based scheduling (simple and predictable)
- Earliest Deadline First (EDF) or Completely Fair Scheduler (CFS) algorithms deferred to V2
- Complex scheduling heuristics for automatic priority adjustment not included

**Real-Time Guarantees**
- Scheduler is best-effort, not hard real-time
- No worst-case execution time (WCET) guarantees
- No guaranteed latency bounds or jitter limits (soft real-time only)

**Distributed Scheduling**
- No coordination between multiple processes or machines
- No network-aware scheduling or distributed task queues
- Single-process scope only

**GPU Command Scheduling**
- Scheduler handles CPU-side task scheduling only
- GPU command buffer scheduling is separate concern in flui-rendering
- No GPU workload balancing or command stream optimization

**Custom Async Executors**
- V1 includes built-in executor only
- Pluggable executor interface (tokio, async-std integration) deferred to V2
- Async/await support is optional and limited in V1

**Advanced Metrics**
- Basic metrics included (task count, timing, frame drops)
- Advanced profiling (flame graphs, call trees) not included
- Integration with external profiling tools (perf, Tracy) deferred

## Dependencies *(mandatory)*

### Internal Flui Dependencies
- **flui-types**: Time measurement types (Duration, Instant equivalents), Size, Constraints
- **flui-platform**: Platform event loop integration, vsync signal delivery, platform-specific APIs
- **flui-foundation**: Error types, Result wrappers, logging infrastructure

### External Dependencies

**Required (Minimal Standard Library Preferred)**
- **std::time**: Instant and Duration for timing measurements
- **std::collections**: VecDeque or BinaryHeap for priority queues
- **std::sync**: Mutex, Arc for thread-safe task submission
- Platform-specific event loop APIs:
  - Windows: Win32 message loop APIs
  - macOS: Core Foundation CFRunLoop APIs
  - Linux: libxcb or Wayland event APIs
  - WASM: browser event loop (requestAnimationFrame)

**Optional (Feature-Gated)**
- **crossbeam**: Lock-free queues for high-performance cross-thread task submission (optional optimization)
- **tokio** or **async-std**: Async runtime integration if async task support is enabled (feature flag)

**Avoided**
- Heavy runtime dependencies
- Mandatory async runtimes (async support is opt-in only)
- Complex third-party scheduling libraries

## Design Constraints *(mandatory)*

### No Busy-Waiting
- Scheduler MUST NOT use spin loops or busy-wait
- Use platform blocking APIs (WaitForSingleObject on Windows, kevent on BSD/macOS, epoll on Linux)
- Wake on vsync signal or new task submission
- Sleep when idle to conserve battery and CPU

### Bounded Latency
- Immediate tasks MUST execute within 100ms (p99 latency)
- Requires preemption or time slicing of lower-priority work
- Long-running tasks must be interruptible

### No Priority Inversion
- High-priority task MUST NEVER be blocked indefinitely by low-priority task
- Bounded priority inversion is acceptable (< 1 frame duration)
- Unbounded priority inversion MUST be prevented

### Deterministic Testing
- Scheduler behavior SHOULD be deterministic given same inputs
- Same task submission order + controlled clock → same execution order
- Enables reproducible testing and debugging

### Low Overhead
- Scheduler overhead MUST be under 5% of total CPU time
- 95%+ of CPU time spent in actual task execution
- Lock-free or fine-grained locking for task submission
- Efficient priority queue implementation (binary heap: O(log n) insert/remove)

### Platform-Agnostic API
- Public API MUST NOT expose platform-specific types
- Platform differences handled internally via trait abstraction
- Same scheduling API works across Windows, macOS, Linux, WASM
