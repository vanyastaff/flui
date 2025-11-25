//! Serialization support for foundation types
//!
//! This module provides serde serialization and deserialization support
//! for FLUI Foundation types when the `serde` feature is enabled.

use crate::{DiagnosticLevel, ElementId, FoundationError, Key, KeyRef, Slot};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

// ============================================================================
// ELEMENTID SERIALIZATION
// ============================================================================

impl Serialize for ElementId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.get() as u64)
    }
}

impl<'de> Deserialize<'de> for ElementId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = u64::deserialize(deserializer)?;
        if id == 0 {
            return Err(serde::de::Error::custom(
                "ElementId cannot be zero (uses NonZeroUsize internally)",
            ));
        }

        // Convert to usize (may truncate on 32-bit systems)
        let id_usize = id as usize;
        if id_usize == 0 {
            return Err(serde::de::Error::custom(
                "ElementId overflowed when converting from u64 to usize",
            ));
        }

        Ok(ElementId::new(id_usize))
    }
}

// ============================================================================
// KEY SERIALIZATION
// ============================================================================

impl Serialize for Key {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.id())
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = u64::deserialize(deserializer)?;
        Ok(Key::from_id(id))
    }
}

impl Serialize for KeyRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.key().serialize(serializer)
    }
}

// ============================================================================
// SLOT SERIALIZATION
// ============================================================================

impl Serialize for Slot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as the contained key
        self.key().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let key = Key::deserialize(deserializer)?;
        Ok(Slot::new(key))
    }
}

// ============================================================================
// DIAGNOSTIC LEVEL SERIALIZATION
// ============================================================================

impl Serialize for DiagnosticLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DiagnosticLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// ============================================================================
// FOUNDATION ERROR SERIALIZATION
// ============================================================================

/// A serializable version of FoundationError.
///
/// Since the full FoundationError enum may contain types that don't implement
/// Serialize, we provide a simplified serializable representation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableFoundationError {
    pub category: String,
    pub message: String,
    pub recoverable: bool,
}

impl From<&FoundationError> for SerializableFoundationError {
    fn from(error: &FoundationError) -> Self {
        Self {
            category: error.category().to_string(),
            message: error.to_string(),
            recoverable: error.is_recoverable(),
        }
    }
}

impl From<FoundationError> for SerializableFoundationError {
    fn from(error: FoundationError) -> Self {
        Self::from(&error)
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Serializes a foundation type to JSON string.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::{ElementId, serde_support::to_json_string};
///
/// let element_id = ElementId::new(42);
/// let json = to_json_string(&element_id).unwrap();
/// assert_eq!(json, "42");
/// ```
pub fn to_json_string<T>(value: &T) -> Result<String, FoundationError>
where
    T: Serialize,
{
    serde_json::to_string(value).map_err(|e| {
        FoundationError::serialization_error(format!("JSON serialization failed: {}", e))
    })
}

/// Deserializes a foundation type from JSON string.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::{ElementId, serde_support::from_json_string};
///
/// let json = "42";
/// let element_id: ElementId = from_json_string(json).unwrap();
/// assert_eq!(element_id.get(), 42);
/// ```
pub fn from_json_string<T>(json: &str) -> Result<T, FoundationError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(json).map_err(|e| {
        FoundationError::serialization_error(format!("JSON deserialization failed: {}", e))
    })
}

/// Serializes a foundation type to binary format using bincode.
pub fn to_binary<T>(value: &T) -> Result<Vec<u8>, FoundationError>
where
    T: Serialize,
{
    bincode::serialize(value).map_err(|e| {
        FoundationError::serialization_error(format!("Binary serialization failed: {}", e))
    })
}

/// Deserializes a foundation type from binary format using bincode.
pub fn from_binary<T>(data: &[u8]) -> Result<T, FoundationError>
where
    T: for<'de> Deserialize<'de>,
{
    bincode::deserialize(data).map_err(|e| {
        FoundationError::serialization_error(format!("Binary deserialization failed: {}", e))
    })
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_element_id_serialization() {
        let element_id = ElementId::new(42);
        let json = serde_json::to_string(&element_id).unwrap();
        assert_eq!(json, "42");

        let deserialized: ElementId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.get(), 42);
    }

    #[test]
    fn test_element_id_zero_rejection() {
        let json = "0";
        let result: Result<ElementId, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_key_serialization() {
        let key = Key::new();
        let json = serde_json::to_string(&key).unwrap();

        let deserialized: Key = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id(), key.id());
    }

    #[test]
    fn test_slot_serialization() {
        let key = Key::new();
        let slot = Slot::new(key);

        let json = serde_json::to_string(&slot).unwrap();
        let deserialized: Slot = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.key(), key);
    }

    #[test]
    fn test_diagnostic_level_serialization() {
        let level = DiagnosticLevel::Warning;
        let json = serde_json::to_string(&level).unwrap();
        assert_eq!(json, "\"warning\"");

        let deserialized: DiagnosticLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, DiagnosticLevel::Warning);
    }

    #[test]
    fn test_serializable_foundation_error() {
        let error = FoundationError::invalid_id(0, "test error");
        let serializable = SerializableFoundationError::from(&error);

        assert_eq!(serializable.category, "invalid_id");
        assert!(serializable.message.contains("Invalid ID: 0"));
        assert!(!serializable.recoverable);

        // Test JSON roundtrip
        let json = serde_json::to_string(&serializable).unwrap();
        let deserialized: SerializableFoundationError = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.category, serializable.category);
    }

    #[test]
    fn test_utility_functions() {
        let element_id = ElementId::new(123);

        // JSON
        let json = to_json_string(&element_id).unwrap();
        let recovered: ElementId = from_json_string(&json).unwrap();
        assert_eq!(recovered.get(), 123);

        // Binary
        let binary = to_binary(&element_id).unwrap();
        let recovered: ElementId = from_binary(&binary).unwrap();
        assert_eq!(recovered.get(), 123);
    }

    #[test]
    fn test_error_handling() {
        // Invalid JSON should produce FoundationError
        let result: Result<ElementId, _> = from_json_string("invalid json");
        assert!(result.is_err());

        match result.unwrap_err() {
            FoundationError::SerializationError { context } => {
                assert!(context.contains("JSON deserialization failed"));
            }
            _ => panic!("Expected SerializationError"),
        }
    }
}
