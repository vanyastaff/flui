//! Input event types for cross-platform support
//!
//! This module provides a unified input event system that works across
//! all platforms: Desktop (Windows/macOS/Linux), Mobile (iOS/Android),
//! and Web.
//!
//! # Design Philosophy
//!
//! 1. **Pointer Events** - Unified API for mouse, touch, and pen
//! 2. **Platform Agnostic** - Same types work on desktop and mobile
//! 3. **Type Safe** - Rust enums prevent invalid states
//! 4. **GPUI-inspired** - Proven patterns from production UI framework
//!
//! # Input Sources
//!
//! - **Mouse**: Desktop pointer device (buttons, wheel, movement)
//! - **Touch**: Mobile/tablet touchscreen (multi-touch support)
//! - **Pen/Stylus**: Precision input with pressure and tilt
//! - **Keyboard**: Text input and shortcuts
//! - **Gamepad**: Game controllers (future)

use flui_types::geometry::{Pixels, Point};
use std::path::PathBuf;
use std::time::Instant;

// ============================================================================
// Pointer Events (Mouse, Touch, Pen)
// ============================================================================

/// Unified pointer event for mouse, touch, and pen input
///
/// This abstraction allows the same event handling code to work across
/// desktop (mouse), mobile (touch), and tablet (pen) platforms.
///
/// # Gesture Recognition
///
/// This event contains all data needed for gesture recognizers:
/// - `timestamp`: For velocity calculation and timeout detection
/// - `pointer_id`: Unique ID to track pointer across phases
/// - `delta`: Movement since last event (for drag gestures)
/// - `position`: Current position (for hit testing)
/// - `pressure`/`tilt`: For advanced pen gestures
///
/// # Example
///
/// ```rust,ignore
/// match pointer_event.kind {
///     PointerKind::Mouse(MouseButton::Left) => { /* handle click */ }
///     PointerKind::Touch { id: 0 } => { /* handle primary touch */ }
///     PointerKind::Pen => { /* handle stylus */ }
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct PointerEvent {
    /// Unique pointer ID (stable across down/move/up phases)
    ///
    /// For multi-touch, each finger gets a unique ID.
    /// For mouse, this is typically 0.
    pub pointer_id: u64,

    /// Device ID (to distinguish multiple mice/touchscreens)
    pub device_id: u32,

    /// The kind of pointer that generated this event
    pub kind: PointerKind,

    /// Position in logical pixels
    pub position: Point<Pixels>,

    /// Movement delta since last event (logical pixels)
    ///
    /// This is crucial for drag gesture recognition.
    pub delta: Point<Pixels>,

    /// Modifiers held during event
    pub modifiers: Modifiers,

    /// Pointer phase (down, move, up, cancel)
    pub phase: PointerPhase,

    /// Timestamp when event occurred
    ///
    /// Used for velocity calculation and timeout detection in gestures.
    pub timestamp: Instant,

    /// Click count (for double-click/tap detection)
    pub click_count: usize,

    /// Pressure (0.0 - 1.0), relevant for pen/touch
    ///
    /// `None` if device doesn't support pressure.
    /// For touch: typically 1.0 or actual finger pressure.
    /// For pen: varies based on stylus pressure.
    pub pressure: Option<f32>,

    /// Tilt angles in degrees (relevant for pen)
    ///
    /// `None` if device doesn't support tilt.
    pub tilt: Option<PointerTilt>,
}

/// The kind of pointer device
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerKind {
    /// Mouse pointer
    Mouse(MouseButton),

    /// Touch pointer (finger)
    Touch {
        /// Touch ID for multi-touch tracking
        id: u64,
    },

    /// Pen/stylus pointer
    Pen,
}

/// Mouse button
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MouseButton {
    /// Left mouse button (primary)
    #[default]
    Left,

    /// Right mouse button (secondary/context menu)
    Right,

    /// Middle mouse button (wheel click)
    Middle,

    /// Back navigation button
    Back,

    /// Forward navigation button
    Forward,

    /// Other/unknown button
    Other(u16),
}

/// Pointer event phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerPhase {
    /// Pointer was pressed down
    Down,

    /// Pointer moved while pressed
    Move,

    /// Pointer was released
    Up,

    /// Pointer event was cancelled (system interruption)
    Cancel,
}

/// Pen tilt information
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerTilt {
    /// Tilt along X axis in degrees (-90 to +90)
    pub x: f32,

    /// Tilt along Y axis in degrees (-90 to +90)
    pub y: f32,
}

/// Mouse wheel/scroll event
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollWheelEvent {
    /// Position where scroll occurred
    pub position: Point<Pixels>,

    /// Scroll delta (logical pixels)
    pub delta: ScrollDelta,

    /// Modifiers held during scroll
    pub modifiers: Modifiers,

    /// Touch phase (for trackpad momentum scrolling)
    pub phase: ScrollPhase,
}

