# Rendering Layer

The rendering layer provides Flutter-like rendering capabilities that egui doesn't support natively. This layer sits between widgets and painters, handling accessibility, semantics, text selection, and mouse tracking.

## Architecture

Similar to Flutter's rendering library, this module bridges the gap between high-level widgets and low-level painting:

```
Widgets (high-level)
    ↓
Rendering (this layer) ← Accessibility, Semantics, Selection
    ↓
Painters + egui (low-level)
```

## Modules

### `accessibility`

Provides accessibility features for users with disabilities:

```rust
use nebula_ui::rendering::{AccessibilityFeatures, AccessibilityPreferences};

// Platform accessibility settings
let features = AccessibilityFeatures {
    screen_reader: true,
    bold_text: true,
    high_contrast: true,
    disable_animations: true,
    text_scale_factor: 1.5,
    ..Default::default()
};

// Check if animations should be disabled
if !features.should_show_animations() {
    // Skip animations
}

// Get effective text scale
let scale = features.effective_text_scale(); // Clamped to 0.5-3.0

// Preset configurations
let screen_reader_prefs = AccessibilityPreferences::for_screen_reader();
let motion_sensitive = AccessibilityPreferences::for_motion_sensitivity();
let visual_impaired = AccessibilityPreferences::for_visual_impairment();
```

**Features:**
- `bold_text` - Make text bolder for readability
- `high_contrast` - High contrast UI
- `disable_animations` / `reduce_motion` - Reduce or disable animations
- `invert_colors` - Invert colors for light sensitivity
- `screen_reader` - Screen reader enabled
- `text_scale_factor` - Platform text scaling

**Preferences:**
- `min_touch_target_size` - Minimum touch target size (default: 48dp)
- `keyboard_navigation` - Enable keyboard navigation
- `announce_changes` - Announce UI changes to screen readers
- `focus_indicator_strength` - Focus indicator visibility (0.0-1.0)

### `semantics`

Semantic annotations for screen readers and accessibility tools:

```rust
use nebula_ui::rendering::{
    SemanticsNode, SemanticsData, SemanticsAction, SemanticsFlag,
};

// Create semantic data for a button
let button_data = SemanticsData::button("Submit Form")
    .with_hint("Press to submit the form")
    .with_flag(SemanticsFlag::IsFocused);

// Create semantic data for a text field
let field_data = SemanticsData::text_field(
    Some("Email".to_string()),
    Some("user@example.com".to_string()),
);

// Create semantic tree
let mut root = SemanticsNode::new(SemanticsData::labeled("Form"));
let submit_button = SemanticsNode::new(button_data);
let email_field = SemanticsNode::new(field_data);

root.add_child(email_field);
root.add_child(submit_button);

// Get announcement for screen readers
let announcement = button_data.announcement();
// Returns: "Submit Form, Press to submit the form"

// Find nodes by action
let tappable_ids = root.find_by_action(SemanticsAction::Tap);
```

**Semantic Actions:**
- `Tap`, `LongPress` - Interaction actions
- `ScrollLeft`, `ScrollRight`, `ScrollUp`, `ScrollDown` - Scrolling
- `Increase`, `Decrease` - Value adjustments
- `Copy`, `Cut`, `Paste` - Clipboard operations
- `SetSelection` - Text selection
- `Dismiss` - Dismiss dialogs/menus

**Semantic Flags:**
- `IsButton`, `IsTextField`, `IsImage`, `IsLink`, `IsHeader`
- `IsChecked`, `IsSelected`, `IsFocused`
- `IsObscured` - For password fields
- `IsReadOnly`, `IsDisabled`, `IsHidden`
- `IsLiveRegion` - Announces changes automatically

### `text_selection`

Text selection and cursor management:

