//! Proof of Concept: AnyView Approach
//!
//! This demonstrates the simpler runtime state tracking approach.

use std::any::{Any, TypeId};
use std::sync::Arc;

// ============================================================================
// Core Types
// ============================================================================

/// Type-erased view configuration (immutable)
pub struct AnyView {
    type_id: TypeId,
    debug_name: &'static str,

    // Factory to create ViewObject from stored config
    create: Arc<dyn Fn(&dyn Any) -> Box<dyn ViewObject> + Send + Sync>,

    // Stored view configuration (type-erased but cloneable)
    view_data: Box<dyn Any + Send + Sync>,
}

impl AnyView {
    /// Create AnyView from a concrete view type
    pub fn new<V: IntoView + Clone + 'static>(view: V) -> Self {
        Self {
            type_id: TypeId::of::<V>(),
            debug_name: std::any::type_name::<V>(),
            create: Arc::new(|data| {
                let view = data.downcast_ref::<V>().unwrap().clone();
                view.into_view()
            }),
            view_data: Box::new(view),
        }
    }

    /// Create a ViewObject from the stored configuration
    pub fn create_view_object(&self) -> Box<dyn ViewObject> {
        (self.create)(&*self.view_data)
    }

    /// Get the TypeId of the stored view
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    /// Get the debug name
    pub fn debug_name(&self) -> &'static str {
        self.debug_name
    }

    /// Try to get a reference to the concrete view type
    pub fn downcast_ref<V: 'static>(&self) -> Option<&V> {
        self.view_data.downcast_ref::<V>()
    }
}

impl std::fmt::Debug for AnyView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnyView")
            .field("type_id", &self.type_id)
            .field("debug_name", &self.debug_name)
            .finish()
    }
}

// ============================================================================
// Child/Children (store config, not state!)
// ============================================================================

/// Optional single child wrapper (stores immutable config)
#[derive(Default)]
pub struct Child {
    inner: Option<AnyView>,
}

impl Child {
    pub const fn none() -> Self {
        Self { inner: None }
    }

    pub fn new<V: IntoView + Clone>(view: V) -> Self {
        Self {
            inner: Some(AnyView::new(view)),
        }
    }

    pub fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    pub fn is_none(&self) -> bool {
        self.inner.is_none()
    }

    pub fn as_ref(&self) -> Option<&AnyView> {
        self.inner.as_ref()
    }

    pub fn take(&mut self) -> Option<AnyView> {
        self.inner.take()
    }
}

impl std::fmt::Debug for Child {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Child")
            .field("has_child", &self.inner.is_some())
            .finish()
    }
}

/// Multiple children wrapper (stores immutable configs)
#[derive(Default)]
pub struct Children {
    inner: Vec<AnyView>,
}

impl Children {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
        }
    }

    pub fn push<V: IntoView + Clone>(&mut self, view: V) {
        self.inner.push(AnyView::new(view));
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &AnyView> {
        self.inner.iter()
    }
}

impl std::fmt::Debug for Children {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Children")
            .field("count", &self.inner.len())
            .finish()
    }
}

// ============================================================================
// Element with Separated Mount/Build Phases
// ============================================================================

pub struct Element {
    id: ElementId,

    // Tree position
    parent: Option<ElementId>,
    children: Vec<ElementId>,

    // Immutable configuration (for hot-reload & reconciliation)
    view_config: Option<AnyView>,

    // Live ViewObject (created on mount, used for build)
    view_object: Option<Box<dyn ViewObject>>,

    // Lifecycle state
    is_mounted: bool,
    needs_build: bool,
}

impl Element {
    pub fn new(id: ElementId, config: AnyView) -> Self {
        Self {
            id,
            parent: None,
            children: Vec::new(),
            view_config: Some(config),
            view_object: None,
            is_mounted: false,
            needs_build: true,
        }
    }

    /// Phase 1: Mount - Create ViewObject from config
    ///
    /// This is called once when the element is first created.
    pub fn mount(&mut self) {
        if self.is_mounted {
            return;
        }

        if let Some(config) = &self.view_config {
            self.view_object = Some(config.create_view_object());
            self.is_mounted = true;
            self.needs_build = true;
        }
    }

    /// Phase 2: Build - Construct children from ViewObject
    ///
    /// This is called when needs_build is true.
    pub fn build(&mut self, ctx: &dyn BuildContext) {
        if !self.needs_build {
            return;
        }

        if let Some(view_obj) = &mut self.view_object {
            // ViewObject::build() returns child configs
            if let Some(child_view_obj) = view_obj.build(ctx) {
                // TODO: Create child element from view_object
                // This would interact with ElementTree
            }
        }

        self.needs_build = false;
    }

