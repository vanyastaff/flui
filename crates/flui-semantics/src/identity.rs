//! Stable identity for nodes exported to accessibility adapters.

use core::{fmt, num::NonZeroU64};

use flui_foundation::RenderId;

/// Stable OS-facing identity for one semantics boundary.
///
/// This is deliberately distinct from `SemanticsId`: `SemanticsId` locates a
/// node in the semantics arena rebuilt by the rendering pipeline, while this
/// value follows the generational [`RenderId`] of the render object that forms
/// the boundary. Configuration changes and sibling reordering therefore keep
/// the same accessibility identity, while removal and slab-slot reuse mint a
/// different value.
///
/// The all-zero value is unrepresentable. No process-global allocator is used;
/// the render tree remains the sole identity authority.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccessibilityNodeId(NonZeroU64);

impl AccessibilityNodeId {
    /// Returns the packed non-zero value exposed at the platform boundary.
    #[inline]
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0.get()
    }
}

impl From<RenderId> for AccessibilityNodeId {
    #[inline]
    fn from(render_id: RenderId) -> Self {
        Self(
            NonZeroU64::new(render_id.as_u64())
                .expect("BUG: a RenderId generation makes its packed value non-zero"),
        )
    }
}

impl fmt::Display for AccessibilityNodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU32;

    use super::*;

    #[test]
    fn conversion_preserves_the_full_generational_render_id() {
        let render_id =
            RenderId::new_gen(7, NonZeroU32::new(3).expect("test generation is non-zero"));
        let accessibility_id = AccessibilityNodeId::from(render_id);

        assert_eq!(accessibility_id.as_u64(), render_id.as_u64());
    }

    #[test]
    fn recycled_slot_generations_produce_distinct_accessibility_ids() {
        let first = RenderId::new_gen(7, NonZeroU32::new(3).expect("test generation is non-zero"));
        let recycled =
            RenderId::new_gen(7, NonZeroU32::new(4).expect("test generation is non-zero"));

        assert_ne!(
            AccessibilityNodeId::from(first),
            AccessibilityNodeId::from(recycled),
        );
    }

    #[test]
    fn optional_identity_uses_the_non_zero_niche() {
        assert_eq!(
            core::mem::size_of::<AccessibilityNodeId>(),
            core::mem::size_of::<Option<AccessibilityNodeId>>(),
        );
    }
}
