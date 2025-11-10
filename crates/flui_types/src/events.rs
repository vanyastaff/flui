//! Event types for pointer and gesture handling
//!
//! This module provides event types for user interactions like mouse clicks,
//! touches, and gestures. Based on Flutter's pointer event system.

use crate::{Offset, Size};

/// Device type that generated the pointer event
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum PointerDeviceKind {
    /// Mouse pointer
    Mouse,
    /// Touch screen
    Touch,
    /// Stylus/pen
    Stylus,
    /// Trackpad
    Trackpad,
    /// Unknown device
    Unknown,
}

/// Mouse button that was pressed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum PointerButton {
    /// Primary button (usually left mouse button)
    Primary,
    /// Secondary button (usually right mouse button)
    Secondary,
    /// Middle button
    Middle,
    /// Additional button
    Other(u8),
}

/// Base pointer event data
///
/// Contains common fields for all pointer events
#[derive(Debug, Clone)]
pub struct PointerEventData {
    /// Position in global coordinates
    pub position: Offset,
    /// Position in local widget coordinates (set during hit testing)
    pub local_position: Offset,
    /// Device that generated the event
    pub device_kind: PointerDeviceKind,
    /// Pointer device ID
    pub device: i32,
    /// Button that was pressed (for down/up events)
    pub button: Option<PointerButton>,
    /// Buttons currently pressed
    pub buttons: u8,
}

impl PointerEventData {
    /// Create new pointer event data
    pub fn new(position: Offset, device_kind: PointerDeviceKind) -> Self {
        Self {
            position,
            local_position: position,
            device_kind,
            device: 0,
            button: None,
            buttons: 0,
        }
    }

    /// Create with button
    pub fn with_button(mut self, button: PointerButton) -> Self {
        self.button = Some(button);
        self
    }
}

/// Pointer event types
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum PointerEvent {
    /// Pointer pressed down
    Down(PointerEventData),
    /// Pointer released
    Up(PointerEventData),
    /// Pointer moved
    Move(PointerEventData),
    /// Pointer entered widget bounds
    Enter(PointerEventData),
    /// Pointer exited widget bounds
    Exit(PointerEventData),
    /// Event cancelled
    Cancel(PointerEventData),
}

impl PointerEvent {
    /// Get the event data
    pub fn data(&self) -> &PointerEventData {
        match self {
            PointerEvent::Down(data) => data,
            PointerEvent::Up(data) => data,
            PointerEvent::Move(data) => data,
            PointerEvent::Enter(data) => data,
            PointerEvent::Exit(data) => data,
            PointerEvent::Cancel(data) => data,
        }
    }

    /// Get mutable event data
    pub fn data_mut(&mut self) -> &mut PointerEventData {
        match self {
            PointerEvent::Down(data) => data,
            PointerEvent::Up(data) => data,
            PointerEvent::Move(data) => data,
            PointerEvent::Enter(data) => data,
            PointerEvent::Exit(data) => data,
            PointerEvent::Cancel(data) => data,
        }
    }

    /// Get position in global coordinates
    pub fn position(&self) -> Offset {
        self.data().position
    }

    /// Get position in local widget coordinates
    pub fn local_position(&self) -> Offset {
        self.data().local_position
    }

    /// Set local position (used during hit testing)
    pub fn set_local_position(&mut self, position: Offset) {
        self.data_mut().local_position = position;
    }
}

/// Type for pointer event handlers
pub type PointerEventHandler = std::sync::Arc<dyn Fn(&PointerEvent) + Send + Sync>;

/// Hit test result entry
///
/// Represents a widget that was hit during hit testing
#[derive(Clone)]
pub struct HitTestEntry {
    /// Position where the hit occurred in local coordinates
    pub local_position: Offset,
    /// Bounds of the widget that was hit
    pub bounds: Size,
    /// Optional event handler (for RenderPointerListener)
    pub handler: Option<PointerEventHandler>,
}

impl std::fmt::Debug for HitTestEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitTestEntry")
            .field("local_position", &self.local_position)
            .field("bounds", &self.bounds)
            .field("has_handler", &self.handler.is_some())
            .finish()
    }
}

impl HitTestEntry {
    /// Create a new hit test entry without handler
    pub fn new(local_position: Offset, bounds: Size) -> Self {
        Self {
            local_position,
            bounds,
            handler: None,
        }
    }

