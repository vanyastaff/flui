//! [`SliverFixedExtentList`] — a sliver that lays out box children one after
//! another, each given the same fixed main-axis extent, building only the
//! ones its window needs.

use std::fmt;
use std::rc::Rc;

use flui_view::element::{SemanticSetMapping, StaticChildren};
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BuildContext, IntoView};

/// A sliver that places its box children sequentially along the scroll axis,
/// each occupying the same `item_extent` — cheaper to lay out than measuring
/// every child, the backbone of a fixed-row-height [`ListView`](crate::ListView).
///
/// The children are handed to the element tree as a static delegate:
/// only the ones inside the viewport's
/// cache window are built, keyed children are found by their key when the
/// list is reordered, and everything outside the window is evicted.
///
/// Backed by `RenderSliverFixedExtentList`. Lives inside a
/// [`Viewport`](crate::Viewport).
#[derive(Clone, StatelessView)]
pub struct SliverFixedExtentList {
    item_extent: f64,
    source: Source,
    /// How this sliver's children number themselves for a screen reader.
    /// `None` leaves the adaptor's default — every child a member, at its own
    /// logical index.
    semantics: Option<SemanticSetMapping>,
}

/// Where the children come from.
#[derive(Clone)]
enum Source {
    /// A fixed list, served lazily by index.
    Static(Rc<StaticChildren>),
    /// Built on demand up to `item_count`.
    Builder {
        item_count: usize,
        builder: Rc<dyn Fn(usize) -> Option<flui_view::BoxedView>>,
    },
}

impl SliverFixedExtentList {
    /// A fixed-extent sliver list: every child gets `item_extent` on the scroll
    /// axis.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent` is not finite or not greater than zero.
    pub fn new(item_extent: f64, children: impl ViewSeq) -> Self {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and positive, got {item_extent}",
        );
        Self {
            item_extent,
            source: Source::Static(StaticChildren::new(children.into_boxed_vec())),
            semantics: None,
        }
    }

    /// The same list over an already shared delegate — two views built over
    /// one delegate compare as unchanged on update, so the resident children
    /// are not rebuilt.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent` is not finite or not greater than zero.
    #[must_use]
    pub fn over(item_extent: f64, children: Rc<StaticChildren>) -> Self {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and positive, got {item_extent}",
        );
        Self {
            item_extent,
            source: Source::Static(children),
            semantics: None,
        }
    }

    /// A fixed-extent list of up to `item_count` children built on demand;
    /// the builder answers `None` at the end of the data. Pass `usize::MAX` for a count the
    /// builder alone knows: the first `None` clamps it.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent` is not finite or not greater than zero.
    pub fn builder<F>(item_extent: f64, item_count: usize, builder: F) -> Self
    where
        F: Fn(usize) -> Option<flui_view::BoxedView> + 'static,
    {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and positive, got {item_extent}",
        );
        Self {
            item_extent,
            source: Source::Builder {
                item_count,
                builder: Rc::new(builder),
            },
            semantics: None,
        }
    }

    /// Declare how these children number themselves for a screen reader.
    ///
    /// The reference spells this as two separate `SliverChildDelegate`
    /// knobs, `semanticIndexCallback` and `semanticIndexOffset`. They are one
    /// value here on purpose: an offset without the composed set's size
    /// announces "item 12 of 5" for the second delegate in a pair, and the
    /// two are only ever correct when chosen together.
    ///
    /// Left unset, every child is a member at its own logical index, in a set
    /// as large as the child count — the reference's default. Note the two
    /// numbering bases: a semantic index is 0-based, and the
    /// `position_in_set` a screen reader is handed is 1-based, so the first
    /// child of three announces as "1 of 3".
    #[must_use]
    pub fn semantics(mut self, semantics: SemanticSetMapping) -> Self {
        self.semantics = Some(semantics);
        self
    }

    /// The per-child main-axis extent.
    #[must_use]
    pub const fn item_extent(&self) -> f64 {
        self.item_extent
    }
}

impl fmt::Debug for SliverFixedExtentList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("SliverFixedExtentList");
        s.field("item_extent", &self.item_extent);
        match &self.source {
            Source::Static(children) => s.field("children", &children.len()),
            Source::Builder { item_count, .. } => s.field("item_count", item_count),
        };
        s.field("semantics", &self.semantics);
        s.finish()
    }
}

impl StatelessView for SliverFixedExtentList {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let adaptor = match &self.source {
            Source::Static(children) => {
                flui_view::element::SliverFixedExtentList::over(self.item_extent, children)
            }
            Source::Builder {
                item_count,
                builder,
            } => flui_view::element::SliverFixedExtentList::new(
                self.item_extent,
                *item_count,
                Rc::clone(builder),
            ),
        };
        // Applied after the match so both delegate kinds carry it: a builder
        // delegate numbers its children exactly as a static one does, and the
        // reference's `semanticIndexCallback` is not a property of either.
        match &self.semantics {
            Some(mapping) => adaptor.semantics(mapping.clone()),
            None => adaptor,
        }
    }
}
