//! Lifecycle validation for debugging
//!
//! Validates element lifecycle transitions to catch bugs early.

use crate::element::ElementLifecycle;
use crate::ElementId;

/// Validate lifecycle transition
///
/// Returns Ok if transition is valid, Err with message if invalid.
pub fn validate_lifecycle_transition(
    element_id: ElementId,
    from: ElementLifecycle,
    to: ElementLifecycle,
) -> Result<(), String> {
    use ElementLifecycle::*;

    let valid = match (from, to) {
        // Valid transitions
        (Initial, Active) => true,        // mount
        (Active, Inactive) => true,       // deactivate
        (Inactive, Active) => true,       // reactivate
        (Active, Defunct) => true,        // unmount from active
        (Inactive, Defunct) => true,      // unmount from inactive

        // Invalid: already in target state
        (a, b) if a == b => false,

        // Invalid: can't go back to Initial
        (_, Initial) => false,

        // Invalid: can't leave Defunct
        (Defunct, _) => false,

        // Any other transition is invalid
        _ => false,
    };

    if valid {
        Ok(())
    } else {
        Err(format!(
            "Invalid lifecycle transition for element {:?}: {:?} -> {:?}",
            element_id, from, to
        ))
    }
}

/// Assert element is in expected lifecycle state
#[inline]
pub fn assert_lifecycle(
    element_id: ElementId,
    actual: ElementLifecycle,
    expected: ElementLifecycle,
    operation: &str,
) {
    if actual != expected {
        panic!(
            "Element {:?} lifecycle violation: Cannot {} in state {:?} (expected {:?})",
            element_id, operation, actual, expected
        );
    }
}

/// Assert element is active (mounted)
#[inline]
pub fn assert_active(element_id: ElementId, lifecycle: ElementLifecycle, operation: &str) {
    if lifecycle != ElementLifecycle::Active {
        panic!(
            "Element {:?} must be active to {}, but is in state {:?}",
            element_id, operation, lifecycle
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_transitions() {
        let id = ElementId::new();

        // Initial -> Active (mount)
        assert!(validate_lifecycle_transition(id, ElementLifecycle::Initial, ElementLifecycle::Active).is_ok());

        // Active -> Inactive (deactivate)
        assert!(validate_lifecycle_transition(id, ElementLifecycle::Active, ElementLifecycle::Inactive).is_ok());

        // Inactive -> Active (reactivate)
        assert!(validate_lifecycle_transition(id, ElementLifecycle::Inactive, ElementLifecycle::Active).is_ok());

        // Active -> Defunct (unmount)
        assert!(validate_lifecycle_transition(id, ElementLifecycle::Active, ElementLifecycle::Defunct).is_ok());
    }

    #[test]
    fn test_invalid_transitions() {
        let id = ElementId::new();

        // Can't go back to Initial
        assert!(validate_lifecycle_transition(id, ElementLifecycle::Active, ElementLifecycle::Initial).is_err());

        // Can't leave Defunct
        assert!(validate_lifecycle_transition(id, ElementLifecycle::Defunct, ElementLifecycle::Active).is_err());

        // Can't skip states
        assert!(validate_lifecycle_transition(id, ElementLifecycle::Initial, ElementLifecycle::Defunct).is_err());
    }

    #[test]
    #[should_panic(expected = "lifecycle violation")]
    fn test_assert_lifecycle_panics() {
        let id = ElementId::new();
        assert_lifecycle(id, ElementLifecycle::Initial, ElementLifecycle::Active, "test");
    }

    #[test]
    #[should_panic(expected = "must be active")]
    fn test_assert_active_panics() {
        let id = ElementId::new();
        assert_active(id, ElementLifecycle::Initial, "test");
    }
}