/// Scroll delta type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// Pixel-based scrolling (touchpad, smooth wheel)
    Pixels { x: f32, y: f32 },

    /// Line-based scrolling (old-style mouse wheels)
    Lines { x: f32, y: f32 },
}

/// Scroll phase (for momentum scrolling)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollPhase {
    /// Scroll started
    Started,

    /// Scrolling continues
    Changed,

    /// Scroll ended
    Ended,
}

// ============================================================================
// Keyboard Events
// ============================================================================

/// Key press event
#[derive(Debug, Clone, PartialEq)]
pub struct KeyDownEvent {
    /// The physical key code
    pub key_code: KeyCode,

    /// The logical key (after keyboard layout)
    pub logical_key: LogicalKey,

    /// Text produced by this key (if any)
    pub text: Option<String>,

    /// Modifiers held during key press
    pub modifiers: Modifiers,

    /// Is this a key repeat event?
    pub is_repeat: bool,
}

/// Key release event
#[derive(Debug, Clone, PartialEq)]
pub struct KeyUpEvent {
    /// The physical key code
    pub key_code: KeyCode,

    /// The logical key (after keyboard layout)
    pub logical_key: LogicalKey,

    /// Modifiers held during key release
    pub modifiers: Modifiers,
}

/// Physical key code (position on keyboard)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KeyCode {
    // Letters
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ,
    KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT,
    KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,

    // Numbers
    Digit0, Digit1, Digit2, Digit3, Digit4,
    Digit5, Digit6, Digit7, Digit8, Digit9,

    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,

    // Navigation
    ArrowLeft, ArrowRight, ArrowUp, ArrowDown,
    Home, End, PageUp, PageDown,

    // Editing
    Backspace, Delete, Enter, Tab, Space, Escape,

    // Modifiers
    ShiftLeft, ShiftRight,
    ControlLeft, ControlRight,
    AltLeft, AltRight,
    MetaLeft, MetaRight, // Command on Mac, Windows key on Windows

    // Other
    CapsLock, NumLock, ScrollLock,
    PrintScreen, Pause, Insert,

    /// Unknown/unmapped key
    Unknown,
}

/// Logical key (after keyboard layout applied)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LogicalKey {
    /// A character key
    Character(String),

    /// A named key
    Named(NamedKey),
}

/// Named logical keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NamedKey {
    Enter, Tab, Space, Backspace, Delete, Escape,
    ArrowLeft, ArrowRight, ArrowUp, ArrowDown,
    Home, End, PageUp, PageDown,
    Insert,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
}

/// Modifier keys state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Shift key
    pub shift: bool,

    /// Control key (Ctrl on Windows/Linux, Command on macOS)
    pub control: bool,

    /// Alt key (Option on macOS)
    pub alt: bool,

    /// Meta/Super key (Command on macOS, Windows key on Windows)
    pub meta: bool,
}

/// Modifiers changed event
#[derive(Debug, Clone, PartialEq)]
pub struct ModifiersChangedEvent {
    /// New modifiers state
    pub modifiers: Modifiers,
}

// ============================================================================
// File Drop Events
// ============================================================================

/// File drag-and-drop event
#[derive(Debug, Clone, PartialEq)]
pub struct FileDropEvent {
    /// Position where files were dropped
    pub position: Point<Pixels>,

    /// Files that were dropped
    pub paths: Vec<PathBuf>,

    /// Drop phase
    pub phase: FileDropPhase,
}

/// File drop phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileDropPhase {
    /// Files are being dragged over the window
    Hover,

    /// Files were dropped
    Dropped,

    /// Drag operation was cancelled
    Cancelled,
}

// ============================================================================
// Platform Input Enum (GPUI-style)
// ============================================================================

/// Unified platform input event
///
/// This enum wraps all input event types and is used for event dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum PlatformInput {
    /// Pointer event (mouse, touch, pen)
    Pointer(PointerEvent),

    /// Mouse wheel/scroll event
    ScrollWheel(ScrollWheelEvent),

    /// Key press event
    KeyDown(KeyDownEvent),

    /// Key release event
    KeyUp(KeyUpEvent),

    /// Modifiers changed
    ModifiersChanged(ModifiersChangedEvent),

    /// File drop event
    FileDrop(FileDropEvent),
}

// ============================================================================
// Traits
// ============================================================================

/// Trait for input events
pub trait InputEvent: Send + Sync + 'static {
    /// Convert to platform input enum
    fn to_platform_input(self) -> PlatformInput;
}

impl InputEvent for PointerEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::Pointer(self)
    }
}

impl InputEvent for ScrollWheelEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::ScrollWheel(self)
    }
}

impl InputEvent for KeyDownEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::KeyDown(self)
    }
}

impl InputEvent for KeyUpEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::KeyUp(self)
    }
}

