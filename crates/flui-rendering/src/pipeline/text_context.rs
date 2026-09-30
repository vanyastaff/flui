//! The realm's text context, lent to layout (ADR-0092 §10 step 3).
//!
//! A realm owns one [`TextContext`] over the app's font collection and every
//! presentation's [`PipelineOwner`](super::PipelineOwner) holds a
//! [`TextContextHandle`] to it. Layout, intrinsic, dry-layout and
//! dry-baseline contexts lend it to a render object as a [`TextCx`], a scoped
//! `&mut TextContext` taken from `&mut` context, so a render object cannot
//! hold two loans at once through the context API. The raw
//! [`RenderObject`](crate::traits::RenderObject) methods and the erased layout
//! context carry the context as a [`TextSource`], an opaque token only this
//! crate can borrow, so a direct `RenderObject` implementation cannot hold a
//! loan across a child query either.
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
/// Cloning shares the context. `!Send`, like the realm and pipeline owners
/// that hold it. A render object measures through the [`TextCx`] its layout
/// context lends; `ptr_eq` and `with`, under the `testing` feature, let a
/// test compare handles and inspect the context.
#[derive(Clone)]
pub struct TextContextHandle(Rc<RefCell<TextContext>>);

impl TextContextHandle {
    /// A handle over `context`.
    #[must_use]
    pub fn new(context: TextContext) -> Self {
        Self(Rc::new(RefCell::new(context)))
    }

    /// A context over a font collection of its own, holding only the bundled
    /// faces, for a pipeline that is a realm by itself: a hot-reload plugin
    /// image, or a test with no app behind it. A realm's presentations share
    /// the context the realm builds over the app's collection instead, so a
    /// face the app registers reaches their layout.
    #[must_use]
    pub fn standalone() -> Self {
        Self::new(TextContext::new(&FontCollection::new()))
    }

    /// The source a layout or query context lends this context through, so a
    /// test can drive a raw entry point such as
    /// [`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only).
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn source(&self) -> TextSource<'_> {
        TextSource::new(&self.0)
    }

    /// Whether `a` and `b` share one context.
    #[cfg(any(test, feature = "testing"))]
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
    #[cfg(any(test, feature = "testing"))]
    pub fn with<R>(&self, f: impl FnOnce(&mut TextContext) -> R) -> R {
        f(&mut lend(TextSource::new(&self.0)))
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

/// The realm's text context as a layout or query walk carries it to a node.
///
/// Opaque outside this crate: a render object passes it on (to a child's
/// raw query, or to a context it builds) but cannot borrow it. The borrow
/// happens only inside a context's `text()`, which ties the loan to `&mut`
/// context, so no loan can outlive a measurement or span a child's.
///
/// ```compile_fail
/// fn hold(source: flui_rendering::TextSource<'_>) {
///     // The cell is crate-private: a raw render object cannot borrow it.
///     let _loan = source.cell().borrow_mut();
/// }
/// ```
#[derive(Clone, Copy)]
pub struct TextSource<'a>(&'a RefCell<TextContext>);

impl<'a> TextSource<'a> {
    /// A source over the pipeline's shared cell.
    pub(crate) fn new(cell: &'a RefCell<TextContext>) -> Self {
        Self(cell)
    }

    /// The cell a context borrows from.
    pub(crate) fn cell(self) -> &'a RefCell<TextContext> {
        self.0
    }
}

impl fmt::Debug for TextSource<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TextSource")
            .field(&std::ptr::from_ref(self.0))
            .finish()
    }
}

/// The realm's text context lent to one measurement.
///
/// Dereferences to `&mut TextContext`, so a render object passes
/// `&mut ctx.text()` straight to `TextPainter`.
pub struct TextCx<'a>(RefMut<'a, TextContext>);

impl Deref for TextCx<'_> {
    type Target = TextContext;

    fn deref(&self) -> &TextContext {
        &self.0
    }
}

impl DerefMut for TextCx<'_> {
    fn deref_mut(&mut self) -> &mut TextContext {
        &mut self.0
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
pub(crate) fn lend(source: TextSource<'_>) -> TextCx<'_> {
    TextCx(
        source
            .cell()
            .try_borrow_mut()
            .expect("BUG: the realm's text context is lent to one measurement at a time"),
    )
}

#[cfg(test)]
mod tests {
    use static_assertions::assert_not_impl_any;

    use super::{TextContextHandle, lend};

    assert_not_impl_any!(TextContextHandle: Send, Sync);

    #[test]
    fn a_clone_shares_the_context() {
        let handle = TextContextHandle::standalone();
        assert!(TextContextHandle::ptr_eq(&handle, &handle.clone()));
        assert!(!TextContextHandle::ptr_eq(
            &handle,
            &TextContextHandle::standalone()
        ));
    }

    #[test]
    fn a_loan_holds_the_shared_context_until_it_drops() {
        let handle = TextContextHandle::standalone();
        let lent = lend(handle.source());
        assert!(
            handle.cell().try_borrow_mut().is_err(),
            "the loan borrows the shared context"
        );
        drop(lent);
        assert!(
            handle.cell().try_borrow_mut().is_ok(),
            "the loan ends with the TextCx"
        );
    }
}
