//! What a `DISPATCH_SOURCE_TYPE_MEMORYPRESSURE` source reports, as a
//! [`MemoryPressureLevel`].
//!
//! The source's data is a mask of the pressure states the system has entered
//! since the handler last ran.

use flui_foundation::memory::MemoryPressureLevel;

/// `DISPATCH_MEMORYPRESSURE_NORMAL`: pressure ended.
pub(crate) const NORMAL: usize = 0x01;
/// `DISPATCH_MEMORYPRESSURE_WARN`: the system is low on memory.
pub(crate) const WARN: usize = 0x02;
/// `DISPATCH_MEMORYPRESSURE_CRITICAL`: the system is about to terminate
/// processes.
pub(crate) const CRITICAL: usize = 0x04;

/// The states a source subscribes to. Pressure ending is subscribed to as well,
/// so the source is never silent about it.
pub(crate) const SUBSCRIBED: [usize; 3] = [NORMAL, WARN, CRITICAL];

/// The level of pressure a source's data names; `None` for pressure ending or a
/// mask with no state this crate knows.
///
/// A mask can hold several states when the handler ran late; the worst wins.
pub(crate) const fn level_of(flags: usize) -> Option<MemoryPressureLevel> {
    if flags & CRITICAL != 0 {
        Some(MemoryPressureLevel::Critical)
    } else if flags & WARN != 0 {
        Some(MemoryPressureLevel::Moderate)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_subscribed_state_but_the_ending_is_a_level() {
        for state in SUBSCRIBED {
            assert_eq!(level_of(state).is_some(), state != NORMAL, "{state:#x}");
        }
    }

    #[test]
    fn a_source_mask_becomes_a_level() {
        // (name, mask, level)
        let rows = [
            ("critical", CRITICAL, Some(MemoryPressureLevel::Critical)),
            ("warn", WARN, Some(MemoryPressureLevel::Moderate)),
            (
                "warn then critical before the handler ran",
                WARN | CRITICAL,
                Some(MemoryPressureLevel::Critical),
            ),
            (
                "critical then back to normal",
                CRITICAL | NORMAL,
                Some(MemoryPressureLevel::Critical),
            ),
            (
                "warn then back to normal",
                WARN | NORMAL,
                Some(MemoryPressureLevel::Moderate),
            ),
            ("pressure ended", NORMAL, None),
            ("nothing", 0, None),
            ("a state this crate does not know", 0x08, None),
        ];
        for (name, mask, expected) in rows {
            assert_eq!(level_of(mask), expected, "{name}");
        }
    }
}
