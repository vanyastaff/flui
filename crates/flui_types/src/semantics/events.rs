//! Semantic events for accessibility announcements

/// Base trait for semantic events
///
/// Similar to Flutter's `SemanticsEvent`. Events that can be dispatched
/// to the accessibility system.
pub trait SemanticsEvent {
    /// Returns the type name of this event
    fn event_type(&self) -> &str;
}

/// An event that announces a message to the accessibility system
///
/// Similar to Flutter's `AnnounceSemanticsEvent`. This is used to make
/// screen readers announce important information.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::{AnnounceSemanticsEvent, SemanticsEvent};
///
/// let event = AnnounceSemanticsEvent::new("Form submitted successfully");
/// assert_eq!(event.message(), "Form submitted successfully");
/// assert_eq!(event.event_type(), "announce");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnnounceSemanticsEvent {
    message: String,
    assertiveness: Assertiveness,
}

/// How assertive the announcement should be
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Assertiveness {
    /// Polite announcement (won't interrupt)
    Polite,

    /// Assertive announcement (may interrupt)
    Assertive,
}

impl AnnounceSemanticsEvent {
    /// Creates a new announce event with polite assertiveness
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            assertiveness: Assertiveness::Polite,
        }
    }

    /// Creates a new announce event with assertive assertiveness
    pub fn assertive(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            assertiveness: Assertiveness::Assertive,
        }
    }

    /// Returns the message
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the assertiveness level
    pub fn assertiveness(&self) -> Assertiveness {
        self.assertiveness
    }
}

impl SemanticsEvent for AnnounceSemanticsEvent {
    fn event_type(&self) -> &str {
        "announce"
    }
}

/// An event indicating a tap occurred
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TapSemanticEvent;

impl SemanticsEvent for TapSemanticEvent {
    fn event_type(&self) -> &str {
        "tap"
    }
}

/// An event indicating a long press occurred
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LongPressSemanticsEvent;

impl SemanticsEvent for LongPressSemanticsEvent {
    fn event_type(&self) -> &str {
        "long_press"
    }
}

/// An event indicating focus changed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FocusSemanticEvent {
    /// Whether focus was gained or lost
    pub gained: bool,
}

impl FocusSemanticEvent {
    /// Creates a focus gained event
    pub fn gained() -> Self {
        Self { gained: true }
    }

    /// Creates a focus lost event
    pub fn lost() -> Self {
        Self { gained: false }
    }
}

impl SemanticsEvent for FocusSemanticEvent {
    fn event_type(&self) -> &str {
        if self.gained {
            "focus_gained"
        } else {
            "focus_lost"
        }
    }
}

/// An event for tooltip announcements
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TooltipSemanticsEvent {
    message: String,
}

impl TooltipSemanticsEvent {
    /// Creates a new tooltip event
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the tooltip message
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl SemanticsEvent for TooltipSemanticsEvent {
    fn event_type(&self) -> &str {
        "tooltip"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_announce_event() {
        let event = AnnounceSemanticsEvent::new("Test message");
        assert_eq!(event.message(), "Test message");
        assert_eq!(event.assertiveness(), Assertiveness::Polite);
        assert_eq!(event.event_type(), "announce");
    }

    #[test]
    fn test_announce_event_assertive() {
        let event = AnnounceSemanticsEvent::assertive("Important!");
        assert_eq!(event.message(), "Important!");
        assert_eq!(event.assertiveness(), Assertiveness::Assertive);
    }

    #[test]
    fn test_tap_event() {
        let event = TapSemanticEvent;
        assert_eq!(event.event_type(), "tap");
    }

    #[test]
    fn test_long_press_event() {
        let event = LongPressSemanticsEvent;
        assert_eq!(event.event_type(), "long_press");
    }

    #[test]
    fn test_focus_event_gained() {
        let event = FocusSemanticEvent::gained();
        assert!(event.gained);
        assert_eq!(event.event_type(), "focus_gained");
    }

    #[test]
    fn test_focus_event_lost() {
        let event = FocusSemanticEvent::lost();
        assert!(!event.gained);
        assert_eq!(event.event_type(), "focus_lost");
    }

    #[test]
    fn test_tooltip_event() {
        let event = TooltipSemanticsEvent::new("Help text");
        assert_eq!(event.message(), "Help text");
        assert_eq!(event.event_type(), "tooltip");
    }
}
