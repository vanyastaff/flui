//! The identity a repaint boundary stamps on its layer: which boundary, and
//! which version of its content.

use std::fmt;
use std::sync::Arc;

use flui_foundation::RenderId;

/// One version of a repaint boundary's painted content.
///
/// The paint pass certifies it: a boundary keeps its token for exactly as long
/// as it is neither queued for paint nor for a layer update and still has a
/// retained capture, and receives a freshly minted one otherwise
/// (`flui-rendering`'s `ARCHITECTURE.md`, "Paint certifies a boundary's content
/// token"). Two tokens compare equal only when they are clones of one mint, so
/// equality means "the paint pass vouched that nothing under this boundary's
/// own region changed".
///
/// Pointer identity of the pictures cannot answer that question: an outer
/// boundary re-records its inline pictures whenever a nested boundary is
/// dirty, so its `Arc`s change on frames whose content did not.
///
/// An `Arc` allocation rather than a counter: a counter restarts with every
/// `PipelineOwner`, and a restarted count would let a new tree's first version
/// compare equal to an old tree's. An allocation cannot be reused while any
/// clone is alive, and a differ holds the previous frame's clones for as long
/// as it compares against them, so a token can never alias one it is being
/// compared with.
#[derive(Clone)]
pub struct ContentToken(Arc<()>);

impl ContentToken {
    /// A token equal to no other token alive.
    #[must_use]
    pub fn mint() -> Self {
        Self(Arc::new(()))
    }
}

impl PartialEq for ContentToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ContentToken {}

impl fmt::Debug for ContentToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentToken({:p})", Arc::as_ptr(&self.0))
    }
}

/// The stamp a repaint boundary's layer carries: the boundary's [`RenderId`]
/// and the [`ContentToken`] of the content it painted.
///
/// The id pairs the layer with the previous frame's (every frame's `LayerId`s
/// are fresh); the token says whether its content changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryStamp {
    render_id: RenderId,
    content: ContentToken,
}

impl BoundaryStamp {
    /// A stamp for `render_id`'s content version `content`.
    #[must_use]
    pub fn new(render_id: RenderId, content: ContentToken) -> Self {
        Self { render_id, content }
    }

    /// The boundary that painted the layer.
    #[inline]
    #[must_use]
    pub fn render_id(&self) -> RenderId {
        self.render_id
    }

    /// The version of its content.
    #[inline]
    #[must_use]
    pub fn content(&self) -> &ContentToken {
        &self.content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_equal_only_as_clones_of_one_mint() {
        let a = ContentToken::mint();
        let b = ContentToken::mint();
        assert_eq!(a, a.clone());
        assert_ne!(a, b);
    }
}
