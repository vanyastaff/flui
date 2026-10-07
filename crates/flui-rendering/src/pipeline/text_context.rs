//! The UI runtime's text context, lent to layout (ADR-0092 §10 step 3).
//!
//! A UI runtime owns one [`TextContext`] over the app's font collection and every
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
//! The one `RefCell` sits between the UI runtime and its pipelines, not on a
//! render object: it is borrowed once per measurement, on the owner thread,
//! and a second borrow at the same time is a bug (`BUG:` panic), not a
//! contended lock. A presentation's layout never drives another's
//! synchronously, since a `PipelineCell` checkout is not re-entrant.
//!
//! Every loan a pipeline's walk makes records the node it was made for
//! ([`TextMeasurers`]): those are the nodes whose layout depends on the font
//! collection, and the ones the pipeline lays out again when a face is
//! registered on it (`PipelineOwner::apply_font_change`). A render object
//! needs no code for this; measuring through the context is the opt-in.

use std::cell::{RefCell, RefMut};
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use flui_foundation::RenderId;
use flui_painting::{FontCollection, TextContext};
use rustc_hash::FxHashSet;

/// A UI runtime's text context, shared with each presentation's pipeline.
///
/// Cloning shares the context. `!Send`, like the UI runtime and pipeline owners
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
    /// faces, for a pipeline that is a UI runtime by itself: a hot-reload plugin
    /// image, or a test with no app behind it. A UI runtime's presentations share
    /// the context the UI runtime builds over the app's collection instead, so a
    /// face the app registers reaches their layout.
    #[must_use]
    pub fn standalone() -> Self {
        Self::new(TextContext::new(&FontCollection::new()))
    }

    /// The source a layout or query context lends this context through, so a
    /// test can drive a raw entry point such as
    /// [`RenderEntry::layout_leaf_only`](crate::storage::RenderEntry::layout_leaf_only).
    /// A loan through it records no node.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn source(&self) -> TextSource<'_> {
        TextSource::unrecorded(&self.0)
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
    /// re-enters another on the same UI runtime could cause.
    #[cfg(any(test, feature = "testing"))]
    pub fn with<R>(&self, f: impl FnOnce(&mut TextContext) -> R) -> R {
        f(&mut lend(TextSource::unrecorded(&self.0)))
    }

    /// Runs `f` on the context, or returns `None` while it is lent: a caller
    /// outside layout, such as the scene assembly that shapes the performance
    /// overlay's readout, skips its work rather than wait or panic.
    ///
    /// The loan records no node, so a font change lays nothing out again on
    /// its account: a caller that shapes through it reshapes on its own
    /// schedule.
    pub fn try_with<R>(&self, f: impl FnOnce(&mut TextContext) -> R) -> Option<R> {
        let mut context = self.0.try_borrow_mut().ok()?;
        Some(f(&mut context))
    }

    /// The shared cell layout borrows from.
    pub(crate) fn cell(&self) -> &RefCell<TextContext> {
        &self.0
    }

    /// The generation of the collection the context was built over, or
    /// `None` while the context is lent: a caller outside a measurement
    /// asks again later rather than wait or panic.
    pub(crate) fn fonts_generation(&self) -> Option<u64> {
        self.0
            .try_borrow()
            .ok()
            .map(|context| context.fonts().generation())
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

/// The nodes of one pipeline that measured through its text context since
/// the collection last changed.
///
/// Owned by the pipeline, since render ids are per pipeline; one `insert` per
/// loan. A node stays recorded until the next change takes the set, so a
/// node that stopped measuring text is laid out once more than it needs at
/// worst. A removed node's id is never reused (ids are generational), so
/// [`Self::prune`] drops the dead ones once they outnumber the live tree:
/// without it an app that never registers a font would keep the id of every
/// text node it ever built.
#[derive(Default)]
pub(crate) struct TextMeasurers(RefCell<FxHashSet<RenderId>>);

/// The record size below which [`TextMeasurers::prune`] never walks it.
const PRUNE_FLOOR: usize = 64;

impl TextMeasurers {
    fn note(&self, node: RenderId) {
        self.0.borrow_mut().insert(node);
    }

    /// Every recorded node, leaving the record empty.
    pub(crate) fn take(&self) -> FxHashSet<RenderId> {
        std::mem::take(&mut *self.0.borrow_mut())
    }

    /// Drops every recorded node `is_live` rejects, once the record holds
    /// more than twice `live_nodes` (and more than a small floor).
    ///
    /// Amortized: a walk halves the record at least, so the next one waits
    /// until as many ids were added again. The record stays within twice the
    /// live tree plus what one frame measures.
    pub(crate) fn prune(&self, live_nodes: usize, is_live: impl Fn(RenderId) -> bool) {
        let mut set = self.0.borrow_mut();
        if set.len() > PRUNE_FLOOR.max(live_nodes.saturating_mul(2)) {
            set.retain(|id| is_live(*id));
        }
    }

    /// How many nodes are recorded.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn len(&self) -> usize {
        self.0.borrow().len()
    }
}

