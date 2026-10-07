### Changed

- **Async tasks are owner-local** (`flui-scheduler`, `flui::view`;
  [ADR-0136](/docs/adr/ADR-0136-owner-local-ui-surfaces.md) §2). A widget's future may now hold
  `Rc` state, `Signal<T>` or a `WriterSource`; only its `Waker` crosses threads. The realm owns
  the tasks, and a handle that outlives the realm keeps none of them alive.
  - `BoxedTask = Pin<Box<dyn Future<Output = ()> + Send>>` → `Pin<Box<dyn Future<Output = ()>>>`:
    no change at call sites; a `Send` future still coerces.
  - `AsyncDriver`, `TaskToken`: `Send + Sync` → `!Send`. Keep them on the owner thread; a worker
    wakes the task through its `Waker`, or asks for a frame through a `FrameWaker`.
  - `AsyncDriver` no longer owns tasks: it is a `Weak` handle to the realm's. `spawn_local` on a
    handle whose realm is gone drops the future at once and returns a cancelled `TaskToken`
    instead of queueing it. `AsyncDriver::new()` → `OwnerFrame::new(&scheduler).async_driver()`;
    `AsyncDriver::{poll_ready, ready_task_count}` → `OwnerFrame::{poll_ready, ready_task_count}`
    (the frame polls; runtimes and tests that drive a scheduler by hand call it).
  - A realm's teardown drops its remaining tasks, its frame hook and its owner-local post-frame
    callbacks on the owner thread, each under its own catch, before it raises any earlier failure.
  - `AsyncDriver::set_request_frame` → test-only (the `testing` feature): the realm's `OwnerFrame`
    installs the hook, and a widget can no longer replace it.
  - `UpdateScheduler::{async_driver, spawn_local, spawn_local_eager, drive_async_tasks,
    pending_task_count}` → removed. Spawn through `LifecycleContext::async_driver()` (widgets) or
    `HeadlessBinding::spawn_local` (tests); count with `AsyncDriver::pending_task_count`.
  - `FutureBuilder<K: Clone + PartialEq + Send + Sync, T: Send, E: Send>` →
    `FutureBuilder<K: Clone + PartialEq + Debug + 'static, T: 'static, E: 'static>`;
    `BoxedResultFuture<T, E>` loses its `Send` bound.
  - New: `UpdateScheduler::frame_waker() -> FrameWaker` (`Clone + Send + Sync`,
    `FrameWaker::request_frame()`), the cross-thread "please run a frame" capability. It wakes its
    own realm only and does nothing once the realm is gone. `FrameWaker` is re-exported from
    `flui::view`.
- **Frame entry points take the realm's owner frame** (`flui-scheduler`; runtimes and tests that
  drive a scheduler by hand). `LocalPostFrameLane` is replaced by `OwnerFrame`
  (`#[doc(hidden)]`), which also holds the async tasks; the lane-less and `*_with_lane` variants
  are removed:
  - `UpdateScheduler::new_local_post_frame_lane()` → `OwnerFrame::new(&scheduler)?`, which returns
    `Err(OwnerFrameError::AlreadyOwned)` while another `OwnerFrame` for that scheduler lives: a frame
    polls only the owner it is handed, so a second owner's tasks would never run;
    `lane.local_handle()` → `owner.local_post_frame_handle()`.
  - `drive_frame(vsync, deadline, pipeline)` / `drive_frame_with_lane(vsync, deadline, pipeline,
    &lane)` → `drive_frame(&owner, vsync, deadline, pipeline)`.
  - `execute_frame()` / `execute_frame_with_lane(&lane)` → `execute_frame(&owner)`;
    `schedule_warm_up_frame()` → `schedule_warm_up_frame(&owner)`.
  - `end_frame()` / `end_frame_with_lane(&lane)` → `end_frame(&owner)`;
    `handle_begin_frame(vsync)` → `handle_begin_frame(vsync, &owner)`.
  - A background wake (`finish_async_pump()` then `drive_async_tasks()`) →
    `finish_async_pump()` then `owner.poll_ready()`.
