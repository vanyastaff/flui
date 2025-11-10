//! Example: Unified Event System - ThemeProvider Pattern
//!
//! This example demonstrates the unified event system where a single `handle_event()`
//! method handles ALL types of events (window, pointer, keyboard, etc.).
//!
//! # Architecture
//!
//! ```text
//! Event Source (OS/User) → FluiApp → PipelineOwner.dispatch_event()
//!                                        ↓
//!                                    All elements in tree
//!                                        ↓
//!                                    Element.handle_event()
//!                                        ↓
//!                          Element matches on Event enum:
//!                          - Event::Window(...) → ThemeProvider
//!                          - Event::Pointer(...) → Button
//!                          - Event::Key(...) → TextField
//!                                        ↓
//!                          Updates state + marks dirty
//! ```
//!
//! # Usage
//!
//! Run: `cargo run --example theme_provider_demo`

use flui_core::pipeline::PipelineOwner;
use flui_types::{Event, Theme, WindowEvent};

fn main() {
    println!("=== Unified Event System Demo ===\n");
    println!("This example demonstrates the SINGLE handle_event() method");
    println!("that handles ALL event types (window, pointer, keyboard, etc.)\n");

    // Create an empty pipeline (no actual tree needed for demo)
    let mut owner = PipelineOwner::new();

    println!("📋 Unified Event System:");
    println!("   ✨ ONE method: handle_event(&mut self, event: &Event) -> bool");
    println!("   ✨ Match on Event enum to choose what to handle");
    println!("   ✨ Supports Window, Pointer, Keyboard, Scroll events\n");

    println!("🔍 Example Implementation:");
    println!("```rust");
    println!("fn handle_event(&mut self, event: &Event) -> bool {{");
    println!("    match event {{");
    println!("        // Window events");
    println!("        Event::Window(WindowEvent::ThemeChanged {{ theme }}) => {{");
    println!("            self.update_theme(*theme);");
    println!("            true");
    println!("        }}");
    println!("        // Pointer events");
    println!("        Event::Pointer(PointerEvent::Down(_)) => {{");
    println!("            self.on_click();");
    println!("            true");
    println!("        }}");
    println!("        // Keyboard events");
    println!("        Event::Key(KeyEvent::Down(_)) => {{");
    println!("            self.on_key_press();");
    println!("            true");
    println!("        }}");
    println!("        _ => false // Ignore other events");
    println!("    }}");
    println!("}}");
    println!("```\n");

    println!("--- Simulating Different Event Types ---\n");

    // Event 1: Window event - Theme change
    let event = Event::Window(WindowEvent::ThemeChanged { theme: Theme::Dark });
    println!("📨 Event 1: {:?}", event);
    println!("   ↓ PipelineOwner.dispatch_event(&event)");
    println!("   ↓ Element.handle_event(&event)");
    println!("   ↓ match event {{ Event::Window(WindowEvent::ThemeChanged {{ ... }}) => ... }}");
    println!("   ✅ ThemeProvider handles: Light → Dark");
    owner.dispatch_event(&event);
    println!();

    // Event 2: Window event - Focus change
    let event = Event::Window(WindowEvent::FocusChanged { focused: false });
    println!("📨 Event 2: {:?}", event);
    println!("   ↓ Element matches Event::Window(FocusChanged {{ ... }})");
    println!("   ✅ AnimationController handles: Pause animations");
    owner.dispatch_event(&event);
    println!();

    // Event 3: Window event - Visibility change
    let event = Event::Window(WindowEvent::VisibilityChanged { visible: false });
    println!("📨 Event 3: {:?}", event);
    println!("   ↓ Element matches Event::Window(VisibilityChanged {{ ... }})");
    println!("   ✅ MediaPlayer handles: Pause playback");
    owner.dispatch_event(&event);
    println!();

    // Event 4: Window event - DPI/Scale change
    let event = Event::Window(WindowEvent::ScaleChanged { scale: 2.0 });
    println!("📨 Event 4: {:?}", event);
    println!("   ↓ Element matches Event::Window(ScaleChanged {{ ... }})");
    println!("   ✅ RenderImage handles: Reload textures at 2x scale");
    owner.dispatch_event(&event);
    println!();

    println!("=== Demo Complete ===\n");
    println!("✅ Key Advantages of Unified Event System:");
    println!("   1. ONE method instead of many (handle_event vs handle_window_event, handle_pointer_event, etc.)");
    println!("   2. Elements choose which events to handle via match");
    println!("   3. Easy to add new event types without changing Element API");
    println!("   4. Clear event flow: Event enum → match → handle");
    println!("   5. Type-safe event data access\n");

    println!("💡 Real-World Event Handling:");
    println!("   ThemeProvider:");
    println!("     - Event::Window(ThemeChanged) → Update theme");
    println!();
    println!("   Button:");
    println!("     - Event::Pointer(Down) → Visual press");
    println!("     - Event::Pointer(Up) → Trigger callback");
    println!();
    println!("   TextField:");
    println!("     - Event::Key(Down) → Insert character");
    println!("     - Event::Pointer(Down) → Set cursor position");
    println!();
    println!("   AnimationController:");
    println!("     - Event::Window(FocusChanged) → Pause/resume");
    println!("     - Event::Window(VisibilityChanged) → Stop/start");
    println!();
}
