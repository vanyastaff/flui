# Phase 7: Frame Scheduling Layer (flui-scheduler) - Детальный План Реализации

> **Базируется на**: `docs/plans/2026-01-22-core-architecture-design.md`  
> **Предыдущие фазы**: Phase 1-6 должны быть завершены  
> **Референсы**: `.gpui/src/app.rs`, Flutter Scheduler, `SchedulerBinding`  
> **Цель**: Production-ready frame scheduling с VSync, task prioritization, и animation coordination

---

## Обзор Текущего Состояния

### ✅ Что Уже Есть

#### flui-scheduler
- ✅ Cargo.toml с зависимостями
- ✅ Модульная структура: `scheduler.rs`, `task.rs`, `ticker.rs`, `vsync.rs`, `frame.rs`
- ✅ Базовый Scheduler
- ✅ Task system с priority
- ✅ Ticker для animations
- ✅ VSync abstraction
- ✅ Duration utilities
- ✅ Frame callbacks
- ✅ Typestate pattern для safety

#### Зависимости готовы
- ✅ flui-foundation - BindingBase
- ✅ web-time - cross-platform time
- ✅ parking_lot - thread-safe sync
- ✅ dashmap - lock-free collections
- ✅ crossbeam - concurrent utilities
- ✅ event-listener - async completion

### ❌ Что Нужно Доделать / Улучшить

#### Core Scheduler System
1. **VSync Integration** - frame timing from platform
2. **Frame Phases** - Build → Layout → Paint phases
3. **Task Priorities** - Immediate, Animation, Layout, Idle
4. **Frame Callbacks** - requestAnimationFrame equivalent
5. **Throttling** - respect target framerate (60fps, 120fps)

#### Animation System
1. **Ticker** - drives animations
2. **AnimationController** - controls animation lifecycle
3. **Tween** - interpolation between values
4. **Curves** - easing functions

#### Task Scheduling
1. **Priority Queue** - schedule tasks by priority
2. **Frame Budget** - time budget per frame
3. **Idle Callbacks** - run during idle time
4. **Microtasks** - high-priority immediate tasks

---

## Детальный План Реализации

### Этап 7.1: Core Scheduler Architecture (Неделя 13, Дни 1-3)

#### День 1: VSync & Frame Timing

**Цель**: Implement VSync integration with platform

**Референсы**:
- `.gpui/src/app.rs` - GPUI frame scheduling
- Flutter `SchedulerBinding`
- Existing `vsync.rs`

**Задачи**:

1. **Обновить `vsync.rs`**
   ```rust
   use web_time::{Duration, Instant};
   use std::sync::Arc;
   use parking_lot::Mutex;
   
   /// VSync provider
   ///
   /// Provides frame timing signals synchronized with display refresh.
   /// On platforms with VSync support, this ensures smooth 60fps (or 120fps)
   /// rendering. On platforms without VSync, falls back to timer-based scheduling.
   pub trait VSync: Send + Sync + 'static {
       /// Request a VSync callback
       ///
       /// The callback will be invoked at the next VSync interval.
       fn request_frame(&self, callback: Box<dyn FnOnce(FrameTime) + Send>);
       
       /// Get target frame interval
       fn frame_interval(&self) -> Duration {
           Duration::from_millis(16) // ~60fps
       }
       
       /// Get display refresh rate (Hz)
       fn refresh_rate(&self) -> f64 {
           60.0 // Default: 60 Hz
       }
   }
   
   /// Frame time information
   #[derive(Copy, Clone, Debug)]
   pub struct FrameTime {
       /// Timestamp when frame was scheduled
       pub scheduled: Instant,
       
       /// Actual timestamp when callback was invoked
       pub actual: Instant,
       
       /// Frame number (monotonically increasing)
       pub frame_number: u64,
       
       /// Time since last frame
       pub delta: Duration,
   }
   
   /// Platform VSync (from platform event loop)
   pub struct PlatformVSync {
       /// Platform's VSync provider
       platform: Arc<dyn flui_platform::Platform>,
       
       /// Pending callback
       callback: Arc<Mutex<Option<Box<dyn FnOnce(FrameTime) + Send>>>>,
       
       /// Frame counter
       frame_number: Arc<AtomicU64>,
       
       /// Last frame time
       last_frame: Arc<Mutex<Option<Instant>>>,
   }
   
   impl PlatformVSync {
       pub fn new(platform: Arc<dyn flui_platform::Platform>) -> Self {
           Self {
               platform,
               callback: Arc::new(Mutex::new(None)),
               frame_number: Arc::new(AtomicU64::new(0)),
               last_frame: Arc::new(Mutex::new(None)),
           }
       }
   }
   
   impl VSync for PlatformVSync {
       fn request_frame(&self, callback: Box<dyn FnOnce(FrameTime) + Send>) {
           *self.callback.lock() = Some(callback);
           
           // Request frame from platform
           let callback = Arc::clone(&self.callback);
           let frame_number = Arc::clone(&self.frame_number);
           let last_frame = Arc::clone(&self.last_frame);
           
           self.platform.request_frame(Box::new(move || {
               let now = Instant::now();
               let last = last_frame.lock().replace(now);
               let delta = last.map(|l| now.duration_since(l))
                   .unwrap_or(Duration::from_millis(16));
               
               let frame_num = frame_number.fetch_add(1, Ordering::Relaxed);
               
               let frame_time = FrameTime {
                   scheduled: now,
                   actual: now,
                   frame_number: frame_num,
                   delta,
               };
               
               if let Some(cb) = callback.lock().take() {
                   cb(frame_time);
               }
           }));
       }
       
       fn refresh_rate(&self) -> f64 {
           // Get from platform display info
           self.platform.primary_display()
               .map(|d| d.refresh_rate())
               .unwrap_or(60.0)
       }
   }
   
   /// Timer-based VSync (fallback)
   ///
   /// For platforms without VSync support or for testing.
   pub struct TimerVSync {
       interval: Duration,
       frame_number: Arc<AtomicU64>,
       last_frame: Arc<Mutex<Option<Instant>>>,
   }
   
   impl TimerVSync {
       pub fn new(fps: u32) -> Self {
           Self {
               interval: Duration::from_millis(1000 / fps as u64),
               frame_number: Arc::new(AtomicU64::new(0)),
               last_frame: Arc::new(Mutex::new(None)),
           }
       }
   }
   
   impl VSync for TimerVSync {
       fn request_frame(&self, callback: Box<dyn FnOnce(FrameTime) + Send>) {
           let interval = self.interval;
           let frame_number = Arc::clone(&self.frame_number);
           let last_frame = Arc::clone(&self.last_frame);
           
           // Spawn timer task
           std::thread::spawn(move || {
               std::thread::sleep(interval);
               
               let now = Instant::now();
               let last = last_frame.lock().replace(now);
               let delta = last.map(|l| now.duration_since(l))
                   .unwrap_or(interval);
               
               let frame_num = frame_number.fetch_add(1, Ordering::Relaxed);
               
               let frame_time = FrameTime {
                   scheduled: now,
                   actual: now,
                   frame_number: frame_num,
                   delta,
               };
               
               callback(frame_time);
           });
       }
       
       fn frame_interval(&self) -> Duration {
           self.interval
       }
       
       fn refresh_rate(&self) -> f64 {
           1000.0 / self.interval.as_millis() as f64
       }
   }
   ```