```rust
use nebula_ui::rendering::{TextSelection, TextAffinity};

// Create a collapsed selection (cursor) at position 5
let cursor = TextSelection::collapsed(5);
assert!(cursor.is_collapsed());

// Create a text selection from 5 to 10
let selection = TextSelection::new(5, 10);
assert_eq!(selection.length(), 5);

// Select all text
let all = TextSelection::all_text(text.len());

// Move selection
let moved = selection.move_by(3); // Move forward by 3 characters
let moved_back = selection.move_by(-2); // Move backward by 2

// Expand selection
let expanded = selection.expand_by(5); // Extend extent by 5 chars

// Collapse to start/end
let at_start = selection.collapse_to_start();
let at_end = selection.collapse_to_end();

// Extract selected text
let text = "Hello, world!";
let selected = selection.extract_from(text); // "Hello"

// Handle UTF-8 properly
let emoji_text = "Hello 🌍 world!";
let sel = TextSelection::new(6, 10);
let extracted = sel.extract_from(emoji_text); // Includes emoji
```

**Selection Handle Types:**
- `Left` - Handle at the start of selection
- `Right` - Handle at the end of selection
- `Collapsed` - Single cursor handle

**Text Affinity:**
- `Upstream` - Cursor affinity for upstream characters
- `Downstream` - Cursor affinity for downstream characters (default)

### `mouse_tracker`

Mouse/pointer tracking and cursor management:

```rust
use nebula_ui::rendering::{
    MouseTracker, MouseTrackerAnnotation, MouseCursor,
};
use nebula_ui::types::core::{Offset, Rect};

// Create a mouse tracker
let mut tracker = MouseTracker::new();

// Register a clickable region
let rect = Rect::from_min_max(
    egui::pos2(10.0, 10.0),
    egui::pos2(100.0, 50.0),
);

let id = tracker.next_id();
let button_region = MouseTrackerAnnotation::new(id, rect)
    .with_cursor(MouseCursor::Hand);

tracker.register_region(button_region);

// Update mouse position
tracker.update_position(Offset::new(50.0, 30.0));

// Get current cursor
let cursor = tracker.current_cursor(); // MouseCursor::Hand

// Convert to egui cursor
let egui_cursor = cursor.to_egui();
ui.ctx().set_cursor_icon(egui_cursor);

// Move outside region
tracker.update_position(Offset::new(200.0, 200.0));
assert_eq!(tracker.current_cursor(), MouseCursor::Default);
```

**Mouse Cursors:**
- `Default`, `Pointer`, `Hand` - Standard cursors
- `Text` - I-beam for text selection
- `Wait`, `Help`, `Crosshair` - Special purpose
- `Move` - 4-way arrows
- `Grab`, `Grabbing` - Drag operations
- `NoDrop`, `NotAllowed` - Forbidden actions
- `ResizeHorizontal`, `ResizeVertical` - Resizing
- `ResizeDiagonal1`, `ResizeDiagonal2` - Diagonal resizing
- `ResizeNorth`, `ResizeSouth`, `ResizeEast`, `ResizeWest` - Cardinal resizing
- `ResizeNorthEast`, `ResizeNorthWest`, `ResizeSouthEast`, `ResizeSouthWest` - Corner resizing
- `None` - Hidden cursor

**Mouse Events:**
- `Enter`, `Exit` - Mouse entering/leaving region
- `Hover` - Mouse moving within region
- `Down`, `Up` - Button press/release
- `Scroll` - Mouse wheel

**Mouse Buttons:**
- `Primary` - Usually left button
- `Secondary` - Usually right button
- `Middle` - Middle button
- `Back`, `Forward` - Navigation buttons

## Integration with Widgets

### Accessibility in Widgets

```rust
// In your widget implementation
impl Widget for MyWidget {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Check accessibility preferences
        let accessibility = ui.ctx().memory(|mem| {
            mem.data.get_temp::<AccessibilityFeatures>(egui::Id::null())
                .unwrap_or_default()
        });

        // Adjust rendering based on accessibility
        if accessibility.bold_text {
            // Use bold font
        }

        if accessibility.high_contrast {
            // Use high contrast colors
        }

        // Apply text scaling
        let font_size = base_size * accessibility.effective_text_scale();

        // ...
    }
}
```

