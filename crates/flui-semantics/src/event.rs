//! Semantics events for accessibility notifications.
//!
//! This module provides event types that render objects can send to
//! notify the accessibility system of changes or actions.

use rustc_hash::FxHashMap;
use smol_str::SmolStr;

// ============================================================================
// SemanticsEvent
// ============================================================================

/// An event that describes a semantic action or state change.
///
/// Semantics events are used to notify assistive technologies about
/// changes that don't necessarily result in a tree structure change.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsEvent` abstract class.
#[derive(Debug, Clone)]
pub struct SemanticsEvent {
    /// The type of event.
    event_type: SemanticsEventType,
    /// Additional data for the event.
    data: FxHashMap<SmolStr, SemanticsEventData>,
}

impl SemanticsEvent {
    /// Creates a new semantics event of the given type.
    pub fn new(event_type: SemanticsEventType) -> Self {
        Self {
            event_type,
            data: FxHashMap::default(),
        }
    }

    /// Creates a tap event.
    ///
    /// Sent when a semantics node has been tapped.
    pub fn tap() -> Self {
        Self::new(SemanticsEventType::Tap)
    }

    /// Creates a long press event.
    ///
    /// Sent when a semantics node has been long pressed.
    pub fn long_press() -> Self {
        Self::new(SemanticsEventType::LongPress)
    }

    /// Creates a tooltip event.
    ///
    /// Sent when a tooltip is shown.
    pub fn tooltip(message: impl Into<SmolStr>) -> Self {
        let mut event = Self::new(SemanticsEventType::Tooltip);
        event.set_string("message", message.into());
        event
    }

    /// Creates an announcement event.
    ///
    /// Used to announce a message to the user through assistive technology.
    pub fn announce(message: impl Into<SmolStr>) -> Self {
        let mut event = Self::new(SemanticsEventType::Announce);
        event.set_string("message", message.into());
        event
    }

    /// Creates a focus event.
    ///
    /// Sent when a semantics node gains accessibility focus.
    pub fn focus(node_id: u64) -> Self {
        let mut event = Self::new(SemanticsEventType::Focus);
        event.set_int("nodeId", node_id as i64);
        event
    }

    /// Creates a scroll event.
    ///
    /// Sent when a scrollable view has scrolled.
    pub fn scroll_completed() -> Self {
        Self::new(SemanticsEventType::ScrollCompleted)
    }

    /// Creates a hidden/shown children change event.
    ///
    /// Sent when the visibility of children has changed.
    pub fn hidden_children_changed() -> Self {
        Self::new(SemanticsEventType::HiddenChildrenChanged)
    }

    /// Returns the event type.
    #[inline]
    pub fn event_type(&self) -> SemanticsEventType {
        self.event_type
    }

    /// Sets a string value in the event data.
    pub fn set_string(&mut self, key: impl Into<SmolStr>, value: SmolStr) {
        self.data
            .insert(key.into(), SemanticsEventData::String(value));
    }

    /// Sets an integer value in the event data.
    pub fn set_int(&mut self, key: impl Into<SmolStr>, value: i64) {
        self.data.insert(key.into(), SemanticsEventData::Int(value));
    }

    /// Sets a float value in the event data.
    pub fn set_float(&mut self, key: impl Into<SmolStr>, value: f64) {
        self.data
            .insert(key.into(), SemanticsEventData::Float(value));
    }

    /// Sets a boolean value in the event data.
    pub fn set_bool(&mut self, key: impl Into<SmolStr>, value: bool) {
        self.data
            .insert(key.into(), SemanticsEventData::Bool(value));
    }