**Критерии завершения**:
- [ ] VSync trait works
- [ ] PlatformVSync integrates with platform
- [ ] TimerVSync fallback works
- [ ] FrameTime accurate
- [ ] 25+ vsync tests

---

#### День 2: Scheduler & Frame Phases

**Цель**: Core scheduler with Build/Layout/Paint phases

**Задачи**:

1. **Обновить `scheduler.rs`**
   ```rust
   use dashmap::DashMap;
   use crossbeam::channel::{Sender, Receiver, unbounded};
   use parking_lot::{RwLock, Mutex};
   use std::sync::Arc;
   use web_time::{Duration, Instant};
   
   /// Scheduler
   ///
   /// Central coordinator for frame scheduling and task execution.
   /// Manages frame phases (Build → Layout → Paint) and task priorities.
   pub struct Scheduler {
       /// VSync provider
       vsync: Arc<dyn VSync>,
       
       /// Current phase
       phase: Arc<RwLock<FramePhase>>,
       
       /// Frame callbacks
       frame_callbacks: Arc<Mutex<FrameCallbacks>>,
       
       /// Task queue
       task_queue: Arc<TaskQueue>,
       
       /// Frame requested flag
       frame_requested: Arc<AtomicBool>,
       
       /// Frame in progress
       frame_in_progress: Arc<AtomicBool>,
       
       /// Performance metrics
       metrics: Arc<RwLock<FrameMetrics>>,
   }
   
   impl Scheduler {
       pub fn new(vsync: Arc<dyn VSync>) -> Self {
           Self {
               vsync,
               phase: Arc::new(RwLock::new(FramePhase::Idle)),
               frame_callbacks: Arc::new(Mutex::new(FrameCallbacks::new())),
               task_queue: Arc::new(TaskQueue::new()),
               frame_requested: Arc::new(AtomicBool::new(false)),
               frame_in_progress: Arc::new(AtomicBool::new(false)),
               metrics: Arc::new(RwLock::new(FrameMetrics::default())),
           }
       }
       
       /// Schedule a frame
       ///
       /// If no frame is currently in progress, requests a VSync callback.
       pub fn schedule_frame(&self) {
           if self.frame_requested.swap(true, Ordering::Relaxed) {
               // Already requested
               return;
           }
           
           tracing::debug!("Scheduling frame");
           
           let scheduler = Self {
               vsync: Arc::clone(&self.vsync),
               phase: Arc::clone(&self.phase),
               frame_callbacks: Arc::clone(&self.frame_callbacks),
               task_queue: Arc::clone(&self.task_queue),
               frame_requested: Arc::clone(&self.frame_requested),
               frame_in_progress: Arc::clone(&self.frame_in_progress),
               metrics: Arc::clone(&self.metrics),
           };
           
           self.vsync.request_frame(Box::new(move |frame_time| {
               scheduler.handle_frame(frame_time);
           }));
       }
       
       /// Handle a frame
       fn handle_frame(&self, frame_time: FrameTime) {
           if self.frame_in_progress.swap(true, Ordering::Relaxed) {
               tracing::warn!("Frame already in progress, skipping");
               return;
           }
           
           self.frame_requested.store(false, Ordering::Relaxed);
           
           let start = Instant::now();
           
           tracing::trace!(
               "Frame {} started (delta: {:?})",
               frame_time.frame_number,
               frame_time.delta
           );
           
           // Execute frame phases
           self.run_frame_phases(frame_time);
           
           let duration = start.elapsed();
           
           // Update metrics
           {
               let mut metrics = self.metrics.write();
               metrics.record_frame(duration);
           }
           
           tracing::trace!(
               "Frame {} completed in {:?}",
               frame_time.frame_number,
               duration
           );
           
           self.frame_in_progress.store(false, Ordering::Relaxed);
           
           // Schedule next frame if requested
           if self.frame_requested.load(Ordering::Relaxed) {
               self.schedule_frame();
           }
       }
       
       /// Run frame phases
       fn run_frame_phases(&self, frame_time: FrameTime) {
           // Phase 1: Transient callbacks (microtasks)
           self.enter_phase(FramePhase::TransientCallbacks);
           self.invoke_transient_callbacks(frame_time);
           
           // Phase 2: Persistent callbacks (animations)
           self.enter_phase(FramePhase::PersistentCallbacks);
           self.invoke_persistent_callbacks(frame_time);
           
           // Phase 3: Post-frame callbacks (cleanup)
           self.enter_phase(FramePhase::PostFrameCallbacks);
           self.invoke_post_frame_callbacks(frame_time);
           
           // Phase 4: Idle
           self.enter_phase(FramePhase::Idle);
       }
       
       fn enter_phase(&self, phase: FramePhase) {
           tracing::trace!("Entering phase: {:?}", phase);
           *self.phase.write() = phase;
       }
       
       fn invoke_transient_callbacks(&self, frame_time: FrameTime) {
           let callbacks = self.frame_callbacks.lock()
               .drain_transient();
           
           for callback in callbacks {
               callback(frame_time);
           }
       }
       
       fn invoke_persistent_callbacks(&self, frame_time: FrameTime) {
           let callbacks = self.frame_callbacks.lock()
               .persistent_callbacks();
           
           for callback in callbacks {
               callback(frame_time);
           }
       }
       
       fn invoke_post_frame_callbacks(&self, frame_time: FrameTime) {
           let callbacks = self.frame_callbacks.lock()
               .drain_post_frame();
           
           for callback in callbacks {
               callback(frame_time);
           }
       }
       
       /// Add transient frame callback (fires once)
       pub fn add_transient_frame_callback<F>(&self, callback: F)
       where
           F: FnOnce(FrameTime) + Send + 'static,
       {
           self.frame_callbacks.lock()
               .add_transient(Box::new(callback));
           
           self.schedule_frame();
       }
       
       /// Add persistent frame callback (fires every frame)
       pub fn add_persistent_frame_callback<F>(&self, callback: F) -> CallbackHandle
       where
           F: Fn(FrameTime) + Send + Sync + 'static,
       {
           let handle = self.frame_callbacks.lock()
               .add_persistent(Arc::new(callback));
           
           self.schedule_frame();
           
           handle
       }
       
       /// Remove persistent callback
       pub fn remove_persistent_frame_callback(&self, handle: CallbackHandle) {
           self.frame_callbacks.lock()
               .remove_persistent(handle);
       }
       
       /// Add post-frame callback
       pub fn add_post_frame_callback<F>(&self, callback: F)
       where
           F: FnOnce(FrameTime) + Send + 'static,
       {
           self.frame_callbacks.lock()
               .add_post_frame(Box::new(callback));
       }
       
       /// Get current phase
       pub fn current_phase(&self) -> FramePhase {
           *self.phase.read()
       }
       
       /// Get frame metrics
       pub fn metrics(&self) -> FrameMetrics {
           self.metrics.read().clone()
       }
   }
   
   /// Frame phase
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum FramePhase {
       /// No frame in progress
       Idle,
       
       /// Executing transient callbacks
       TransientCallbacks,
       
       /// Executing persistent callbacks
       PersistentCallbacks,
       
       /// Executing post-frame callbacks
       PostFrameCallbacks,
   }
   
   /// Frame callbacks
   struct FrameCallbacks {
       transient: Vec<Box<dyn FnOnce(FrameTime) + Send>>,
       persistent: DashMap<CallbackHandle, Arc<dyn Fn(FrameTime) + Send + Sync>>,
       post_frame: Vec<Box<dyn FnOnce(FrameTime) + Send>>,
       next_handle: AtomicU64,
   }
   
   impl FrameCallbacks {
       fn new() -> Self {
           Self {
               transient: Vec::new(),
               persistent: DashMap::new(),
               post_frame: Vec::new(),
               next_handle: AtomicU64::new(1),
           }
       }
       
       fn add_transient(&mut self, callback: Box<dyn FnOnce(FrameTime) + Send>) {
           self.transient.push(callback);
       }
       
       fn add_persistent(&mut self, callback: Arc<dyn Fn(FrameTime) + Send + Sync>) -> CallbackHandle {
           let handle = CallbackHandle(self.next_handle.fetch_add(1, Ordering::Relaxed));
           self.persistent.insert(handle, callback);
           handle
       }
       
       fn remove_persistent(&mut self, handle: CallbackHandle) {
           self.persistent.remove(&handle);
       }
       
       fn add_post_frame(&mut self, callback: Box<dyn FnOnce(FrameTime) + Send>) {
           self.post_frame.push(callback);
       }
       
       fn drain_transient(&mut self) -> Vec<Box<dyn FnOnce(FrameTime) + Send>> {
           std::mem::take(&mut self.transient)
       }
       
       fn persistent_callbacks(&self) -> Vec<Arc<dyn Fn(FrameTime) + Send + Sync>> {
           self.persistent.iter()
               .map(|entry| Arc::clone(entry.value()))
               .collect()
       }
       
       fn drain_post_frame(&mut self) -> Vec<Box<dyn FnOnce(FrameTime) + Send>> {
           std::mem::take(&mut self.post_frame)
       }
   }
   
   /// Callback handle (for removing persistent callbacks)
   #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
   pub struct CallbackHandle(u64);
   
   /// Frame metrics
   #[derive(Clone, Debug, Default)]
   pub struct FrameMetrics {
       /// Average frame duration (ms)
       pub avg_frame_duration: f64,
       
       /// Max frame duration (ms)
       pub max_frame_duration: f64,
       
       /// Min frame duration (ms)
       pub min_frame_duration: f64,
       
       /// Frame count
       pub frame_count: u64,
       
       /// Dropped frames (took longer than target)
       pub dropped_frames: u64,
   }
   
   impl FrameMetrics {
       fn record_frame(&mut self, duration: Duration) {
           let ms = duration.as_secs_f64() * 1000.0;
           
           self.frame_count += 1;
           
           if self.frame_count == 1 {
               self.avg_frame_duration = ms;
               self.max_frame_duration = ms;
               self.min_frame_duration = ms;
           } else {
               self.avg_frame_duration = (self.avg_frame_duration * (self.frame_count - 1) as f64 + ms) / self.frame_count as f64;
               self.max_frame_duration = self.max_frame_duration.max(ms);
               self.min_frame_duration = self.min_frame_duration.min(ms);
           }
           
           // Frame budget: 16.67ms for 60fps
           if duration.as_millis() > 16 {
               self.dropped_frames += 1;
           }
       }
       
       pub fn fps(&self) -> f64 {
           1000.0 / self.avg_frame_duration
       }
       
       pub fn drop_rate(&self) -> f64 {
           if self.frame_count == 0 {
               0.0
           } else {
               self.dropped_frames as f64 / self.frame_count as f64
           }
       }
   }
   ```