impl InputEvent for ModifiersChangedEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::ModifiersChanged(self)
    }
}

impl InputEvent for FileDropEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::FileDrop(self)
    }
}

// ============================================================================
// Helper implementations
// ============================================================================

impl Modifiers {
    /// Check if any modifier is pressed
    pub fn any(&self) -> bool {
        self.shift || self.control || self.alt || self.meta
    }

    /// Check if only Shift is pressed
    pub fn only_shift(&self) -> bool {
        self.shift && !self.control && !self.alt && !self.meta
    }

    /// Check if only Control is pressed
    pub fn only_control(&self) -> bool {
        !self.shift && self.control && !self.alt && !self.meta
    }

    /// Check if only Alt is pressed
    pub fn only_alt(&self) -> bool {
        !self.shift && !self.control && self.alt && !self.meta
    }

    /// Check if only Meta is pressed
    pub fn only_meta(&self) -> bool {
        !self.shift && !self.control && !self.alt && self.meta
    }
}

impl PointerEvent {
    /// Check if this is a primary pointer event (left mouse button or first touch)
    pub fn is_primary(&self) -> bool {
        matches!(
            self.kind,
            PointerKind::Mouse(MouseButton::Left) | PointerKind::Touch { id: 0 }
        )
    }

    /// Check if this event should trigger focus
    pub fn is_focusing(&self) -> bool {
        self.is_primary() && self.phase == PointerPhase::Down
    }

    /// Get distance from another pointer position (for gesture recognition)
    pub fn distance_to(&self, other: Point<Pixels>) -> f32 {
        let dx = self.position.x.0 - other.x.0;
        let dy = self.position.y.0 - other.y.0;
        (dx * dx + dy * dy).sqrt()
    }

    /// Check if pointer moved significantly (for drag gesture detection)
    ///
    /// Uses platform-specific thresholds (typically 8-10 pixels)
    pub fn moved_significantly(&self, threshold: f32) -> bool {
        self.delta.x.0.abs() > threshold || self.delta.y.0.abs() > threshold
    }

    /// Check if this is a down event
    pub fn is_down(&self) -> bool {
        self.phase == PointerPhase::Down
    }

    /// Check if this is a move event
    pub fn is_move(&self) -> bool {
        self.phase == PointerPhase::Move
    }

    /// Check if this is an up event
    pub fn is_up(&self) -> bool {
        self.phase == PointerPhase::Up
    }

    /// Check if this is a cancel event
    pub fn is_cancel(&self) -> bool {
        self.phase == PointerPhase::Cancel
    }
}

/// Velocity tracker for gesture recognition
///
/// Tracks pointer velocity to detect fling/swipe gestures.
/// Based on Flutter's VelocityTracker.
#[derive(Debug, Clone)]
pub struct VelocityTracker {
    samples: Vec<VelocitySample>,
    max_samples: usize,
}

#[derive(Debug, Clone, Copy)]
struct VelocitySample {
    timestamp: Instant,
    position: Point<Pixels>,
}

impl VelocityTracker {
    /// Create a new velocity tracker
    pub fn new() -> Self {
        Self {
            samples: Vec::with_capacity(20),
            max_samples: 20,
        }
    }

    /// Add a pointer event sample
    pub fn add_sample(&mut self, event: &PointerEvent) {
        self.samples.push(VelocitySample {
            timestamp: event.timestamp,
            position: event.position,
        });

        // Keep only recent samples
        if self.samples.len() > self.max_samples {
            self.samples.remove(0);
        }
    }

    /// Calculate current velocity in pixels per second
    ///
    /// Returns None if insufficient data for calculation.
    pub fn velocity(&self) -> Option<Velocity> {
        if self.samples.len() < 2 {
            return None;
        }

        let first = self.samples.first()?;
        let last = self.samples.last()?;

        let dt = last.timestamp.duration_since(first.timestamp);
        if dt.as_secs_f32() < 0.001 {
            return None; // Too short time span
        }

        let dx = last.position.x.0 - first.position.x.0;
        let dy = last.position.y.0 - first.position.y.0;

        let dt_secs = dt.as_secs_f32();

        Some(Velocity {
            x: dx / dt_secs,
            y: dy / dt_secs,
        })
    }

    /// Clear all samples
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

impl Default for VelocityTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Velocity in pixels per second
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Velocity {
    /// Horizontal velocity (pixels/second)
    pub x: f32,

    /// Vertical velocity (pixels/second)
    pub y: f32,
}

impl Velocity {
    /// Get velocity magnitude (speed)
    pub fn magnitude(&self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Check if velocity exceeds threshold (for fling detection)
    pub fn is_fling(&self, threshold: f32) -> bool {
        self.magnitude() > threshold
    }

    /// Get velocity direction in radians
    pub fn direction(&self) -> f32 {
        self.y.atan2(self.x)
    }
}
