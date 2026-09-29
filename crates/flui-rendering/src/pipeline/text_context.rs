//! The realm's text context, lent to layout (ADR-0092 §10 step 3).
//!
//! A realm owns one [`TextContext`] over the app's font collection and every
//! presentation's [`PipelineOwner`](super::PipelineOwner) holds a
//! [`TextContextHandle`] to it. Layout, intrinsic, dry-layout and
//! dry-baseline contexts lend it to a render object as a [`TextCx`], a scoped
//! `&mut TextContext` taken from `&mut` context, so a render object never
//! names the `RefCell` and cannot hold two loans at once through the context
//! API.
//!
//! The one `RefCell` sits between the realm and its pipelines, not on a
//! render object: it is borrowed once per measurement, on the owner thread,
//! and a second borrow at the same time is a bug (`BUG:` panic), not a
//! contended lock. A presentation's layout never drives another's
//! synchronously, since a `PipelineCell` checkout is not re-entrant.

use std::cell::{RefCell, RefMut};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use flui_painting::{FontCollection, TextContext};

/// A realm's text context, shared with each presentation's pipeline.
///
/// Cloning shares the context ([`TextContextHandle::ptr_eq`]). `!Send`, like
/// the realm and pipeline owners that hold it.
#[derive(Clone)]
pub struct TextContextHandle(Rc<RefCell<TextContext>>);

impl TextContextHandle {
    /// A handle over `context`.
    #[must_use]
    pub fn new(context: TextContext) -> Self {
        Self(Rc::new(RefCell::new(context)))
    }

    /// Whether `a` and `b` share one context.
    #[must_use]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    /// Runs `f` on the context.
    ///
    /// # Panics
    ///
    /// If the context is already lent, which only a measurement that
    /// re-enters another on the same realm could cause.
    pub fn with<R>(&self, f: impl FnOnce(&mut TextContext) -> R) -> R {
        f(&mut lend(&self.0))
    }

    /// The shared cell layout borrows from.
    pub(crate) fn cell(&self) -> &RefCell<TextContext> {
        &self.0
    }
}

impl fmt::Debug for TextContextHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("TextContextHandle");
        debug.field("id", &Rc::as_ptr(&self.0));
        match self.0.try_borrow() {
            Ok(context) => debug.field("context", &*context),
            Err(_) => debug.field("context", &"<lent>"),
        };
        debug.finish()
    }
}

/// The text context lent to one measurement: the realm's, or one a context
/// built for itself when its pipeline has none.
///
/// Dereferences to `&mut TextContext`, so a render object passes
/// `&mut ctx.text()` straight to `TextPainter`.
pub struct TextCx<'a>(Lent<'a>);

enum Lent<'a> {
    Shared(RefMut<'a, TextContext>),
    Own(&'a mut TextContext),
}

impl Deref for TextCx<'_> {
    type Target = TextContext;

    fn deref(&self) -> &TextContext {
        match &self.0 {
            Lent::Shared(context) => context,
            Lent::Own(context) => context,
        }
    }
}

impl DerefMut for TextCx<'_> {
    fn deref_mut(&mut self) -> &mut TextContext {
        match &mut self.0 {
            Lent::Shared(context) => context,
            Lent::Own(context) => context,
        }
    }
}

impl fmt::Debug for TextCx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TextCx").field(&**self).finish()
    }
}

/// Borrows the realm's context for one measurement.
#[expect(
    clippy::expect_used,
    reason = "a second loan is a re-entrant measurement, an invariant violation"
)]
pub(crate) fn lend(cell: &RefCell<TextContext>) -> TextCx<'_> {
    TextCx(Lent::Shared(cell.try_borrow_mut().expect(
        "BUG: the realm's text context is lent to one measurement at a time",
    )))
}

/// A context over a collection of its own, for a measurement with no realm
/// behind it: a pipeline that was never given the realm's context, or a
/// context a test built by hand.
pub(crate) fn private_context() -> TextContext {
    TextContext::new(&FontCollection::new())
}

/// Where a layout or query context gets the text context it lends: the
/// pipeline's, or one of its own built on first use.
#[derive(Default)]
pub(crate) struct TextSlot<'a> {
    source: Option<&'a RefCell<TextContext>>,
    own: Option<Box<TextContext>>,
}

impl<'a> TextSlot<'a> {
    /// A slot that lends `source`, or its own context when `None`.
    pub(crate) fn new(source: Option<&'a RefCell<TextContext>>) -> Self {
        Self { source, own: None }
    }

    /// The pipeline's context, if the slot has one.
    pub(crate) fn source(&self) -> Option<&'a RefCell<TextContext>> {
        self.source
    }

    /// Lends `source` when given, else the pipeline's context, else the
    /// slot's own.
    pub(crate) fn lend_from<'s>(
        &'s mut self,
        source: Option<&'s RefCell<TextContext>>,
    ) -> TextCx<'s> {
        match source.or(self.source) {
            Some(cell) => lend(cell),
            None => self.lend_own(),
        }
    }

    /// Lends the pipeline's context, or the slot's own.
    pub(crate) fn lend(&mut self) -> TextCx<'_> {
        self.lend_from(None)
    }

    fn lend_own(&mut self) -> TextCx<'_> {
        TextCx(Lent::Own(
            self.own.get_or_insert_with(|| Box::new(private_context())),
        ))
    }
}

impl fmt::Debug for TextSlot<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextSlot")
            .field("shared", &self.source.is_some())
            .field("own", &self.own.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use static_assertions::assert_not_impl_any;

    use super::{TextContextHandle, TextSlot, private_context};

    assert_not_impl_any!(TextContextHandle: Send, Sync);

    #[test]
    fn a_clone_shares_the_context() {
        let handle = TextContextHandle::new(private_context());
        assert!(TextContextHandle::ptr_eq(&handle, &handle.clone()));
        assert!(!TextContextHandle::ptr_eq(
            &handle,
            &TextContextHandle::new(private_context())
        ));
    }

    #[test]
    fn a_slot_lends_the_shared_context_and_releases_it() {
        let handle = TextContextHandle::new(private_context());
        let mut slot = TextSlot::new(Some(handle.cell()));
        let lent = slot.lend();
        assert!(
            handle.cell().try_borrow_mut().is_err(),
            "the slot lends the shared context"
        );
        drop(lent);
        assert!(
            handle.cell().try_borrow_mut().is_ok(),
            "the loan ends with the TextCx"
        );
    }

    #[test]
    fn a_slot_without_a_source_lends_one_context_of_its_own() {
        let mut slot = TextSlot::new(None);
        let first = std::ptr::from_ref(&*slot.lend()).addr();
        let second = std::ptr::from_ref(&*slot.lend()).addr();
        assert_eq!(first, second, "built once, on first use");
    }
}