**Критерии завершения**:
- [ ] Scheduler coordinates frames
- [ ] Frame phases work
- [ ] Callbacks fire correctly
- [ ] Metrics tracked
- [ ] 40+ scheduler tests

---

#### День 3: Task Priority System

**Цель**: Task scheduling with priorities

**Задачи**:

1. **Обновить `task.rs`**
   ```rust
   use std::cmp::Ordering;
   use std::sync::Arc;
   use parking_lot::Mutex;
   use crossbeam::queue::SegQueue;
   
   /// Task queue
   ///
   /// Schedules tasks by priority.
   pub struct TaskQueue {
       /// High-priority tasks (animations, input)
       high_priority: SegQueue<Task>,
       
       /// Normal-priority tasks (layout, build)
       normal_priority: SegQueue<Task>,
       
       /// Low-priority tasks (cleanup, analytics)
       low_priority: SegQueue<Task>,
       
       /// Idle tasks (run when nothing else to do)
       idle_tasks: SegQueue<Task>,
   }
   
   impl TaskQueue {
       pub fn new() -> Self {
           Self {
               high_priority: SegQueue::new(),
               normal_priority: SegQueue::new(),
               low_priority: SegQueue::new(),
               idle_tasks: SegQueue::new(),
           }
       }
       
       /// Schedule a task
       pub fn schedule(&self, task: Task) {
           match task.priority {
               TaskPriority::Immediate => self.high_priority.push(task),
               TaskPriority::Animation => self.high_priority.push(task),
               TaskPriority::Normal => self.normal_priority.push(task),
               TaskPriority::Low => self.low_priority.push(task),
               TaskPriority::Idle => self.idle_tasks.push(task),
           }
       }
       
       /// Get next task to execute
       pub fn next_task(&self) -> Option<Task> {
           // Priority order: High → Normal → Low → Idle
           self.high_priority.pop()
               .or_else(|| self.normal_priority.pop())
               .or_else(|| self.low_priority.pop())
               .or_else(|| self.idle_tasks.pop())
       }
       
       /// Execute tasks until time budget exhausted
       pub fn execute_tasks(&self, budget: Duration) -> usize {
           let start = Instant::now();
           let mut count = 0;
           
           while start.elapsed() < budget {
               if let Some(task) = self.next_task() {
                   task.execute();
                   count += 1;
               } else {
                   break;
               }
           }
           
           count
       }
   }
   
   /// Task
   pub struct Task {
       /// Task priority
       priority: TaskPriority,
       
       /// Task callback
       callback: Box<dyn FnOnce() + Send>,
       
       /// Task name (for debugging)
       name: Option<String>,
   }
   
   impl Task {
       pub fn new<F>(priority: TaskPriority, callback: F) -> Self
       where
           F: FnOnce() + Send + 'static,
       {
           Self {
               priority,
               callback: Box::new(callback),
               name: None,
           }
       }
       
       pub fn with_name(mut self, name: impl Into<String>) -> Self {
           self.name = Some(name.into());
           self
       }
       
       fn execute(self) {
           if let Some(name) = &self.name {
               tracing::trace!("Executing task: {}", name);
           }
           
           (self.callback)();
       }
   }
   
   /// Task priority
   #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
   pub enum TaskPriority {
       /// Immediate execution (microtasks)
       Immediate = 0,
       
       /// Animation frame (smooth 60fps)
       Animation = 1,
       
       /// Normal priority (layout, build)
       Normal = 2,
       
       /// Low priority (cleanup, logging)
       Low = 3,
       
       /// Idle (run when nothing else to do)
       Idle = 4,
   }
   ```

