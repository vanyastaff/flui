//! Accessible regions authored by one render object.

use flui_foundation::geometry::{Matrix4, Rect};

use crate::{AccessibilityNodeId, SemanticsConfiguration};

/// Logical identity within one render object's before- or after-child region list.
///
/// Use a domain identity, not a label or a position in the returned list. Reordering
/// a retained key preserves its accessibility identity; removing then reinserting
/// it creates a new identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticsRegionKey(pub u64);

/// One independently addressable accessible part of custom content.
#[derive(Debug, Clone)]
pub struct SemanticsRegion {
    /// Identity scoped to the host and the contribution list.
    pub key: SemanticsRegionKey,
    /// Bounds in the region's local coordinate space.
    pub rect: Rect<f64>,
    /// Local-to-host transform; layout position is applied by the pipeline.
    pub transform: Matrix4,
    /// Existing typed semantics attributes and action handlers.
    pub config: SemanticsConfiguration,
}

impl SemanticsRegion {
    /// Creates a region in the host's coordinate space.
    #[must_use]
    pub fn new(key: SemanticsRegionKey, rect: Rect<f64>, config: SemanticsConfiguration) -> Self {
        Self {
            key,
            rect,
            transform: Matrix4::IDENTITY,
            config,
        }
    }
}

/// Ordered contributions around a render object's actual children.
///
/// Each list has its own key namespace. These are flat region lists; nested
/// accessible structure follows the render children and normal merge policies.
#[derive(Debug, Clone, Default)]
pub struct SemanticsRegions {
    /// Content before the real children in accessibility traversal order.
    pub before_children: Vec<SemanticsRegion>,
    /// Content after the real children in accessibility traversal order.
    pub after_children: Vec<SemanticsRegion>,
}

/// An allocated identity for content with no separate render object.
///
/// Obtain values from a presentation-owned [`SemanticsIdentitySource`]. The
/// token cannot alias a generational render identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticsRegionId(AccessibilityNodeId);

impl SemanticsRegionId {
    /// The identity published to accessibility adapters.
    #[must_use]
    pub const fn accessibility_id(self) -> AccessibilityNodeId {
        self.0
    }
}

/// Presentation-local, monotonic identity authority for synthetic nodes.
///
/// Keep this source alive across semantics-owner replacement and accessibility
/// disable/enable transitions. The low 32-bit range is disjoint from RenderId's
/// nonzero generation. Allocated values are never returned to the source.
#[derive(Debug)]
pub struct SemanticsIdentitySource {
    next: u64,
}

impl Default for SemanticsIdentitySource {
    fn default() -> Self {
        Self { next: 1 }
    }
}

/// The presentation has exhausted the reserved synthetic identity space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("synthetic semantics identity space exhausted")]
pub struct SemanticsIdentityExhausted;

impl SemanticsIdentitySource {
    /// Reserves a complete batch or returns an error without consuming any IDs.
    /// Exhaustion never wraps or reissues an earlier identity.
    pub fn allocate(
        &mut self,
        count: usize,
    ) -> Result<Vec<SemanticsRegionId>, SemanticsIdentityExhausted> {
        let count = u64::try_from(count).map_err(|_| SemanticsIdentityExhausted)?;
        let end = self
            .next
            .checked_add(count)
            .ok_or(SemanticsIdentityExhausted)?;
        if end > u64::from(u32::MAX) + 1 {
            return Err(SemanticsIdentityExhausted);
        }
        let ids = (self.next..end)
            .map(|value| {
                SemanticsRegionId(
                    AccessibilityNodeId::from_u64(value)
                        .expect("BUG: synthetic identities start at one"),
                )
            })
            .collect();
        self.next = end;
        Ok(ids)
    }

    /// Spends every remaining identity, so the next reservation fails: how a
    /// test reaches exhaustion, which no practical run does.
    #[cfg(any(test, feature = "testing"))]
    pub fn exhaust(&mut self) {
        self.next = u64::from(u32::MAX) + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The terminal counter cannot be reached through a practical public test.
    #[test]
    fn synthetic_identity_exhaustion_never_wraps_or_partially_reserves() {
        let mut source = SemanticsIdentitySource {
            next: u64::from(u32::MAX),
        };
        assert_eq!(source.allocate(2), Err(SemanticsIdentityExhausted));
        let last = source.allocate(1).expect("the final identity is available");
        assert_eq!(last[0].accessibility_id().as_u64(), u64::from(u32::MAX));
        assert_eq!(source.allocate(1), Err(SemanticsIdentityExhausted));
        assert_eq!(source.allocate(1), Err(SemanticsIdentityExhausted));
        assert_eq!(
            source.allocate(0).expect("empty reservation"),
            [] as [SemanticsRegionId; 0]
        );
    }
}
