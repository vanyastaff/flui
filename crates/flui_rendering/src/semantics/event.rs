//! Semantics events for accessibility notifications.
//!
//! This module re-exports types from `flui-semantics` for use in the rendering layer.

// Re-export all event types from flui-semantics
pub use flui_semantics::{SemanticsEvent, SemanticsEventData, SemanticsEventType};

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
        event.set_string("key1", "value1".into());
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
