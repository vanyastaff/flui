//! The animation slot a transition's state keeps: the proxy its render object
//! listens to for life, and the animation the proxy currently wraps.

use std::rc::Rc;

use flui_animation::{Animation, ProxyAnimation};

/// A stable [`ProxyAnimation`] retargeted in place when the view hands the
/// transition a different animation.
#[derive(Debug)]
pub(super) struct ProxySlot<T: Clone + 'static> {
    proxy: ProxyAnimation<T>,
    source: Rc<dyn Animation<T>>,
}

impl<T: Clone + 'static> ProxySlot<T> {
    pub(super) fn new(source: &Rc<dyn Animation<T>>) -> Self {
        Self {
            proxy: ProxyAnimation::new(Rc::clone(source)),
            source: Rc::clone(source),
        }
    }

    /// The proxy the render object subscribes to.
    pub(super) fn proxy(&self) -> ProxyAnimation<T> {
        self.proxy.clone()
    }

    /// Points the proxy at `source` unless it already wraps that animation.
    pub(super) fn retarget(&mut self, source: &Rc<dyn Animation<T>>) {
        if !Rc::ptr_eq(&self.source, source) {
            self.source = Rc::clone(source);
            self.proxy.set_parent(Rc::clone(source));
        }
    }
}