**Критерии завершения**:
- [ ] TaskQueue works
- [ ] Priority ordering correct
- [ ] Time budget respected
- [ ] 30+ task tests

---

### Этап 7.2: Animation System (Неделя 13-14, Дні 4-6)

#### День 4: Ticker & AnimationController

**Цель**: Core animation infrastructure

**Задачи**:

1. **Обновить `ticker.rs`**
   ```rust
   /// Ticker
   ///
   /// Drives animations by providing elapsed time on each frame.
   pub struct Ticker {
       /// Callback handle
       handle: Option<CallbackHandle>,
       
       /// Scheduler
       scheduler: Arc<Scheduler>,
       
       /// Start time
       start_time: Option<Instant>,
       
       /// Callback
       callback: Arc<Mutex<Option<Box<dyn Fn(Duration) + Send + Sync>>>>,
       
       /// Is active
       active: Arc<AtomicBool>,
   }
   
   impl Ticker {
       pub fn new(scheduler: Arc<Scheduler>) -> Self {
           Self {
               handle: None,
               scheduler,
               start_time: None,
               callback: Arc::new(Mutex::new(None)),
               active: Arc::new(AtomicBool::new(false)),
           }
       }
       
       /// Start ticker
       pub fn start<F>(&mut self, callback: F)
       where
           F: Fn(Duration) + Send + Sync + 'static,
       {
           if self.active.load(Ordering::Relaxed) {
               return; // Already running
           }
           
           self.start_time = Some(Instant::now());
           *self.callback.lock() = Some(Box::new(callback));
           self.active.store(true, Ordering::Relaxed);
           
           let start_time = self.start_time.unwrap();
           let callback = Arc::clone(&self.callback);
           let active = Arc::clone(&self.active);
           
           self.handle = Some(self.scheduler.add_persistent_frame_callback(move |frame_time| {
               if !active.load(Ordering::Relaxed) {
                   return;
               }
               
               let elapsed = frame_time.actual.duration_since(start_time);
               
               if let Some(cb) = &*callback.lock() {
                   cb(elapsed);
               }
           }));
       }
       
       /// Stop ticker
       pub fn stop(&mut self) {
           if !self.active.swap(false, Ordering::Relaxed) {
               return; // Already stopped
           }
           
           if let Some(handle) = self.handle.take() {
               self.scheduler.remove_persistent_frame_callback(handle);
           }
           
           self.start_time = None;
           *self.callback.lock() = None;
       }
       
       /// Check if ticker is active
       pub fn is_active(&self) -> bool {
           self.active.load(Ordering::Relaxed)
       }
   }
   
   impl Drop for Ticker {
       fn drop(&mut self) {
           self.stop();
       }
   }
   ```