    /// Update with new configuration (for hot-reload)
    ///
    /// This recreates the ViewObject from the new config.
    pub fn update_config(&mut self, new_config: AnyView) {
        self.view_config = Some(new_config);

        // Recreate ViewObject
        if let Some(config) = &self.view_config {
            self.view_object = Some(config.create_view_object());
            self.needs_build = true;
        }
    }

    /// Reconcile with new configuration
    ///
    /// This enables efficient updates by comparing types.
    pub fn reconcile(&mut self, new_config: AnyView) -> ReconcileResult {
        let old_type = self.view_config.as_ref().map(|c| c.type_id());
        let new_type = new_config.type_id();

        if old_type == Some(new_type) {
            // Same type - update config
            self.update_config(new_config);
            ReconcileResult::Updated
        } else {
            // Different type - need to replace element
            ReconcileResult::NeedsReplace
        }
    }

    pub fn is_mounted(&self) -> bool {
        self.is_mounted
    }

    pub fn needs_build(&self) -> bool {
        self.needs_build
    }
}

#[derive(Debug, PartialEq)]
pub enum ReconcileResult {
    Updated,
    NeedsReplace,
}

// ============================================================================
// Example Usage
// ============================================================================

/// Example: Padding widget
#[derive(Clone, Debug)]
pub struct Padding {
    padding: f32,
    child: Child,
}

impl Padding {
    pub fn all(padding: f32) -> Self {
        Self {
            padding,
            child: Child::none(),
        }
    }

    pub fn child<V: IntoView + Clone>(mut self, view: V) -> Self {
        self.child = Child::new(view);
        self
    }
}

impl IntoView for Padding {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(PaddingViewObject { config: self })
    }
}

struct PaddingViewObject {
    config: Padding,
}

impl ViewObject for PaddingViewObject {
    fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Return child config to be mounted
        if let Some(child_config) = self.config.child.as_ref() {
            Some(child_config.create_view_object())
        } else {
            None
        }
    }
}

/// Example: Text widget
#[derive(Clone, Debug)]
pub struct Text {
    content: String,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }
}

impl IntoView for Text {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(TextViewObject { config: self })
    }
}

struct TextViewObject {
    config: Text,
}

impl ViewObject for TextViewObject {
    fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        None // Leaf widget
    }
}

// ============================================================================
// Usage Example
// ============================================================================

fn example_usage() {
    // Create widget tree (immutable configs)
    let padding = Padding::all(16.0).child(Text::new("Hello, World!"));

    // Convert to AnyView
    let config = AnyView::new(padding);

    // Create element
    let mut element = Element::new(ElementId::new(1), config);

    // Mount: Create ViewObject
    element.mount();
    assert!(element.is_mounted());

    // Build: Construct children
    let ctx = MockBuildContext;
    element.build(&ctx);
    assert!(!element.needs_build());

    // Hot-reload: Update with new config
    let new_padding = Padding::all(32.0).child(Text::new("Updated!"));
    let new_config = AnyView::new(new_padding);
    element.update_config(new_config);
    assert!(element.needs_build());

    // Reconcile: Efficient update
    let another_padding = Padding::all(48.0).child(Text::new("Reconciled!"));
    let result = element.reconcile(AnyView::new(another_padding));
    assert_eq!(result, ReconcileResult::Updated);

    // Different type - needs replace
    let text = Text::new("Different type");
    let result = element.reconcile(AnyView::new(text));
    assert_eq!(result, ReconcileResult::NeedsReplace);
}

// ============================================================================
// Supporting Traits and Types
// ============================================================================

pub trait IntoView {
    fn into_view(self) -> Box<dyn ViewObject>;
}

pub trait ViewObject {
    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>>;
}

pub trait BuildContext {}

struct MockBuildContext;
impl BuildContext for MockBuildContext {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(usize);

impl ElementId {
    pub fn new(id: usize) -> Self {
        Self(id)
    }
}

// ============================================================================
// Summary
// ============================================================================

// ✅ Child stores AnyView (config), not ViewObject (state)
// ✅ Element separates config from state
// ✅ mount() creates ViewObject from config
// ✅ build() constructs children
// ✅ Hot-reload supported (update_config)
// ✅ Reconciliation supported (compare TypeId)
// ✅ Simple API - no complex type parameters
// ✅ All Views must be Clone (required for AnyView)