    /// Create a new hit test entry with handler
    pub fn with_handler(
        local_position: Offset,
        bounds: Size,
        handler: PointerEventHandler,
    ) -> Self {
        Self {
            local_position,
            bounds,
            handler: Some(handler),
        }
    }

    /// Dispatch event to handler if present
    pub fn dispatch(&self, event: &PointerEvent) {
        if let Some(handler) = &self.handler {
            handler(event);
        }
    }
}

/// Hit test result
///
/// Contains all widgets that were hit during hit testing,
/// ordered from front to back
#[derive(Debug, Clone, Default)]
pub struct HitTestResult {
    /// Stack of hit entries (front to back)
    entries: Vec<HitTestEntry>,
}

impl HitTestResult {
    /// Create a new empty hit test result
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add an entry to the result
    pub fn add(&mut self, entry: HitTestEntry) {
        self.entries.push(entry);
    }

    /// Get all entries
    pub fn entries(&self) -> &[HitTestEntry] {
        &self.entries
    }

    /// Check if any widget was hit
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Check if any widget was hit (opposite of is_empty)
    pub fn is_hit(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Get the top-most (front) entry
    pub fn front(&self) -> Option<&HitTestEntry> {
        self.entries.first()
    }

    /// Dispatch event to all hit entries
    ///
    /// Calls handler on each entry that has one, in order from front to back
    pub fn dispatch(&self, event: &PointerEvent) {
        for entry in &self.entries {
            entry.dispatch(event);
        }
    }
}

/// Physical key on the keyboard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum PhysicalKey {
    /// Letter keys
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,

    /// Number keys
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,

    /// Function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,

    /// Navigation keys
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,

    /// Editing keys
    Backspace,
    Delete,
    Insert,
    Enter,
    Tab,
    Escape,
    Space,

    /// Modifier keys
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    MetaLeft,
    MetaRight, // Windows/Command key

    /// Other common keys
    CapsLock,
    NumLock,
    ScrollLock,
    PrintScreen,
    Pause,

    /// Unidentified key
    Unidentified,
}

/// Logical key - the meaning of the key press
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum LogicalKey {
    /// Character input (e.g., 'a', 'A', '1', '!')
    Character(String),

    /// Named key (non-character keys)
    Named(PhysicalKey),
}

/// Keyboard modifiers state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    /// Shift key is pressed
    pub shift: bool,
    /// Control key is pressed
    pub control: bool,
    /// Alt/Option key is pressed
    pub alt: bool,
    /// Meta/Command/Windows key is pressed
    pub meta: bool,
}

impl KeyModifiers {
    /// Create new modifiers with all keys released
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if no modifiers are pressed
    pub fn is_empty(&self) -> bool {
        !self.shift && !self.control && !self.alt && !self.meta
    }
}

/// Keyboard event data
#[derive(Debug, Clone)]
pub struct KeyEventData {
    /// Physical key that was pressed
    pub physical_key: PhysicalKey,

    /// Logical key (what the key means)
    pub logical_key: LogicalKey,

    /// Text produced by the key press (if any)
    pub text: Option<String>,

    /// Current modifier keys state
    pub modifiers: KeyModifiers,

    /// Whether this is a repeat event (key held down)
    pub repeat: bool,
}

impl KeyEventData {
    /// Create new keyboard event data
    pub fn new(physical_key: PhysicalKey, logical_key: LogicalKey) -> Self {
        Self {
            physical_key,
            logical_key,
            text: None,
            modifiers: KeyModifiers::new(),
            repeat: false,
        }
    }

    /// Set the text produced by this key press
    pub fn with_text(mut self, text: String) -> Self {
        self.text = Some(text);
        self
    }

    /// Set the modifiers state
    pub fn with_modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }
}

/// Keyboard event types
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum KeyEvent {
    /// Key was pressed down
    Down(KeyEventData),

    /// Key was released
    Up(KeyEventData),
}

impl KeyEvent {
    /// Get the event data
    pub fn data(&self) -> &KeyEventData {
        match self {
            KeyEvent::Down(data) => data,
            KeyEvent::Up(data) => data,
        }
    }

    /// Get mutable event data
    pub fn data_mut(&mut self) -> &mut KeyEventData {
        match self {
            KeyEvent::Down(data) => data,
            KeyEvent::Up(data) => data,
        }
    }

    /// Get the physical key
    pub fn physical_key(&self) -> PhysicalKey {
        self.data().physical_key
    }

