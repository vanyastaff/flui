# Chapter 8: Frame Scheduler

## 📋 Overview

Frame Scheduler координирует весь rendering pipeline: **build → layout → paint → composite**. Он использует **VSync** для smooth 60+ FPS и приоритизирует work для responsive UI.

## 🔄 Frame Lifecycle

```
┌─────────────────────────────────────────────────────────┐
│                    Frame N                              │
├─────────────────────────────────────────────────────────┤
│ 1. Input Events        → EventDispatcher               │
│ 2. Animation Tick      → AnimationController           │
│ 3. Signal Updates      → ReactiveRuntime               │
│ 4. Build Phase         → Element.rebuild()             │
│ 5. Layout Phase        → RenderPipeline.flush_layout() │
│ 6. Paint Phase         → RenderPipeline.flush_paint()  │
│ 7. Composite Phase     → Compositor.composite()        │
│ 8. Present             → Backend.end_frame()           │
└─────────────────────────────────────────────────────────┘
                           ↓
                      VSync Signal
                           ↓
┌─────────────────────────────────────────────────────────┐
│                    Frame N+1                            │
└─────────────────────────────────────────────────────────┘
```

## 🎯 FrameScheduler Implementation

```rust
pub struct FrameScheduler {
    /// VSync callback
    vsync: VSync,
    
    /// Element tree
    tree: Rc<RefCell<ElementTree>>,
    
    /// Render pipeline
    pipeline: RenderPipeline,
    
    /// Compositor
    compositor: Compositor,
    
    /// Event dispatcher
    event_dispatcher: EventDispatcher,
    
    /// Animation controller
    animation_controller: AnimationController,
    
    /// Frame stats
    stats: FrameStats,
}

impl FrameScheduler {
    pub fn run(&mut self) {
        loop {
            // Wait for VSync
            self.vsync.wait();
            
            let frame_start = Instant::now();
            
            // Execute frame
            self.execute_frame();
            
            let frame_time = frame_start.elapsed();
            self.stats.record_frame(frame_time);
            
            // Check if we're maintaining target FPS
            if frame_time > Duration::from_millis(16) {
                warn!("Frame took {}ms (target: 16ms)", frame_time.as_millis());
            }
        }
    }
    
    fn execute_frame(&mut self) {
        // 1. Process input events
        self.process_input();
        
        // 2. Tick animations
        self.animation_controller.tick(self.vsync.delta_time());
        
        // 3. Flush reactive updates
        REACTIVE_RUNTIME.with(|rt| rt.borrow_mut().flush());
        
        // 4. Build phase (rebuild dirty elements)
        self.build_phase();
        
        // 5. Layout phase
        let window_size = self.window_size();
        let root_constraints = BoxConstraints::tight(window_size);
        self.pipeline.flush_layout(root_constraints);
        
        // 6. Paint phase
        let root_layer = self.pipeline.flush_paint();
        
        // 7. Composite & present
        self.compositor.composite(&root_layer, window_size);
    }
    
    fn build_phase(&mut self) {
        let mut tree = self.tree.borrow_mut();
        
        // Get dirty elements
        let dirty_elements = tree.take_dirty_elements();
        
        for element_id in dirty_elements {
            if let Some(element) = tree.get_mut(element_id) {
                // Rebuild element
                let new_children = element.rebuild(element_id);
                
                // Update tree with new children
                for (parent_id, child_widget, slot) in new_children {
                    tree.update_child(parent_id, slot, child_widget);
                }
            }
        }
    }
}
```

## 📊 Frame Budget Management

```rust
pub struct FrameBudget {
    /// Target frame time (16.67ms for 60fps)
    target_frame_time: Duration,
    
    /// Time spent on build phase
    build_time: Duration,
    
    /// Time spent on layout phase
    layout_time: Duration,
    
    /// Time spent on paint phase
    paint_time: Duration,
    
    /// Time spent on composite phase
    composite_time: Duration,
}

impl FrameBudget {
    pub fn new_60fps() -> Self {
        Self {
            target_frame_time: Duration::from_micros(16_667), // 1/60 second
            build_time: Duration::ZERO,
            layout_time: Duration::ZERO,
            paint_time: Duration::ZERO,
            composite_time: Duration::ZERO,
        }
    }
    
    pub fn is_over_budget(&self) -> bool {
        self.total_time() > self.target_frame_time
    }
    
    pub fn total_time(&self) -> Duration {
        self.build_time + self.layout_time + self.paint_time + self.composite_time
    }
    
    pub fn remaining_budget(&self) -> Duration {
        self.target_frame_time.saturating_sub(self.total_time())
    }
}
```

