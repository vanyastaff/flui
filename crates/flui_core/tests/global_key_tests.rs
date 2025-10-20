//! Tests for GlobalKey integration with BuildOwner
//!
//! This tests Phase 1: Key System Enhancement - GlobalKey methods

use flui_core::{AnyWidget, BuildOwner, Context, StatelessWidget};
use flui_core::foundation::key::GlobalKey;

// ============================================================================
// Test Widgets
// ============================================================================

#[derive(Debug, Clone)]
struct TestWidget {
    _key: Option<GlobalKey<()>>,
    text: String,
}

impl StatelessWidget for TestWidget {
    fn build(&self, _ctx: &Context) -> Box<dyn AnyWidget> {
        Box::new(LeafWidget { value: self.text.clone() })
    }
}

#[derive(Debug, Clone)]
struct LeafWidget {
    value: String,
}

impl StatelessWidget for LeafWidget {
    fn build(&self, _ctx: &Context) -> Box<dyn AnyWidget> {
        Box::new(self.clone())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[test]
fn test_global_key_not_registered() {
    let owner = BuildOwner::new();
    let key = GlobalKey::<()>::new();

    // Key is not registered, should return None
    assert!(key.current_context(&owner).is_none());
}

#[test]
fn test_global_key_register_and_lookup() {
    use flui_core::ElementId;

    let mut owner = BuildOwner::new();
    let key = GlobalKey::<()>::new();
    let element_id = ElementId::new();

    // Register key
    owner.register_global_key(key.to_global_key_id(), element_id);

    // Lookup should work
    assert_eq!(
        owner.get_element_for_global_key(key.to_global_key_id()),
        Some(element_id)
    );
}

#[test]
fn test_global_key_current_context_without_element() {
    use flui_core::ElementId;

    let mut owner = BuildOwner::new();
    let key = GlobalKey::<()>::new();
    let element_id = ElementId::new();

    // Register key but don't add element to tree
    owner.register_global_key(key.to_global_key_id(), element_id);

    // current_context should return None because element doesn't exist in tree
    assert!(key.current_context(&owner).is_none());
}

#[test]
fn test_global_key_current_context_with_element() {
    let mut owner = BuildOwner::new();
    let key = GlobalKey::<()>::new();

    // Create widget with key
    let widget = TestWidget {
        _key: Some(key.clone()),
        text: "test".to_string(),
    };

    // Set root (this will mount the element)
    let root_id = owner.set_root(Box::new(widget));

    // Register the key manually (in real implementation, this would be automatic)
    owner.register_global_key(key.to_global_key_id(), root_id);

    // current_context should now return Some
    let context = key.current_context(&owner);
    assert!(context.is_some());

    if let Some(ctx) = context {
        assert_eq!(ctx.element_id(), root_id);
    }
}

#[test]
fn test_global_key_current_widget_not_implemented() {
    let owner = BuildOwner::new();
    let key = GlobalKey::<()>::new();

    // current_widget is not implemented yet
    assert!(key.current_widget(&owner).is_none());
}

#[test]
fn test_global_key_current_state_not_implemented() {
    let owner = BuildOwner::new();
    let key = GlobalKey::<()>::new();

    // current_state is not implemented yet
    assert!(key.current_state(&owner).is_none());
}

#[test]
fn test_global_key_uniqueness() {
    let key1 = GlobalKey::<()>::new();
    let key2 = GlobalKey::<()>::new();

    // Each key should have unique ID
    assert_ne!(key1.raw_id(), key2.raw_id());
    assert_ne!(key1.to_global_key_id(), key2.to_global_key_id());
}

#[test]
fn test_global_key_clone() {
    let key1 = GlobalKey::<()>::new();
    let key2 = key1.clone();

    // Cloned key should have same ID
    assert_eq!(key1.raw_id(), key2.raw_id());
    assert_eq!(key1.to_global_key_id(), key2.to_global_key_id());
}

#[test]
fn test_global_key_conversion() {
    let key = GlobalKey::<()>::new();
    let key_id = key.to_global_key_id();

    // Conversion should preserve ID
    assert_eq!(key.raw_id(), key_id.raw());
}
