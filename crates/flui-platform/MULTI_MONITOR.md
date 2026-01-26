# Multi-Monitor Best Practices

This guide covers best practices for developing applications that work correctly across multiple monitors with varying DPI settings, resolutions, and arrangements.

## Table of Contents

- [Overview](#overview)
- [Display Enumeration](#display-enumeration)
- [DPI Awareness](#dpi-awareness)
- [Window Positioning](#window-positioning)
- [Event Handling](#event-handling)
- [Common Patterns](#common-patterns)
- [Testing Strategies](#testing-strategies)
- [Platform-Specific Considerations](#platform-specific-considerations)

## Overview

Modern multi-monitor setups can include:
- **Mixed DPI**: 4K (2x) monitor + 1080p (1x) monitor
- **Different orientations**: Portrait + landscape
- **Different refresh rates**: 144Hz gaming monitor + 60Hz standard monitor
- **Different sizes**: 27" + 24" + 13" laptop display

Your application must handle all these scenarios gracefully.

## Display Enumeration

### Basic Enumeration

```rust
use flui_platform::{current_platform, PlatformDisplay};

let platform = current_platform()?;
let displays = platform.displays();

for display in displays.iter() {
    println!("Display: {}", display.name());
    println!("  Resolution: {}x{}", 
        display.bounds().size.width.0,
        display.bounds().size.height.0
    );
    println!("  Scale: {}x", display.scale_factor());
    println!("  Primary: {}", display.is_primary());
}
```

### Finding the Primary Display

```rust
let primary = displays.iter()
    .find(|d| d.is_primary())
    .expect("No primary display found");

println!("Primary display: {}", primary.name());
```

### Finding the Best Display for a Window

```rust
/// Find the display that contains the most area of the given bounds
fn find_best_display<'a>(
    displays: &'a [Arc<dyn PlatformDisplay>],
    window_bounds: Bounds<DevicePixels>
) -> &'a Arc<dyn PlatformDisplay> {
    displays.iter()
        .max_by_key(|d| {
            // Calculate intersection area
            calculate_intersection_area(d.bounds(), window_bounds)
        })
        .unwrap_or(&displays[0])
}
```

## DPI Awareness

### Understanding Scale Factors

| Scale Factor | DPI (Windows) | DPI (macOS) | Common Name |
|--------------|---------------|-------------|-------------|
| 1.0          | 96            | 72          | Standard DPI |
| 1.25         | 120           | 90          | 125% scaling |
| 1.5          | 144           | 108         | 150% scaling |
| 2.0          | 192           | 144         | Retina/HiDPI |
| 3.0          | 288           | 216         | 4K/UHD |

### Always Use Logical Pixels for UI

```rust
// ❌ WRONG: Using device pixels directly
let button_width = 100; // Will be wrong on HiDPI displays

// ✅ CORRECT: Using logical pixels
use flui_types::geometry::px;
let button_width = px(100.0); // Automatically scaled for DPI
```

### Converting Between Logical and Device Pixels

```rust
// Logical → Device (for rendering)
let device_width = logical_width.0 * scale_factor as f32;

// Device → Logical (for events)
let logical_x = device_x as f32 / scale_factor as f32;

// Use display.logical_size() for convenience
let logical_size = display.logical_size();
```

### Handling Scale Factor Changes

```rust
platform.on_window_event(Box::new(|event| {
    match event {
        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            // Window moved to display with different DPI
            println!("New scale factor: {}x", scale_factor);
            
            // Actions to take:
            // 1. Regenerate scaled assets (icons, images)
            // 2. Recalculate font sizes
            // 3. Update render buffers
            // 4. Request redraw
        }
        _ => {}
    }
}));
```

## Window Positioning

### Best Practices

1. **Always save window positions in logical coordinates**
   ```rust
   // Save
   let logical_pos = window.logical_position();
   config.save("window_x", logical_pos.x.0);
   config.save("window_y", logical_pos.y.0);
   
   // Restore
   let x = config.get("window_x");
   let y = config.get("window_y");
   window.set_logical_position(Point::new(px(x), px(y)));
   ```

2. **Validate restored positions are still visible**
   ```rust
   fn validate_window_position(
       pos: Point<Pixels>,
       size: Size<Pixels>,
       displays: &[Arc<dyn PlatformDisplay>]
   ) -> Point<Pixels> {
       // Check if window is visible on any display
       for display in displays {
           let bounds = display.bounds();
           let usable = display.usable_bounds();
           
           // Convert to logical
           let scale = display.scale_factor() as f32;
           let logical_bounds = Rect::new(
               px(bounds.origin.x.0 as f32 / scale),
               px(bounds.origin.y.0 as f32 / scale),
               px(bounds.size.width.0 as f32 / scale),
               px(bounds.size.height.0 as f32 / scale),
           );
           
           if logical_bounds.contains_point(pos) {
               return pos; // Position is valid
           }
       }
       
       // Position is off-screen, use primary display center
       let primary = displays.iter().find(|d| d.is_primary()).unwrap();
       let center = primary.logical_size();
       Point::new(
           center.width / 2.0 - size.width / 2.0,
           center.height / 2.0 - size.height / 2.0,
       )
   }
   ```

3. **Respect usable bounds (taskbar/menu bar)**
   ```rust
   let usable = display.usable_bounds();
   // Don't position windows outside usable area
   // System UI (taskbar, menu bar) is excluded
   ```

### Window Placement Strategies

**Center on Primary Display**:
```rust
let primary = displays.iter().find(|d| d.is_primary()).unwrap();
let screen_size = primary.logical_size();
let window_size = Size::new(px(800.0), px(600.0));

let x = (screen_size.width.0 - window_size.width.0) / 2.0;
let y = (screen_size.height.0 - window_size.height.0) / 2.0;

window.set_logical_position(Point::new(px(x), px(y)));
```

**Remember Last Position**:
```rust
if let Some(saved_pos) = load_saved_position() {
    if is_position_visible(saved_pos, displays) {
        window.set_logical_position(saved_pos);
    } else {
        // Fall back to default position
        center_window_on_primary(&window, displays);
    }
}
```

**Cascade Windows**:
```rust
const CASCADE_OFFSET: f32 = 30.0;
let cascade_pos = Point::new(
    px(base_x + window_count as f32 * CASCADE_OFFSET),
    px(base_y + window_count as f32 * CASCADE_OFFSET),
);
```

## Event Handling

### Critical Events

1. **ScaleFactorChanged**
   - Fires when window moves between displays with different DPI
   - Regenerate all DPI-dependent resources
   - Update font rendering
   - Recreate render buffers

2. **Resized**
   - Happens automatically when scale factor changes
   - Update viewport and projection matrices
   - Recreate swap chain/render targets

3. **Moved**
   - Track which display the window is on
   - Useful for multi-monitor game fullscreen

### Event Handler Example

```rust
platform.on_window_event(Box::new(|event| {
    match event {
        WindowEvent::ScaleFactorChanged { scale_factor, window_id } => {
            tracing::info!("Window {} moved to {}x DPI display", window_id.0, scale_factor);
            
            // Update application state
            app_state.scale_factor = scale_factor;
            app_state.regenerate_scaled_assets();
            app_state.request_redraw();
        }
        
        WindowEvent::Moved { position, window_id } => {
            // Determine which display the window is now on
            let current_display = find_display_at_position(position, &displays);
            
            if current_display.id() != app_state.last_display_id {
                tracing::info!("Window moved to display: {}", current_display.name());
                app_state.last_display_id = current_display.id();
            }
        }
        
        _ => {}
    }
}));
```

## Common Patterns

### Pattern 1: Fullscreen on Specific Display

```rust
/// Enter fullscreen on the display that contains the window
fn enter_fullscreen_on_current_display(
    window: &mut dyn PlatformWindow,
    displays: &[Arc<dyn PlatformDisplay>]
) {
    let window_pos = window.physical_position();
    
    // Find the display with the largest intersection
    let target_display = displays.iter()
        .max_by_key(|d| {
            calculate_intersection_area(d.bounds(), window.physical_bounds())
        })
        .unwrap();
    
    // Enter fullscreen on that display
    window.set_fullscreen(Some(target_display.id()));
}
```

### Pattern 2: Synchronized Windows Across Displays

```rust
/// Keep two windows synchronized (e.g., presenter view + audience view)
struct DualWindowSync {
    presenter_window: WindowId,
    audience_window: WindowId,
}

impl DualWindowSync {
    fn handle_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::Resized { window_id, .. } if window_id == &self.presenter_window => {
                // Sync content to audience window
                self.update_audience_content();
            }
            _ => {}
        }
    }
}
```

### Pattern 3: Per-Display UI Scaling

```rust
/// Adjust UI scale based on display DPI
fn calculate_ui_scale(display: &dyn PlatformDisplay) -> f32 {
    let scale_factor = display.scale_factor() as f32;
    
    // Base UI scale on effective DPI
    let effective_dpi = 96.0 * scale_factor; // Windows base
    
    match effective_dpi {
        0.0..=96.0 => 1.0,      // Standard DPI
        96.0..=144.0 => 1.25,   // Slightly scaled
        144.0..=192.0 => 1.5,   // Medium scaling
        _ => 2.0,               // High DPI
    }
}
```

## Testing Strategies

### Manual Testing Checklist

- [ ] Test on single monitor (standard DPI)
- [ ] Test on single monitor (HiDPI/Retina)
- [ ] Test on dual monitors (same DPI)
- [ ] Test on dual monitors (different DPI)
- [ ] Test on triple monitor setup
- [ ] Test window drag between displays
- [ ] Test fullscreen on secondary display
- [ ] Test with displays in different arrangements (horizontal, vertical, L-shape)
- [ ] Test with portrait orientation displays
- [ ] Test display hotplug (connecting/disconnecting displays)

### Automated Testing

```rust
#[test]
fn test_multi_monitor_window_position() {
    let platform = current_platform().unwrap();
    let displays = platform.displays();
    
    if displays.len() < 2 {
        // Skip test on single-display systems
        return;
    }
    
    // Test window positioning on each display
    for display in displays.iter() {
        let window = create_test_window(&platform).unwrap();
        
        // Position on this display
        let bounds = display.bounds();
        let scale = display.scale_factor() as f32;
        let logical_x = bounds.origin.x.0 as f32 / scale;
        let logical_y = bounds.origin.y.0 as f32 / scale;
        
        window.set_logical_position(Point::new(px(logical_x), px(logical_y)));
        
        // Verify window is on correct display
        let window_pos = window.physical_position();
        assert!(bounds.contains_point(window_pos));
    }
}
```

### Simulation Testing

```rust
// Simulate DPI change for testing without physical multi-monitor setup
#[cfg(test)]
mod tests {
    fn simulate_scale_factor_change(window: &mut MockWindow, new_scale: f64) {
        window.trigger_event(WindowEvent::ScaleFactorChanged {
            scale_factor: new_scale,
            window_id: window.id(),
        });
    }
}
```

## Platform-Specific Considerations

### Windows

- **Per-Monitor DPI v2**: Windows 10+ supports per-monitor DPI awareness
- **DPI_AWARENESS_CONTEXT**: Set to `PER_MONITOR_AWARE_V2` in manifest
- **EnumDisplayMonitors**: Used for display enumeration
- **GetMonitorInfo**: Provides usable work area (excludes taskbar)
- **WM_DPICHANGED**: Sent when window moves between different DPI displays

**Manifest Example**:
```xml
<application xmlns="urn:schemas-microsoft-com:asm.v3">
  <windowsSettings>
    <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
    <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">permonitorv2</dpiAwareness>
  </windowsSettings>
</application>
```

### macOS

- **Retina Displays**: Scale factor is typically 2.0
- **NSScreen**: Used for display enumeration
- **Menu Bar**: Excluded from usable bounds (typically 25-44 pixels)
- **Spaces**: Each display can have its own spaces (virtual desktops)
- **Notification Center**: Additional area exclusion on some displays

**Special Considerations**:
- macOS coordinates start from bottom-left (flipped Y-axis)
- Menu bar is always on the primary display
- Dock can be on any display edge

### Linux (X11/Wayland)

- **X11**: Uses RandR extension for display enumeration
- **Wayland**: Uses `wl_output` protocol
- **Different compositors**: Handle multi-monitor differently (GNOME, KDE, etc.)
- **HiDPI**: `GDK_SCALE` and `GDK_DPI_SCALE` environment variables

## Common Pitfalls

### ❌ Don't: Assume Single Display

```rust
// BAD: Assumes only one display
let screen_size = displays[0].bounds().size;
```

```rust
// GOOD: Use primary display explicitly
let primary = displays.iter().find(|d| d.is_primary()).unwrap();
let screen_size = primary.bounds().size;
```

### ❌ Don't: Hardcode DPI Values

```rust
// BAD: Assumes 96 DPI
let font_size_pixels = 16;
```

```rust
// GOOD: Use logical points
let font_size = px(12.0); // Will scale automatically
```

### ❌ Don't: Ignore ScaleFactorChanged

```rust
// BAD: Only handle initial scale factor
let scale = window.scale_factor();
// ... never update it
```

```rust
// GOOD: Handle scale changes
platform.on_window_event(Box::new(|event| {
    if let WindowEvent::ScaleFactorChanged { scale_factor, .. } = event {
        update_scale_factor(scale_factor);
    }
}));
```

### ❌ Don't: Save Device Pixel Positions

```rust
// BAD: Saving physical pixels
config.save("window_x", window.physical_position().x.0);
```

```rust
// GOOD: Save logical coordinates
config.save("window_x", window.logical_position().x.0);
```

## Resources

- [Windows DPI Documentation](https://docs.microsoft.com/en-us/windows/win32/hidpi/high-dpi-desktop-application-development-on-windows)
- [macOS Retina Display Guide](https://developer.apple.com/design/human-interface-guidelines/macos/visual-design/display/)
- [Linux HiDPI Wiki](https://wiki.archlinux.org/title/HiDPI)
- [flui-platform API Documentation](https://docs.rs/flui-platform)

## Example Application

See `crates/flui-platform/examples/displays.rs` for a complete working example of display enumeration and multi-monitor handling.

```bash
# Run the example
cargo run -p flui-platform --example displays
```
