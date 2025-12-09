//! Proof of Concept: Typestate Approach
//!
//! This demonstrates the compile-time state enforcement approach
//! and shows why it's more complex than AnyView.

use std::any::{Any, TypeId};
use std::marker::PhantomData;

// ============================================================================
// Typestate Core Types
// ============================================================================

/// Marker trait for view states (with generic for view type)
pub trait ViewState<V> {
    type Inner;
}

/// Unmounted state: holds immutable config
pub struct Unmounted;

pub struct UnmountedInner<V> {
    view_config: V,
}

impl<V: Clone> ViewState<V> for Unmounted {
    type Inner = UnmountedInner<V>;
}

/// Mounted state: holds live ViewObject
pub struct Mounted;

pub struct MountedInner {
    view_object: Box<dyn ViewObject>,
    parent: Option<ElementId>,
    children: Vec<ElementId>,
}

// ❌ Problem: Mounted is not generic over V!
// We lose the view type information after mounting
impl ViewState<()> for Mounted {
    type Inner = MountedInner;
}

/// Type-safe view handle with state enforcement
pub struct ViewHandle<V, S: ViewState<V>> {
    type_id: TypeId,
    debug_name: &'static str,
    inner: S::Inner,
    _phantom: PhantomData<V>,
}

// ============================================================================
// State Transitions
// ============================================================================

impl<V: IntoView + Clone + 'static> ViewHandle<V, Unmounted> {
    /// Create a new unmounted view handle
    pub fn new(view: V) -> Self {
        Self {
            type_id: TypeId::of::<V>(),
            debug_name: std::any::type_name::<V>(),
            inner: UnmountedInner { view_config: view },
            _phantom: PhantomData,
        }
    }

    /// Get reference to the view config
    pub fn config(&self) -> &V {
        &self.inner.view_config
    }

    /// Mount: Transition from Unmounted to Mounted
    ///
    /// ✅ Compile-time guarantee: Can only mount once
    /// ✅ Type-safe: Consumes self, returns Mounted handle
    pub fn mount(self) -> ViewHandle<(), Mounted> {
        let view_object = self.inner.view_config.into_view();

        ViewHandle {
            type_id: self.type_id,
            debug_name: self.debug_name,
            inner: MountedInner {
                view_object,
                parent: None,
                children: Vec::new(),
            },
            _phantom: PhantomData,
        }
    }
}

impl ViewHandle<(), Mounted> {
    /// Access the ViewObject
    ///
    /// ✅ Can only call this on Mounted handle
    pub fn view_object(&self) -> &dyn ViewObject {
        &*self.inner.view_object
    }

    pub fn view_object_mut(&mut self) -> &mut dyn ViewObject {
        &mut *self.inner.view_object
    }

    /// Get parent element
    pub fn parent(&self) -> Option<ElementId> {
        self.inner.parent
    }

    /// Get children
    pub fn children(&self) -> &[ElementId] {
        &self.inner.children
    }
}

impl<V, S: ViewState<V>> ViewHandle<V, S> {
    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    pub fn debug_name(&self) -> &'static str {
        self.debug_name
    }
}

// ============================================================================
// Type Erasure Problem
// ============================================================================

// ❌ Problem: Child needs to store heterogeneous unmounted views
// But ViewHandle<V, Unmounted> has different V for each child!

/// Trait for type-erased unmounted views
pub trait AnyUnmountedView: Send + Sync {
    fn mount(self: Box<Self>) -> Box<dyn ViewObject>;
    fn type_id(&self) -> TypeId;
    fn debug_name(&self) -> &'static str;
}

/// Implement AnyUnmountedView for all unmounted ViewHandles
impl<V: IntoView + Clone + 'static> AnyUnmountedView for ViewHandle<V, Unmounted> {
    fn mount(self: Box<Self>) -> Box<dyn ViewObject> {
        (*self).mount().inner.view_object
    }

    fn type_id(&self) -> TypeId {
        ViewHandle::type_id(self)
    }

    fn debug_name(&self) -> &'static str {
        ViewHandle::debug_name(self)
    }
}

// Child STILL needs type erasure!
pub struct Child {
    // ⚠️ Same as AnyView approach - we still use trait objects!
    inner: Option<Box<dyn AnyUnmountedView>>,
}

impl Child {
    pub const fn none() -> Self {
        Self { inner: None }
    }

    pub fn new<V: IntoView + Clone>(view: V) -> Self {
        Self {
            inner: Some(Box::new(ViewHandle::new(view))),
        }
    }