impl fmt::Debug for TextMeasurers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TextMeasurers")
            .field(&self.0.try_borrow().map(|set| set.len()).ok())
            .finish()
    }
}

/// A pipeline's text context and the record its loans go into, as a layout
/// or query walk carries them: it mints a [`TextSource`] per node.
#[derive(Clone, Copy)]
pub(crate) struct TextLender<'a> {
    cell: &'a RefCell<TextContext>,
    measurers: &'a TextMeasurers,
}

impl<'a> TextLender<'a> {
    pub(crate) fn new(cell: &'a RefCell<TextContext>, measurers: &'a TextMeasurers) -> Self {
        Self { cell, measurers }
    }

    /// The source `node`'s contexts lend through; a loan records `node`.
    pub(crate) fn source(self, node: RenderId) -> TextSource<'a> {
        TextSource {
            cell: self.cell,
            measured: Some((self.measurers, node)),
        }
    }
}

/// The UI runtime's text context as a layout or query walk carries it to a node.
///
/// Opaque outside this crate: a render object passes it on (to a context it
/// builds) but cannot borrow it. The borrow happens only inside a context's
/// `text()`, which ties the loan to `&mut` context, so no loan can outlive a
/// measurement or span a child's.
///
/// ```compile_fail
/// fn hold(source: flui_rendering::TextSource<'_>) {
///     // The cell is crate-private: a raw render object cannot borrow it.
///     let _loan = source.cell().borrow_mut();
/// }
/// ```
#[derive(Clone, Copy)]
pub struct TextSource<'a> {
    cell: &'a RefCell<TextContext>,
    /// The record a loan goes into and the node it is made for; `None` for a
    /// test's source, which belongs to no walk.
    measured: Option<(&'a TextMeasurers, RenderId)>,
}

impl<'a> TextSource<'a> {
    /// A source whose loans record nothing.
    #[cfg(any(test, feature = "testing"))]
    fn unrecorded(cell: &'a RefCell<TextContext>) -> Self {
        Self {
            cell,
            measured: None,
        }
    }

    /// The cell a context borrows from.
    pub(crate) fn cell(self) -> &'a RefCell<TextContext> {
        self.cell
    }
}

impl fmt::Debug for TextSource<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextSource")
            .field("context", &std::ptr::from_ref(self.cell))
            .field("node", &self.measured.map(|(_, node)| node))
            .finish()
    }
}

/// The UI runtime's text context lent to one measurement.
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

/// Borrows the UI runtime's context for one measurement, and records the node it
/// is lent to.
#[expect(
    clippy::expect_used,
    reason = "a second loan is a re-entrant measurement, an invariant violation"
)]
pub(crate) fn lend(source: TextSource<'_>) -> TextCx<'_> {
    if let Some((measurers, node)) = source.measured {
        measurers.note(node);
    }
    TextCx(
        source
            .cell()
            .try_borrow_mut()
            .expect("BUG: the ui_runtime's text context is lent to one measurement at a time"),
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

    /// `try_with` declines while a measurement holds the context, rather
    /// than panic, and runs once the loan ends.
    #[test]
    fn try_with_declines_while_the_context_is_lent() {
        let handle = TextContextHandle::standalone();
        let lent = lend(handle.source());
        assert_eq!(handle.try_with(|_| ()), None, "the context is lent");
        drop(lent);
        assert_eq!(handle.try_with(|_| 7), Some(7), "the loan has ended");
    }
}
