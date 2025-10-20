//! Global key registry for validation
//!
//! Ensures global keys are unique across the application.

use crate::error::KeyError;
use crate::ElementId;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::RwLock;

/// Global key registry for uniqueness validation
///
/// Tracks which elements are using which global keys to ensure uniqueness.
pub struct GlobalKeyRegistry {
    /// Map from key TypeId to element that owns it
    keys: HashMap<TypeId, ElementId>,
}

impl GlobalKeyRegistry {
    /// Create new empty registry
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    /// Get global instance
    pub fn global() -> &'static RwLock<Self> {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<RwLock<GlobalKeyRegistry>> = OnceLock::new();
        INSTANCE.get_or_init(|| RwLock::new(GlobalKeyRegistry::new()))
    }

    /// Register a global key with an element
    ///
    /// Returns error if key is already registered.
    pub fn register(&mut self, key_id: TypeId, element_id: ElementId) -> Result<(), KeyError> {
        if let Some(&existing_element) = self.keys.get(&key_id) {
            return Err(KeyError::DuplicateKey {
                key_id,
                existing_element,
                new_element: element_id,
            });
        }

        self.keys.insert(key_id, element_id);
        Ok(())
    }

    /// Unregister a global key
    pub fn unregister(&mut self, key_id: &TypeId) {
        self.keys.remove(key_id);
    }

    /// Check if key is registered
    pub fn is_registered(&self, key_id: &TypeId) -> bool {
        self.keys.contains_key(key_id)
    }

    /// Get element for key
    pub fn get_element(&self, key_id: &TypeId) -> Option<ElementId> {
        self.keys.get(key_id).copied()
    }

    /// Get number of registered keys
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Clear all registered keys
    pub fn clear(&mut self) {
        self.keys.clear();
    }
}

impl Default for GlobalKeyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_key() {
        let mut registry = GlobalKeyRegistry::new();
        let key_id = TypeId::of::<()>();
        let element_id = ElementId::new();

        // First registration should succeed
        assert!(registry.register(key_id, element_id).is_ok());
        assert!(registry.is_registered(&key_id));
        assert_eq!(registry.get_element(&key_id), Some(element_id));
    }

    #[test]
    fn test_duplicate_key_error() {
        let mut registry = GlobalKeyRegistry::new();
        let key_id = TypeId::of::<()>();
        let element1 = ElementId::new();
        let element2 = ElementId::new();

        // First registration succeeds
        registry.register(key_id, element1).unwrap();

        // Second registration should fail
        let result = registry.register(key_id, element2);
        assert!(result.is_err());

        match result {
            Err(KeyError::DuplicateKey {
                existing_element,
                new_element,
                ..
            }) => {
                assert_eq!(existing_element, element1);
                assert_eq!(new_element, element2);
            }
            _ => panic!("Expected DuplicateKey error"),
        }
    }

    #[test]
    fn test_unregister_key() {
        let mut registry = GlobalKeyRegistry::new();
        let key_id = TypeId::of::<()>();
        let element_id = ElementId::new();

        registry.register(key_id, element_id).unwrap();
        assert!(registry.is_registered(&key_id));

        registry.unregister(&key_id);
        assert!(!registry.is_registered(&key_id));
    }

    #[test]
    fn test_global_instance() {
        // Test that global instance is accessible
        let registry = GlobalKeyRegistry::global();
        assert!(registry.read().is_ok());
    }

    #[test]
    fn test_registry_clear() {
        let mut registry = GlobalKeyRegistry::new();
        let key1 = TypeId::of::<()>();
        let key2 = TypeId::of::<i32>();

        registry.register(key1, ElementId::new()).unwrap();
        registry.register(key2, ElementId::new()).unwrap();

        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());

        registry.clear();

        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }
}