2. **AnimationController**
   ```rust
   /// Animation controller
   ///
   /// Controls animation lifecycle (start, stop, reverse, repeat).
   pub struct AnimationController {
       /// Ticker
       ticker: Ticker,
       
       /// Duration
       duration: Duration,
       
       /// Current value (0.0 - 1.0)
       value: Arc<RwLock<f64>>,
       
       /// Status
       status: Arc<RwLock<AnimationStatus>>,
       
       /// Direction
       direction: Arc<RwLock<AnimationDirection>>,
       
       /// Listeners
       listeners: Arc<Mutex<Vec<Box<dyn Fn(f64) + Send + Sync>>>>,
       
       /// Status listeners
       status_listeners: Arc<Mutex<Vec<Box<dyn Fn(AnimationStatus) + Send + Sync>>>>,
   }
   
   impl AnimationController {
       pub fn new(scheduler: Arc<Scheduler>, duration: Duration) -> Self {
           Self {
               ticker: Ticker::new(scheduler),
               duration,
               value: Arc::new(RwLock::new(0.0)),
               status: Arc::new(RwLock::new(AnimationStatus::Dismissed)),
               direction: Arc::new(RwLock::new(AnimationDirection::Forward)),
               listeners: Arc::new(Mutex::new(Vec::new())),
               status_listeners: Arc::new(Mutex::new(Vec::new())),
           }
       }
       
       /// Forward animation
       pub fn forward(&mut self) {
           *self.direction.write() = AnimationDirection::Forward;
           self.set_status(AnimationStatus::Forward);
           
           let duration = self.duration;
           let value = Arc::clone(&self.value);
           let status = Arc::clone(&self.status);
           let listeners = Arc::clone(&self.listeners);
           let status_listeners = Arc::clone(&self.status_listeners);
           
           self.ticker.start(move |elapsed| {
               let t = (elapsed.as_secs_f64() / duration.as_secs_f64()).min(1.0);
               
               *value.write() = t;
               
               // Notify listeners
               for listener in listeners.lock().iter() {
                   listener(t);
               }
               
               // Check completion
               if t >= 1.0 {
                   *status.write() = AnimationStatus::Completed;
                   
                   for listener in status_listeners.lock().iter() {
                       listener(AnimationStatus::Completed);
                   }
               }
           });
       }
       
       /// Reverse animation
       pub fn reverse(&mut self) {
           *self.direction.write() = AnimationDirection::Reverse;
           self.set_status(AnimationStatus::Reverse);
           
           let duration = self.duration;
           let value = Arc::clone(&self.value);
           let status = Arc::clone(&self.status);
           let listeners = Arc::clone(&self.listeners);
           let status_listeners = Arc::clone(&self.status_listeners);
           
           self.ticker.start(move |elapsed| {
               let t = 1.0 - (elapsed.as_secs_f64() / duration.as_secs_f64()).min(1.0);
               
               *value.write() = t.max(0.0);
               
               for listener in listeners.lock().iter() {
                   listener(t);
               }
               
               if t <= 0.0 {
                   *status.write() = AnimationStatus::Dismissed;
                   
                   for listener in status_listeners.lock().iter() {
                       listener(AnimationStatus::Dismissed);
                   }
               }
           });
       }
       
       /// Stop animation
       pub fn stop(&mut self) {
           self.ticker.stop();
           self.set_status(AnimationStatus::Dismissed);
       }
       
       /// Repeat animation
       pub fn repeat(&mut self) {
           // TODO: Implement repeat logic
       }
       
       /// Get current value
       pub fn value(&self) -> f64 {
           *self.value.read()
       }
       
       /// Get status
       pub fn status(&self) -> AnimationStatus {
           *self.status.read()
       }
       
       /// Add listener
       pub fn add_listener<F>(&self, listener: F)
       where
           F: Fn(f64) + Send + Sync + 'static,
       {
           self.listeners.lock().push(Box::new(listener));
       }
       
       /// Add status listener
       pub fn add_status_listener<F>(&self, listener: F)
       where
           F: Fn(AnimationStatus) + Send + Sync + 'static,
       {
           self.status_listeners.lock().push(Box::new(listener));
       }
       
       fn set_status(&self, status: AnimationStatus) {
           *self.status.write() = status;
           
           for listener in self.status_listeners.lock().iter() {
               listener(status);
           }
       }
   }
   
   /// Animation status
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum AnimationStatus {
       /// Animation is dismissed (value = 0)
       Dismissed,
       
       /// Animation is running forward
       Forward,
       
       /// Animation is running in reverse
       Reverse,
       
       /// Animation completed (value = 1)
       Completed,
   }
   
   /// Animation direction
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum AnimationDirection {
       Forward,
       Reverse,
   }
   ```

