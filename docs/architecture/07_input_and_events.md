# Chapter 7: Input & Events

## 📋 Overview

Input система обрабатывает пользовательский ввод (mouse, touch, keyboard) и превращает его в events которые propagate через widget tree. **Hit testing** определяет какие виджеты получают события, а **event bubbling** позволяет обрабатывать события на разных уровнях иерархии.

## 🎯 Event System

### Event Types

```rust
/// Base Event trait
pub trait Event: Debug + Send + Sync + 'static {
    fn event_type(&self) -> EventType;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventType {
    Pointer,    // Mouse, touch, stylus
    Keyboard,   // Keyboard input
    Focus,      // Focus gain/loss
    Lifecycle,  // Widget lifecycle events
}

// ═══════════════════════════════════════════════════════════════
// Pointer Events
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct PointerEvent {
    pub kind: PointerEventKind,
    pub position: Offset,
    pub device: PointerDevice,
    pub buttons: PointerButtons,
    pub timestamp: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEventKind {
    Down,
    Move,
    Up,
    Cancel,
    Hover,
    Enter,
    Exit,
}

// ═══════════════════════════════════════════════════════════════
// Keyboard Events
// ═══════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct KeyboardEvent {
    pub kind: KeyboardEventKind,
    pub key: Key,
    pub modifiers: Modifiers,
    pub is_repeat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Character(char),
    Enter,
    Tab,
    Backspace,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    // ... more keys
}
```

## 🎯 Hit Testing

```rust
impl RenderPipeline {
    pub fn hit_test(&self, position: Offset) -> HitTestResult {
        let mut result = HitTestResult {
            path: Vec::new(),
            local_positions: HashMap::new(),
        };
        
        self.hit_test_recursive(ElementId::root(), position, Mat4::identity(), &mut result);
        
        result
    }
    
    fn hit_test_recursive(
        &self,
        element_id: ElementId,
        global_position: Offset,
        transform: Mat4,
        result: &mut HitTestResult,
    ) -> bool {
        // Transform to local coordinates
        let local_position = transform.inverse().transform_point(global_position);
        
        // Check bounds
        let size = self.size_cache.get(&element_id).copied().unwrap_or(Size::ZERO);
        let bounds = Rect::from_origin_size(Offset::ZERO, size);
        
        if !bounds.contains(local_position) {
            return false;
        }
        
        // Test children (reverse order - top to bottom)
        let children = self.tree.borrow().children(element_id);
        let mut hit_child = false;
        
        for &child_id in children.iter().rev() {
            let child_offset = self.offset_cache.get(&child_id).copied().unwrap_or(Offset::ZERO);
            let child_transform = transform * Mat4::translate(child_offset.x, child_offset.y, 0.0);
            
            if self.hit_test_recursive(child_id, global_position, child_transform, result) {
                hit_child = true;
                break;
            }
        }
        
        // Add to path if hit
        if hit_child || self.element_hit_test(element_id, local_position) {
            result.path.push(element_id);
            result.local_positions.insert(element_id, local_position);
            return true;
        }
        
        false
    }
}
```

## 🔄 Event Dispatch

```rust
pub struct EventDispatcher {
    focus: Option<ElementId>,
    hover: Option<ElementId>,
}

impl EventDispatcher {
    pub fn dispatch_pointer_event(&mut self, event: PointerEvent, pipeline: &RenderPipeline) {
        // Perform hit test
        let hit_result = pipeline.hit_test(event.position);
        
        // Dispatch to hit path (bubbling)
        for &element_id in &hit_result.path {
            let local_position = hit_result.local_positions[&element_id];
            
            let local_event = PointerEvent {
                position: local_position,
                ..event.clone()
            };
            
            if self.dispatch_to_element(element_id, &local_event, pipeline) {
                break; // Event handled
            }
        }
        
        // Update hover state
        self.update_hover(hit_result.path.first().copied(), &event, pipeline);
    }
}
```

## 🔗 Cross-References

- **Previous:** [Chapter 6: Render Backend](06_render_backend.md)
- **Next:** [Chapter 8: Frame Scheduler](08_frame_scheduler.md)

---

**Key Takeaway:** FLUI's event system provides precise hit testing and event bubbling for responsive UIs!
