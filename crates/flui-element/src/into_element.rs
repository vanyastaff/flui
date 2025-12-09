//! IntoElement trait - Convert types into Element
//!
//! This module provides the `IntoElement` trait for converting various types
//! into Element instances.

use crate::element::Element;

/// Trait for types that can be converted into an Element.
///
/// This is the primary way to create Elements from ViewObjects and other types.
///
/// # Four-Tree Architecture
///
/// In the four-tree architecture, ViewObjects and RenderObjects must be inserted
/// into their respective trees (ViewTree, RenderTree) first to obtain IDs.
/// Then Element can be created referencing those IDs.
///
/// # Example
///
/// ```rust,ignore
/// use flui_element::{IntoElement, Element};
///
/// // Create element from existing element (identity)
/// let element = my_element.into_element();
///
/// // Create element from unit (empty element)
/// let element = ().into_element();
///
/// // Create element from Option
/// let element = Some(my_element).into_element();
/// ```
pub trait IntoElement {
    /// Convert this value into an Element.
    fn into_element(self) -> Element;
}

// ============================================================================
// IMPLEMENTATIONS
// ============================================================================

/// Identity implementation - Element converts to itself.
impl IntoElement for Element {
    #[inline]
    fn into_element(self) -> Element {
        self
    }
}

/// Unit type converts to empty Element.
impl IntoElement for () {
    #[inline]
    fn into_element(self) -> Element {
        Element::empty()
    }
}

/// Option<T: IntoElement> converts to Element or empty.
impl<T: IntoElement> IntoElement for Option<T> {
    fn into_element(self) -> Element {
        match self {
            Some(inner) => inner.into_element(),
            None => Element::empty(),
        }
    }
}

/// Vec<Element> converts to a container Element with pending children.
impl IntoElement for Vec<Element> {
    fn into_element(self) -> Element {
        if self.is_empty() {
            Element::empty()
        } else {
            Element::container(Element::boxed_children(self))
        }
    }
}

// ============================================================================
// RENDER VIEW IMPLEMENTATIONS
// ============================================================================

use flui_rendering::core::arity::{Leaf, Single};
use flui_rendering::core::protocol::BoxProtocol;
use flui_rendering::core::{ProtocolId, RenderElement};
use flui_tree::RuntimeArity;
use flui_view::{RenderObjectFor, RenderView, RenderViewLeaf, RenderViewWithChild};

/// RenderViewLeaf converts to a Render element with pending render object.
///
/// This enables the pattern: `Text::headline("Hello").leaf().into_element()`
impl<V> IntoElement for RenderViewLeaf<V>
where
    V: RenderView<BoxProtocol, Leaf>,
    V::RenderObject: RenderObjectFor<BoxProtocol, Leaf> + 'static,
{
    fn into_element(self) -> Element {
        let render_object = self.view.create();
        Element::render_with_pending(
            Box::new(render_object),
            ProtocolId::Box,
            RuntimeArity::Exact(0),
        )
    }
}

/// RenderViewWithChild converts to a Render element with pending render object and child.
///
/// This enables the pattern: `Padding::all(16.0).with_child(child).into_element()`
impl<V, C> IntoElement for RenderViewWithChild<V, C>
where
    V: RenderView<BoxProtocol, Single>,
    V::RenderObject: RenderObjectFor<BoxProtocol, Single> + 'static,
    C: IntoElement,
{
    fn into_element(self) -> Element {
        let render_object = self.view.create();
        let child_element = self.child.into_element();

        Element::Render(RenderElement::with_pending_and_children(
            Box::new(render_object),
            ProtocolId::Box,
            RuntimeArity::Exact(1),
            vec![Box::new(child_element)],
        ))
    }
}

// ============================================================================
// VIEW WRAPPER IMPLEMENTATIONS
// ============================================================================