**Критерії завершення**:
- [ ] Ticker drives animations
- [ ] AnimationController works
- [ ] Forward/reverse/stop work
- [ ] Listeners notified
- [ ] 35+ animation tests

---

#### День 5: Tween & Interpolation

**Цель**: Value interpolation

**Задачи**:

1. **Tween (створити `tween.rs`)**
   ```rust
   /// Tween (interpolation between values)
   ///
   /// Defines how to interpolate between a begin and end value.
   pub trait Tween<T>: Send + Sync {
       /// Interpolate at time t (0.0 - 1.0)
       fn lerp(&self, t: f64) -> T;
       
       /// Begin value
       fn begin(&self) -> &T;
       
       /// End value
       fn end(&self) -> &T;
   }
   
   /// Tween between two values
   pub struct ValueTween<T> {
       begin: T,
       end: T,
   }
   
   impl<T> ValueTween<T> {
       pub fn new(begin: T, end: T) -> Self {
           Self { begin, end }
       }
   }
   
   impl Tween<f64> for ValueTween<f64> {
       fn lerp(&self, t: f64) -> f64 {
           self.begin + (self.end - self.begin) * t
       }
       
       fn begin(&self) -> &f64 {
           &self.begin
       }
       
       fn end(&self) -> &f64 {
           &self.end
       }
   }
   
   impl Tween<Color> for ValueTween<Color> {
       fn lerp(&self, t: f64) -> Color {
           Color {
               r: (self.begin.r as f64 + (self.end.r as f64 - self.begin.r as f64) * t) as u8,
               g: (self.begin.g as f64 + (self.end.g as f64 - self.begin.g as f64) * t) as u8,
               b: (self.begin.b as f64 + (self.end.b as f64 - self.begin.b as f64) * t) as u8,
               a: (self.begin.a as f64 + (self.end.a as f64 - self.begin.a as f64) * t) as u8,
           }
       }
       
       fn begin(&self) -> &Color {
           &self.begin
       }
       
       fn end(&self) -> &Color {
           &self.end
       }
   }
   
   impl Tween<Offset> for ValueTween<Offset> {
       fn lerp(&self, t: f64) -> Offset {
           Offset {
               x: self.begin.x + (self.end.x - self.begin.x) * t as f32,
               y: self.begin.y + (self.end.y - self.begin.y) * t as f32,
           }
       }
       
       fn begin(&self) -> &Offset {
           &self.begin
       }
       
       fn end(&self) -> &Offset {
           &self.end
       }
   }
   
   /// Animated value
   pub struct AnimatedValue<T> {
       tween: Box<dyn Tween<T>>,
       controller: Arc<AnimationController>,
       curve: Box<dyn Curve>,
   }
   
   impl<T> AnimatedValue<T> {
       pub fn new(
           tween: Box<dyn Tween<T>>,
           controller: Arc<AnimationController>,
       ) -> Self {
           Self {
               tween,
               controller,
               curve: Box::new(LinearCurve),
           }
       }
       
       pub fn with_curve(mut self, curve: Box<dyn Curve>) -> Self {
           self.curve = curve;
           self
       }
       
       pub fn value(&self) -> T {
           let t = self.controller.value();
           let curved_t = self.curve.transform(t);
           self.tween.lerp(curved_t)
       }
   }
   ```