### Semantics for Widgets

```rust
// Add semantic information to widgets
let response = ui.button("Submit");

// Attach semantic data
let semantics = SemanticsData::button("Submit")
    .with_hint("Submit the form")
    .with_flag(SemanticsFlag::IsFocused);

// Store in context for screen readers
ui.ctx().memory_mut(|mem| {
    mem.data.insert_temp(response.id, semantics);
});
```

### Text Selection in Widgets

```rust
// In a text editing widget
let mut selection = TextSelection::collapsed(cursor_pos);

if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
    selection = selection.move_by(1);
}

if ui.input(|i| i.modifiers.shift) {
    // Extend selection
    selection = selection.expand_by(1);
} else {
    // Move cursor
    selection = selection.collapse_to_end();
}

// Extract selected text
let selected_text = selection.extract_from(&text);
```

### Mouse Tracking in Widgets

```rust
// Use mouse tracker for custom cursor handling
let mut tracker = MouseTracker::new();

// Register interactive regions
let button_id = tracker.next_id();
let button_annotation = MouseTrackerAnnotation::new(button_id, button_rect)
    .with_cursor(MouseCursor::Hand);
tracker.register_region(button_annotation);

// Update on hover
if response.hovered() {
    tracker.update_position(response.hover_pos().unwrap().into());
    ui.ctx().set_cursor_icon(tracker.current_cursor().to_egui());
}
```

## Comparison with Flutter

| Feature | Flutter | Nebula-UI Rendering Layer |
|---------|---------|--------------------------|
| AccessibilityFeatures | ✅ | ✅ |
| SemanticsNode | ✅ | ✅ |
| TextSelection | ✅ | ✅ |
| MouseTracker | ✅ | ✅ |
| RenderObject | ✅ | ❌ (egui handles rendering) |
| CustomPainter | ✅ | ✅ (via painters module) |
| HitTesting | ✅ | ✅ (via egui) |

## Future Enhancements

Potential additions to the rendering layer:

1. **Platform Integration**
   - Native screen reader support (Windows Narrator, macOS VoiceOver)
   - System accessibility settings detection
   - Platform-specific optimizations

2. **Advanced Text Features**
   - Text input methods (IME)
   - Spell checking
   - Grammar checking
   - Text-to-speech integration

3. **Gesture Recognition**
   - Swipe gestures
   - Pinch-to-zoom
   - Rotation gestures
   - Multi-touch support

4. **Performance**
   - Semantic tree diffing
   - Incremental updates
   - Caching strategies

## Testing

All rendering modules include comprehensive tests:

```bash
# Run all rendering tests
cargo test --lib rendering

# Run specific module tests
cargo test --lib rendering::accessibility
cargo test --lib rendering::semantics
cargo test --lib rendering::text_selection
cargo test --lib rendering::mouse_tracker
```

Current test coverage:
- `accessibility`: 5 tests
- `semantics`: 8 tests
- `text_selection`: 10 tests
- `mouse_tracker`: 3 tests

Total: **545 tests passing** in nebula-ui (including rendering layer)

## Examples

See the examples directory for demonstrations:

- `examples/accessibility_demo.rs` - Accessibility features showcase
- `examples/semantics_demo.rs` - Screen reader support
- `examples/text_selection_demo.rs` - Text selection UI
- `examples/mouse_cursor_demo.rs` - Custom cursors

## References

- [Flutter Rendering Library](https://api.flutter.dev/flutter/rendering/rendering-library.html)
- [Flutter Accessibility](https://docs.flutter.dev/development/accessibility-and-localization/accessibility)
- [WCAG 2.1 Guidelines](https://www.w3.org/WAI/WCAG21/quickref/)
- [Material Design Accessibility](https://material.io/design/usability/accessibility.html)