## ⏱️ VSync Integration

```rust
pub struct VSync {
    /// Last VSync timestamp
    last_vsync: Instant,
    
    /// Frame callback
    callback: Box<dyn Fn()>,
}

impl VSync {
    pub fn new(callback: impl Fn() + 'static) -> Self {
        Self {
            last_vsync: Instant::now(),
            callback: Box::new(callback),
        }
    }
    
    pub fn wait(&mut self) {
        // Wait for next VSync signal from platform
        // This is platform-specific
        
        #[cfg(target_os = "windows")]
        self.wait_windows();
        
        #[cfg(target_os = "linux")]
        self.wait_linux();
        
        #[cfg(target_os = "macos")]
        self.wait_macos();
        
        // Update timestamp
        self.last_vsync = Instant::now();
    }
    
    pub fn delta_time(&self) -> Duration {
        self.last_vsync.elapsed()
    }
    
    #[cfg(target_os = "windows")]
    fn wait_windows(&self) {
        // Use D3DKMTWaitForVerticalBlankEvent
        // or DwmFlush
    }
    
    #[cfg(target_os = "linux")]
    fn wait_linux(&self) {
        // Use glXSwapBuffers with swap interval = 1
    }
    
    #[cfg(target_os = "macos")]
    fn wait_macos(&self) {
        // Use CVDisplayLink
    }
}
```

## 🎯 Priority Scheduling

```rust
pub enum TaskPriority {
    /// User interaction (highest priority)
    Immediate,
    
    /// Animation frames
    High,
    
    /// Normal updates
    Normal,
    
    /// Background tasks
    Low,
    
    /// Idle time tasks
    Idle,
}

pub struct PriorityScheduler {
    queues: HashMap<TaskPriority, VecDeque<Task>>,
}

impl PriorityScheduler {
    pub fn schedule(&mut self, task: Task, priority: TaskPriority) {
        self.queues.entry(priority)
            .or_insert_with(VecDeque::new)
            .push_back(task);
    }
    
    pub fn execute_tasks(&mut self, budget: Duration) -> Duration {
        let start = Instant::now();
        
        // Execute tasks in priority order
        for priority in [
            TaskPriority::Immediate,
            TaskPriority::High,
            TaskPriority::Normal,
            TaskPriority::Low,
            TaskPriority::Idle,
        ] {
            if let Some(queue) = self.queues.get_mut(&priority) {
                while let Some(task) = queue.pop_front() {
                    task.execute();
                    
                    // Check budget
                    if start.elapsed() >= budget {
                        return start.elapsed();
                    }
                }
            }
        }
        
        start.elapsed()
    }
}
```

## 📊 Performance Monitoring

```rust
#[derive(Debug, Default)]
pub struct FrameStats {
    frame_times: VecDeque<Duration>,
    frame_count: u64,
}

impl FrameStats {
    pub fn record_frame(&mut self, duration: Duration) {
        self.frame_times.push_back(duration);
        if self.frame_times.len() > 120 {
            self.frame_times.pop_front();
        }
        self.frame_count += 1;
    }
    
    pub fn avg_frame_time(&self) -> Duration {
        let sum: Duration = self.frame_times.iter().sum();
        sum / self.frame_times.len() as u32
    }
    
    pub fn fps(&self) -> f32 {
        1000.0 / self.avg_frame_time().as_millis() as f32
    }
    
    pub fn frame_time_percentile(&self, percentile: f32) -> Duration {
        let mut sorted: Vec<_> = self.frame_times.iter().copied().collect();
        sorted.sort();
        
        let index = (sorted.len() as f32 * percentile) as usize;
        sorted[index.min(sorted.len() - 1)]
    }
}
```

## 🔗 Cross-References

- **Previous:** [Chapter 7: Input & Events](07_input_and_events.md)
- **Next:** [Chapter 9: Debug & DevTools](09_debug_and_devtools.md)
- **Related:** [Chapter 1: Architecture](01_architecture.md)

---

**Key Takeaway:** FLUI's frame scheduler ensures smooth 60+ FPS through VSync synchronization, priority-based task scheduling, and frame budget management!