**Критерії завершення**:
- [ ] Tween trait works
- [ ] ValueTween works for common types
- [ ] AnimatedValue works
- [ ] 25+ tween tests

---

#### День 6: Curves (Easing Functions)

**Цель**: Animation curves

**Задачи**:

1. **Curve Trait**
   ```rust
   /// Curve (easing function)
   ///
   /// Transforms linear time (0.0 - 1.0) to curved time.
   pub trait Curve: Send + Sync {
       /// Transform time
       fn transform(&self, t: f64) -> f64;
   }
   
   /// Linear curve (no easing)
   pub struct LinearCurve;
   
   impl Curve for LinearCurve {
       fn transform(&self, t: f64) -> f64 {
           t
       }
   }
   
   /// Ease-in curve (slow start)
   pub struct EaseInCurve;
   
   impl Curve for EaseInCurve {
       fn transform(&self, t: f64) -> f64 {
           t * t
       }
   }
   
   /// Ease-out curve (slow end)
   pub struct EaseOutCurve;
   
   impl Curve for EaseOutCurve {
       fn transform(&self, t: f64) -> f64 {
           1.0 - (1.0 - t) * (1.0 - t)
       }
   }
   
   /// Ease-in-out curve (slow start and end)
   pub struct EaseInOutCurve;
   
   impl Curve for EaseInOutCurve {
       fn transform(&self, t: f64) -> f64 {
           if t < 0.5 {
               2.0 * t * t
           } else {
               1.0 - 2.0 * (1.0 - t) * (1.0 - t)
           }
       }
   }
   
   /// Elastic curve (bouncy)
   pub struct ElasticCurve {
       period: f64,
   }
   
   impl ElasticCurve {
       pub fn new(period: f64) -> Self {
           Self { period }
       }
   }
   
   impl Curve for ElasticCurve {
       fn transform(&self, t: f64) -> f64 {
           let s = self.period / 4.0;
           if t == 0.0 || t == 1.0 {
               t
           } else {
               -((2.0_f64).powf(10.0 * (t - 1.0))) *
                   ((t - 1.0 - s) * (2.0 * std::f64::consts::PI) / self.period).sin()
           }
       }
   }
   
   /// Bounce curve
   pub struct BounceCurve;
   
   impl Curve for BounceCurve {
       fn transform(&self, t: f64) -> f64 {
           if t < 1.0 / 2.75 {
               7.5625 * t * t
           } else if t < 2.0 / 2.75 {
               let t = t - 1.5 / 2.75;
               7.5625 * t * t + 0.75
           } else if t < 2.5 / 2.75 {
               let t = t - 2.25 / 2.75;
               7.5625 * t * t + 0.9375
           } else {
               let t = t - 2.625 / 2.75;
               7.5625 * t * t + 0.984375
           }
       }
   }
   
   /// Cubic Bezier curve
   pub struct CubicCurve {
       a: f64,
       b: f64,
       c: f64,
       d: f64,
   }
   
   impl CubicCurve {
       pub fn new(p1x: f64, p1y: f64, p2x: f64, p2y: f64) -> Self {
           Self {
               a: 3.0 * p1x - 3.0 * p2x + 1.0,
               b: -6.0 * p1x + 3.0 * p2x,
               c: 3.0 * p1x,
               d: -3.0 * p1y + 3.0 * p2y,
           }
       }
   }
   
   impl Curve for CubicCurve {
       fn transform(&self, t: f64) -> f64 {
           // Solve cubic equation
           let t2 = t * t;
           let t3 = t2 * t;
           self.a * t3 + self.b * t2 + self.c * t
       }
   }
   ```

