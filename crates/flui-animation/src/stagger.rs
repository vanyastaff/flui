//! Per-index delays for staggered motion.

use std::time::Duration;

/// A delay that grows by `step` per index away from an origin.
///
/// Element `i` of `count` waits `step · |origin − i|`. One controller drives
/// every element: read a [`Keyframes`](crate::Keyframes) track at
/// `elapsed − delay` for a one-shot stagger, or at
/// `elapsed + total − delay` with
/// [`value_at_looped`](crate::Keyframes::value_at_looped) for a cycle.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use flui_animation::{Stagger, StaggerOrigin};
///
/// let ms = Duration::from_millis;
/// let stagger = Stagger::new(ms(100), StaggerOrigin::Center);
/// // Five elements: the middle one starts first, the ends last.
/// assert_eq!(stagger.delay(2, 5), ms(0));
/// assert_eq!(stagger.delay(0, 5), ms(200));
/// // Four elements: the centre lies between indices 1 and 2.
/// assert_eq!(stagger.delay(1, 4), ms(50));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stagger {
    step: Duration,
    origin: StaggerOrigin,
}

/// The index a [`Stagger`] counts its delays from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StaggerOrigin {
    /// Index 0 starts first.
    First,
    /// The last index, `count − 1`, starts first.
    Last,
    /// The middle, `(count − 1) / 2`, starts first; for an even count it
    /// lies between two indices, which both wait half a step.
    Center,
    /// The given index starts first. It may lie outside `0..count`.
    Index(usize),
}

impl Stagger {
    /// A stagger of `step` per index away from `origin`.
    #[must_use]
    pub const fn new(step: Duration, origin: StaggerOrigin) -> Self {
        Self { step, origin }
    }

    /// The delay of element `index` among `count`: `step · |origin − index|`,
    /// computed exactly in nanoseconds and saturating at [`Duration::MAX`].
    #[must_use]
    pub fn delay(&self, index: usize, count: usize) -> Duration {
        // Work in half-steps so a centre between two indices stays exact.
        let index = index as u128 * 2;
        let origin = match self.origin {
            StaggerOrigin::First => 0,
            StaggerOrigin::Last => count.saturating_sub(1) as u128 * 2,
            StaggerOrigin::Center => count.saturating_sub(1) as u128,
            StaggerOrigin::Index(origin) => origin as u128 * 2,
        };
        let half_steps = origin.abs_diff(index);
        let nanos = self.step.as_nanos().saturating_mul(half_steps) / 2;
        let seconds = nanos / 1_000_000_000;
        match u64::try_from(seconds) {
            Ok(seconds) => {
                let subsec = u32::try_from(nanos % 1_000_000_000)
                    .expect("BUG: a remainder of 1e9 fits in u32");
                Duration::new(seconds, subsec)
            }
            Err(_) => Duration::MAX,
        }
    }
}