    /// Get the logical key
    pub fn logical_key(&self) -> &LogicalKey {
        &self.data().logical_key
    }

    /// Get the text produced by this key press
    pub fn text(&self) -> Option<&str> {
        self.data().text.as_deref()
    }
}

/// Scroll delta
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(missing_docs)]
pub enum ScrollDelta {
    /// Scroll by lines (e.g., mouse wheel clicks)
    Lines { x: f32, y: f32 },

    /// Scroll by pixels (e.g., touchpad)
    Pixels { x: f32, y: f32 },
}

/// Scroll event data
#[derive(Debug, Clone)]
pub struct ScrollEventData {
    /// Position where scroll occurred
    pub position: Offset,

    /// Scroll delta
    pub delta: ScrollDelta,

    /// Current modifier keys state
    pub modifiers: KeyModifiers,
}

impl ScrollEventData {
    /// Create new scroll event data
    pub fn new(position: Offset, delta: ScrollDelta) -> Self {
        Self {
            position,
            delta,
            modifiers: KeyModifiers::new(),
        }
    }
}

/// System theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Theme {
    /// Light theme
    Light,
    /// Dark theme
    Dark,
}

impl Theme {
    /// Check if this is the dark theme
    pub fn is_dark(&self) -> bool {
        matches!(self, Theme::Dark)
    }

    /// Check if this is the light theme
    pub fn is_light(&self) -> bool {
        matches!(self, Theme::Light)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Light
    }
}

/// Window event types
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(missing_docs)]
pub enum WindowEvent {
    /// Window was resized
    Resized { width: u32, height: u32 },

    /// Window gained or lost focus
    FocusChanged { focused: bool },

    /// Window visibility changed (minimized/restored)
    VisibilityChanged { visible: bool },

    /// Window close was requested
    CloseRequested,

    /// Window scale factor changed (DPI change)
    ScaleChanged { scale: f64 },

    /// System theme changed (dark/light mode)
    ThemeChanged { theme: Theme },
}

/// Unified event type
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum Event {
    /// Pointer event (mouse, touch, etc.)
    Pointer(PointerEvent),

    /// Keyboard event
    Key(KeyEvent),

    /// Scroll event (mouse wheel, touchpad)
    Scroll(ScrollEventData),

    /// Window event
    Window(WindowEvent),
}

impl Event {
    /// Create a pointer event
    pub fn pointer(event: PointerEvent) -> Self {
        Event::Pointer(event)
    }

    /// Create a keyboard event
    pub fn key(event: KeyEvent) -> Self {
        Event::Key(event)
    }

    /// Create a scroll event
    pub fn scroll(data: ScrollEventData) -> Self {
        Event::Scroll(data)
    }

    /// Create a window event
    pub fn window(event: WindowEvent) -> Self {
        Event::Window(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_event_data() {
        let data = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        assert_eq!(data.position, Offset::new(10.0, 20.0));
        assert_eq!(data.device_kind, PointerDeviceKind::Mouse);
    }

    #[test]
    fn test_pointer_event() {
        let data = PointerEventData::new(Offset::new(5.0, 10.0), PointerDeviceKind::Touch);
        let event = PointerEvent::Down(data);

        assert_eq!(event.position(), Offset::new(5.0, 10.0));
    }

    #[test]
    fn test_hit_test_result() {
        let mut result = HitTestResult::new();
        assert!(result.is_empty());

        result.add(HitTestEntry::new(
            Offset::new(1.0, 2.0),
            Size::new(100.0, 50.0),
        ));

        assert!(!result.is_empty());
        assert_eq!(result.entries().len(), 1);
    }

    #[test]
    fn test_key_event() {
        let data = KeyEventData::new(PhysicalKey::KeyA, LogicalKey::Character("a".to_string()));
        let event = KeyEvent::Down(data);

        assert_eq!(event.physical_key(), PhysicalKey::KeyA);
    }

    #[test]
    fn test_key_modifiers() {
        let modifiers = KeyModifiers {
            shift: true,
            control: false,
            alt: false,
            meta: false,
        };

        assert!(!modifiers.is_empty());
    }

    #[test]
    fn test_unified_event() {
        let pointer_data = PointerEventData::new(Offset::new(10.0, 20.0), PointerDeviceKind::Mouse);
        let event = Event::pointer(PointerEvent::Down(pointer_data));

        assert!(matches!(event, Event::Pointer(_)));
    }
}