use flui_view::{
    Animated, AnimatedView, AnimatedViewWrapper, Listenable, Provider, ProviderView,
    ProviderViewWrapper, Proxy, ProxyView, ProxyViewWrapper, Stateful, StatefulView,
    StatefulViewWrapper, Stateless, StatelessView, StatelessViewWrapper, ViewElement, ViewMode,
};

/// Stateless wrapper converts to a View element with pending view object.
impl<V: StatelessView> IntoElement for Stateless<V> {
    fn into_element(self) -> Element {
        let wrapper = StatelessViewWrapper::new(self.0);
        Element::View(ViewElement::with_pending(
            Box::new(wrapper),
            ViewMode::Stateless,
        ))
    }
}

/// Stateful wrapper converts to a View element with pending view object.
impl<V: StatefulView> IntoElement for Stateful<V> {
    fn into_element(self) -> Element {
        let wrapper = StatefulViewWrapper::new(self.0);
        Element::View(ViewElement::with_pending(
            Box::new(wrapper),
            ViewMode::Stateful,
        ))
    }
}

/// Proxy wrapper converts to a View element with pending view object.
impl<V: ProxyView> IntoElement for Proxy<V> {
    fn into_element(self) -> Element {
        let wrapper = ProxyViewWrapper::new(self.0);
        Element::View(ViewElement::with_pending(
            Box::new(wrapper),
            ViewMode::Proxy,
        ))
    }
}

/// Provider wrapper converts to a View element with pending view object.
impl<V, T> IntoElement for Provider<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    fn into_element(self) -> Element {
        let wrapper = ProviderViewWrapper::new(self.0);
        Element::View(ViewElement::with_pending(
            Box::new(wrapper),
            ViewMode::Provider,
        ))
    }
}

/// Animated wrapper converts to a View element with pending view object.
impl<V, L> IntoElement for Animated<V, L>
where
    V: AnimatedView<L>,
    L: Listenable,
{
    fn into_element(self) -> Element {
        let wrapper = AnimatedViewWrapper::new(self.0);
        Element::View(ViewElement::with_pending(
            Box::new(wrapper),
            ViewMode::Animated,
        ))
    }
}

// ============================================================================
// CHILD IMPLEMENTATION
// ============================================================================

use flui_view::{Child, Children};

/// Child converts to Element (for single-child widgets like Padding).
impl IntoElement for Child {
    fn into_element(self) -> Element {
        match self.into_inner() {
            Some(view_object) => {
                let mode = view_object.mode();
                Element::View(ViewElement::with_pending(view_object, mode))
            }
            None => Element::empty(),
        }
    }
}

/// Children converts to container Element with pending children.
impl IntoElement for Children {
    fn into_element(self) -> Element {
        let view_objects = self.into_inner();
        if view_objects.is_empty() {
            Element::empty()
        } else {
            let child_elements: Vec<Element> = view_objects
                .into_iter()
                .map(|view_object| {
                    let mode = view_object.mode();
                    Element::View(ViewElement::with_pending(view_object, mode))
                })
                .collect();
            child_elements.into_element()
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_into_element() {
        let element = Element::empty();
        let result = element.into_element();
        assert!(matches!(result, Element::View(_)));
    }

    #[test]
    fn test_unit_into_element() {
        let element = ().into_element();
        assert!(matches!(element, Element::View(_)));
    }

    #[test]
    fn test_option_some_into_element() {
        let element = Some(Element::empty()).into_element();
        assert!(matches!(element, Element::View(_)));
    }

    #[test]
    fn test_option_none_into_element() {
        let element: Element = None::<Element>.into_element();
        assert!(matches!(element, Element::View(_)));
    }

    #[test]
    fn test_vec_empty_into_element() {
        let elements: Vec<Element> = vec![];
        let element = elements.into_element();
        assert!(matches!(element, Element::View(_)));
    }

    #[test]
    fn test_vec_with_children_into_element() {
        let elements = vec![Element::empty(), Element::empty()];
        let element = elements.into_element();

        let Element::View(view_elem) = element else {
            panic!("Expected View element, got Render element")
        };
        assert!(view_elem.has_pending_children());
    }
}