**Критерії завершення**:
- [ ] Curve trait works
- [ ] Common curves work (ease-in, ease-out, etc.)
- [ ] Elastic/bounce curves work
- [ ] 30+ curve tests

---

### Етап 7.3: Integration & Polish (Тиждень 14, Дні 7-10)

#### День 7: SchedulerBinding

**Цель**: Integrate with flui_app

**Задачи**:

1. **SchedulerBinding (створити `binding.rs`)**
   ```rust
   use flui_foundation::BindingBase;
   
   /// Scheduler binding
   ///
   /// Integrates scheduler with app lifecycle.
   pub struct SchedulerBinding {
       scheduler: Arc<Scheduler>,
   }
   
   impl SchedulerBinding {
       pub fn new(vsync: Arc<dyn VSync>) -> Self {
           Self {
               scheduler: Arc::new(Scheduler::new(vsync)),
           }
       }
       
       pub fn scheduler(&self) -> &Arc<Scheduler> {
           &self.scheduler
       }
   }
   
   impl BindingBase for SchedulerBinding {
       fn init_instances(&mut self) {
           tracing::info!("SchedulerBinding initialized");
       }
   }
   ```

**Критерії завершення**:
- [ ] SchedulerBinding works
- [ ] Integrates with BindingBase
- [ ] 15+ binding tests

---

#### День 8: Performance Optimization

**Цель**: Optimize scheduler performance

**Задачи**:

1. **Frame Budget**
   ```rust
   impl Scheduler {
       /// Set frame budget (time limit per frame)
       pub fn set_frame_budget(&self, budget: Duration) {
           // Limit work per frame to budget
       }
       
       /// Measure frame budget usage
       pub fn frame_budget_usage(&self) -> f64 {
           // Return 0.0 - 1.0 (percentage of budget used)
           0.0
       }
   }
   ```

2. **Microbenchmarks**
   ```rust
   #[bench]
   fn bench_schedule_frame(b: &mut Bencher) {
       let vsync = Arc::new(TimerVSync::new(60));
       let scheduler = Scheduler::new(vsync);
       
       b.iter(|| {
           scheduler.schedule_frame();
       });
   }
   ```

**Критерії завершення**:
- [ ] Frame budget works
- [ ] Performance acceptable
- [ ] 10+ benchmarks

---

#### День 9: Integration Testing

**Цель**: End-to-end tests

**Задачи**:

1. **Full Animation Test**
   ```rust
   #[test]
   fn test_full_animation() {
       let vsync = Arc::new(TimerVSync::new(60));
       let scheduler = Arc::new(Scheduler::new(vsync));
       
       let mut controller = AnimationController::new(
           Arc::clone(&scheduler),
           Duration::from_secs(1),
       );
       
       let mut values = Vec::new();
       controller.add_listener(move |v| {
           values.push(v);
       });
       
       controller.forward();
       
       // Wait for animation
       std::thread::sleep(Duration::from_millis(1100));
       
       assert!(values.len() > 50); // At least 50 frames
       assert!((values.last().unwrap() - 1.0).abs() < 0.01);
   }
   ```

**Критерії завершення**:
- [ ] All tests pass (150+)
- [ ] Integration tests work
- [ ] No race conditions

---

#### День 10: Documentation

**Цель**: Complete documentation

**Задачи**:

1. **README.md**
2. **API docs**
3. **Examples**
4. **Architecture diagrams**

**Критерії завершення**:
- [ ] cargo doc builds
- [ ] README complete
- [ ] Examples work
- [ ] Architecture documented

---

## Критерії Завершення Phase 7

- [ ] **flui-scheduler 0.1.0**
  - [ ] VSync integration
  - [ ] Frame scheduling works
  - [ ] Task priorities work
  - [ ] Animation system complete
  - [ ] Curves implemented
  - [ ] Performance acceptable
  - [ ] 150+ tests pass

---

**Статус**: 🟡 Ready for Implementation  
**Останнє оновлення**: 2026-01-22  
**Автор**: Claude with executing-plans skill  
**Базується на**: GPUI app.rs + Flutter Scheduler + original architecture design