    pub fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    pub fn take(&mut self) -> Option<Box<dyn AnyUnmountedView>> {
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

// ============================================================================
// Element with Typestate
// ============================================================================

pub struct Element {
    id: ElementId,
    state: ElementState,
}

/// Element state enum (runtime state despite typestate!)
///
/// ⚠️ We STILL need runtime state tracking because:
/// 1. Elements are stored in a heterogeneous collection
/// 2. Need to handle different types of elements
/// 3. Type-erased ViewHandles lose compile-time guarantees
pub enum ElementState {
    Unmounted {
        config: Box<dyn AnyUnmountedView>,
    },
    Mounted {
        view_object: Box<dyn ViewObject>,
        parent: Option<ElementId>,
        children: Vec<ElementId>,
    },
}

impl Element {
    pub fn new(id: ElementId, config: Box<dyn AnyUnmountedView>) -> Self {
        Self {
            id,
            state: ElementState::Unmounted { config },
        }
    }

    /// Mount the element
    ///
    /// ❌ Runtime check still needed!
    pub fn mount(&mut self) {
        match std::mem::replace(&mut self.state, ElementState::Mounted {
            view_object: Box::new(EmptyViewObject),
            parent: None,
            children: Vec::new(),
        }) {
            ElementState::Unmounted { config } => {
                let view_object = config.mount();
                self.state = ElementState::Mounted {
                    view_object,
                    parent: None,
                    children: Vec::new(),
                };
            }
            already_mounted => {
                self.state = already_mounted;
            }
        }
    }

    pub fn is_mounted(&self) -> bool {
        matches!(self.state, ElementState::Mounted { .. })
    }

    pub fn view_object(&self) -> Option<&dyn ViewObject> {
        match &self.state {
            ElementState::Mounted { view_object, .. } => Some(&**view_object),
            _ => None,
        }
    }
}

// ============================================================================
// Example Usage (Compare with AnyView)
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
        if let Some(child_config) = self.config.child.take() {
            Some(child_config.mount())
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
        None
    }
}

// ============================================================================
// Usage Example
// ============================================================================

fn example_usage() {
    // Create unmounted handle (type-safe!)
    let padding = Padding::all(16.0).child(Text::new("Hello"));
    let unmounted_handle = ViewHandle::new(padding);

    // ✅ Compile-time guarantee: config() only on unmounted
    let _config = unmounted_handle.config();

    // ✅ Compile-time guarantee: mount() consumes unmounted, returns mounted
    let mounted_handle = unmounted_handle.mount();

    // ✅ Compile-time guarantee: view_object() only on mounted
    let _view_obj = mounted_handle.view_object();

    // ❌ But as soon as we put it in Element, we lose type safety!
    let padding2 = Padding::all(32.0);
    let config: Box<dyn AnyUnmountedView> = Box::new(ViewHandle::new(padding2));
    let mut element = Element::new(ElementId::new(1), config);

    // ❌ Runtime check needed (same as AnyView approach)
    element.mount();
    assert!(element.is_mounted());
}

// ============================================================================
// Analysis: Where Does Typestate Help?
// ============================================================================

// ✅ Helps: Direct usage of ViewHandle (no type erasure)
fn type_safe_usage() {
    let text = Text::new("Hello");
    let unmounted = ViewHandle::new(text);

    // ✅ Compile error: can't access view_object() on unmounted
    // let _ = unmounted.view_object();  // ERROR!

    let mounted = unmounted.mount();

    // ✅ Compile error: can't call mount() again
    // let _ = mounted.mount();  // ERROR!

    // ✅ OK: Can access view_object() on mounted
    let _ = mounted.view_object();
}

// ❌ Doesn't help: After type erasure (same as AnyView)
fn type_erased_usage() {
    let padding = Padding::all(16.0);
    let config: Box<dyn AnyUnmountedView> = Box::new(ViewHandle::new(padding));

    // ❌ Typestate benefits lost - just runtime checks
    let view_object = config.mount();

    // ❌ Could call mount() multiple times if we had multiple references
    // (same problem as AnyView approach)
}

// ============================================================================
// Comparison Summary
// ============================================================================

/*
┌─────────────────────────────────────────────────────────────────────────┐
│                     Typestate vs AnyView Comparison                      │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│  Direct Usage (No Type Erasure):                                         │
│    Typestate:  ✅ Compile-time safety for mount state                    │
│    AnyView:    ❌ Runtime checks only                                    │
│                                                                           │
│  Type-Erased Usage (Child, Element):                                     │
│    Typestate:  ❌ Same as AnyView - trait objects, runtime checks        │
│    AnyView:    ❌ Runtime checks                                         │
│                                                                           │
│  Implementation Complexity:                                               │
│    Typestate:  ❌ High - multiple type parameters, trait objects         │
│    AnyView:    ✅ Low - single type, simple erasure                      │
│                                                                           │
│  API Ergonomics:                                                          │
│    Typestate:  ❌ Complex - ViewHandle<V, S> type parameters             │
│    AnyView:    ✅ Simple - just AnyView                                  │
│                                                                           │
│  Hot-reload Support:                                                      │
│    Typestate:  ⚠️ Need to store config separately (lost after mount)    │
│    AnyView:    ✅ Config stored in AnyView                               │
│                                                                           │
│  Reconciliation:                                                          │
│    Typestate:  ⚠️ Need compare trait for configs                        │
│    AnyView:    ✅ Compare TypeId directly                                │
│                                                                           │
├─────────────────────────────────────────────────────────────────────────┤
│  Key Insight:                                                             │
│                                                                           │
│  90% of our code uses type-erased Child/Element, where typestate         │
│  benefits disappear. The remaining 10% gains compile-time safety         │
│  but at significant complexity cost.                                      │
│                                                                           │
│  Verdict: Typestate is overkill for this use case.                       │
└─────────────────────────────────────────────────────────────────────────┘
*/

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(usize);

impl ElementId {
    pub fn new(id: usize) -> Self {
        Self(id)
    }
}

struct EmptyViewObject;

impl ViewObject for EmptyViewObject {
    fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        None
    }
}

// ============================================================================
// Conclusion
// ============================================================================

// The typestate approach provides compile-time guarantees ONLY for non-erased
// ViewHandle usage. But 90% of our codebase uses type-erased Child/Element,
// where we still need:
//
// 1. Trait objects (Box<dyn AnyUnmountedView>)
// 2. Runtime state checks
// 3. Complex type parameters
//
// AnyView provides the same capabilities with much simpler implementation.
//
// Recommendation: Use AnyView approach.
