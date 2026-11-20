//! Leader/follower links: the identity a [`LeaderLayer`] publishes and a
//! [`FollowerLayer`] targets, and what the resolution of a follower's
//! position needs from it (ADR-0276). The retained scene resolves a
//! follower in place ([`SceneVersion::follower_transform`]).
//!
//! [`LeaderLayer`]: crate::LeaderLayer
//! [`FollowerLayer`]: crate::FollowerLayer
//! [`SceneVersion::follower_transform`]: crate::SceneVersion::follower_transform

use std::num::NonZeroU64;

use flui_foundation::AtomicIdMint;
use flui_foundation::geometry::Matrix4;

/// The identity shared by one leader and any number of followers.
///
/// Minted from a process-wide mint: a link is a `Copy` token a widget in one
/// UI runtime may hand to a widget in another, so a UI runtime-scoped counter
/// would invite collisions where a global one cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerLink {
    id: NonZeroU64,
}

/// Links, minted from every thread that builds a widget holding one.
static NEXT_LINK: AtomicIdMint<NonZeroU64> = AtomicIdMint::new();

impl LayerLink {
    /// Mints a link no other call has returned.
    ///
    /// # Panics
    ///
    /// After 2^64 - 1 links in one process, which a process minting one a
    /// nanosecond reaches in 584 years; the mint never wraps, so a link is
    /// never issued twice.
    #[expect(
        clippy::new_without_default,
        reason = "a Default that minted a fresh unique link per call would read as the zero value"
    )]
    pub fn new() -> Self {
        Self {
            id: NEXT_LINK
                .next()
                .expect("BUG: a process mints fewer than 2^64 - 1 layer links"),
        }
    }

    /// The raw identity, for diagnostics.
    #[inline]
    pub fn id(&self) -> u64 {
        self.id.get()
    }
}

/// How many followers may stand on one another's chains before resolution
/// gives up: a menu in a menu in a menu is a handful, and a cycle (two
/// followers each on the other's leader chain) would never end.
pub(crate) const MAX_FOLLOWER_NESTING: usize = 16;

/// The inverse of `matrix`, kept in the 2D affine shape when `matrix` has
/// it.
///
/// The general 4x4 inverse divides by the determinant everywhere, so the
/// inverse of a scale of 7 holds `49 * (1/49)` where the affine shape needs
/// an exact `1` and `0`, and a consumer that admits only affine transforms
/// (the GPU painter) would refuse an ordinary scaled chain. Products of
/// affine matrices keep the shape exactly, so only the inverse needs this.
pub(crate) fn invert(matrix: &Matrix4) -> Option<Matrix4> {
    let coefficients = &matrix.m;
    let affine = [2, 3, 6, 7, 8, 9, 11, 14]
        .iter()
        .all(|&index| coefficients[index] == 0.0)
        && coefficients[10] == 1.0
        && coefficients[15] == 1.0;
    if !affine {
        return matrix.try_inverse();
    }
    // Column-major: x' = xx·x + yx·y + tx, y' = xy·x + yy·y + ty.
    let [xx, xy, yx, yy, tx, ty] = [0, 1, 4, 5, 12, 13].map(|index| coefficients[index]);
    let determinant = xx * yy - xy * yx;
    if determinant == 0.0 || !determinant.is_finite() {
        return None;
    }
    let [inv_xx, inv_xy, inv_yx, inv_yy] = [yy, -xy, -yx, xx].map(|value| value / determinant);
    let mut inverse = Matrix4::IDENTITY;
    inverse.m[0] = inv_xx;
    inverse.m[1] = inv_xy;
    inverse.m[4] = inv_yx;
    inverse.m[5] = inv_yy;
    inverse.m[12] = -(inv_xx * tx + inv_yx * ty);
    inverse.m[13] = -(inv_xy * tx + inv_yy * ty);
    inverse
        .m
        .iter()
        .all(|value| value.is_finite())
        .then_some(inverse)
}