    /// Gets a string value from the event data.
    pub fn get_string(&self, key: &str) -> Option<&str> {
        match self.data.get(key) {
            Some(SemanticsEventData::String(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Gets an integer value from the event data.
    pub fn get_int(&self, key: &str) -> Option<i64> {
        match self.data.get(key) {
            Some(SemanticsEventData::Int(i)) => Some(*i),
            _ => None,
        }
    }

    /// Gets a float value from the event data.
    pub fn get_float(&self, key: &str) -> Option<f64> {
        match self.data.get(key) {
            Some(SemanticsEventData::Float(f)) => Some(*f),
            _ => None,
        }
    }

    /// Gets a boolean value from the event data.
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.data.get(key) {
            Some(SemanticsEventData::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    /// Returns the event data map.
    pub fn data(&self) -> &FxHashMap<SmolStr, SemanticsEventData> {
        &self.data
    }

    /// Converts this event to a map for serialization.
    pub fn to_map(&self) -> FxHashMap<SmolStr, SmolStr> {
        let mut map = FxHashMap::default();
        map.insert(
            SmolStr::from("type"),
            SmolStr::from(self.event_type.as_str()),
        );
        for (key, value) in &self.data {
            map.insert(key.clone(), SmolStr::from(value.to_string()));
        }
        map
    }
}

// ============================================================================
// SemanticsEventType
// ============================================================================

/// The type of semantics event.
///
/// # Flutter Equivalence
///
/// Corresponds to different subclasses of `SemanticsEvent` in Flutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticsEventType {
    /// A tap gesture was recognized.
    Tap,
    /// A long press gesture was recognized.
    LongPress,
    /// A tooltip was shown.
    Tooltip,
    /// An announcement should be made.
    Announce,
    /// A semantics node gained focus.
    Focus,
    /// A scroll action completed.
    ScrollCompleted,
    /// Hidden children visibility changed.
    HiddenChildrenChanged,
    /// Custom event type for extensibility.
    Custom,
}

impl SemanticsEventType {
    /// Returns the string representation of this event type.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tap => "tap",
            Self::LongPress => "longPress",
            Self::Tooltip => "tooltip",
            Self::Announce => "announce",
            Self::Focus => "focus",
            Self::ScrollCompleted => "scrollCompleted",
            Self::HiddenChildrenChanged => "hiddenChildrenChanged",
            Self::Custom => "custom",
        }
    }
}

impl std::fmt::Display for SemanticsEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// SemanticsEventData
// ============================================================================

/// A value in the event data map.
#[derive(Debug, Clone, PartialEq)]
pub enum SemanticsEventData {
    /// String value.
    String(SmolStr),
    /// Integer value.
    Int(i64),
    /// Float value.
    Float(f64),
    /// Boolean value.
    Bool(bool),
}

impl std::fmt::Display for SemanticsEventData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => write!(f, "{}", s),
            Self::Int(i) => write!(f, "{}", i),
            Self::Float(fl) => write!(f, "{}", fl),
            Self::Bool(b) => write!(f, "{}", b),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tap_event() {
        let event = SemanticsEvent::tap();
        assert_eq!(event.event_type(), SemanticsEventType::Tap);
    }

    #[test]
    fn test_tooltip_event() {
        let event = SemanticsEvent::tooltip("Hello World");
        assert_eq!(event.event_type(), SemanticsEventType::Tooltip);
        assert_eq!(event.get_string("message"), Some("Hello World"));
    }

    #[test]
    fn test_announce_event() {
        let event = SemanticsEvent::announce("Item selected");
        assert_eq!(event.event_type(), SemanticsEventType::Announce);
        assert_eq!(event.get_string("message"), Some("Item selected"));
    }

    #[test]
    fn test_focus_event() {
        let event = SemanticsEvent::focus(42);
        assert_eq!(event.event_type(), SemanticsEventType::Focus);
        assert_eq!(event.get_int("nodeId"), Some(42));
    }

    #[test]
    fn test_custom_data() {
        let mut event = SemanticsEvent::new(SemanticsEventType::Custom);
        event.set_string("key1", SmolStr::from("value1"));
        event.set_int("key2", 123);
        event.set_float("key3", 3.14);
        event.set_bool("key4", true);

        assert_eq!(event.get_string("key1"), Some("value1"));
        assert_eq!(event.get_int("key2"), Some(123));
        assert_eq!(event.get_float("key3"), Some(3.14));
        assert_eq!(event.get_bool("key4"), Some(true));
    }

    #[test]
    fn test_to_map() {
        let event = SemanticsEvent::tooltip("Test");
        let map = event.to_map();
        assert_eq!(map.get("type").map(|s| s.as_str()), Some("tooltip"));
        assert_eq!(map.get("message").map(|s| s.as_str()), Some("Test"));
    }
}
