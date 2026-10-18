//! The display contract: an immutable snapshot of every display, the
//! framework-issued identity of each, and the refresh period of its current
//! mode (ADR-0236).
//!
//! A platform samples the OS into a [`DisplaySnapshot`]; the host commits it and
//! derives what each window needs from it ([`WindowDisplay`]). Nothing here
//! queries the OS, so a snapshot can be read from any thread and compared for
//! equality.
//!
//! # Identity
//!
//! A [`DisplayId`] is opaque and non-zero. It is issued by a [`DisplayRegistry`]
//! from the operating system's own identity for the output ([`DisplayKey`]),
//! never reused for another display and never wrapped. An id is meaningful only
//! within the platform instance (the one registry) that issued it: an id carries
//! its registry, so ids from different instances never compare equal and never
//! resolve against another instance's snapshot, and ids from different runs mean
//! nothing to each other. The same physical connector
//! keeps its id when it is unplugged and plugged back in during the session; two
//! identical monitors differ by connector; swapping the cables swaps the
//! identities; an output with no stable native key gets a fresh id each time it
//! appears. An identity that survives a restart is deliberately absent: it
//! belongs with window placement restore, and [`DisplayInfo`] is
//! `#[non_exhaustive]` so it can gain one without breaking this contract.
//!
//! # Refresh period
//!
//! [`DisplayInfo::refresh_period`] is the period of the mode the display is
//! running, never its maximum, and a variable-rate range is not modelled. A
//! value that does not describe a cadence (zero, a zero denominator, a
//! non-positive rate, or a period outside 1 ms to 1 s) is recorded as unknown
//! (`None`): [`RefreshPeriod`] cannot hold it. The snapshot never substitutes
//! 60 Hz; that fallback is a consumer's policy. Where a platform has no native
//! source and estimates the period, it says so in its own documentation.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::UniqueKey;
use flui_foundation::geometry::Bounds;
use parking_lot::Mutex;

/// A display's identity within one platform instance.
///
/// Opaque and non-zero: it cannot be built from a number, only issued by a
/// [`DisplayRegistry`]. It carries its issuing registry, so an id from another
/// registry never equals, hashes with or resolves against this one's: two
/// registries both issue number 1 first, and those two ids differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayId {
    number: NonZeroU64,
    registry: UniqueKey,
}

impl DisplayId {
    /// The issued number, for diagnostics and logging. It is unique within the
    /// issuing registry and means nothing outside it; equal numbers from two
    /// registries are two different ids.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.number.get()
    }
}

/// The operating system's own identity for one output, from which a
/// [`DisplayRegistry`] issues a [`DisplayId`].
///
/// A key is an identity, not a label: two outputs with the same name are two
/// keys. Each variant names the source it comes from, and a variant exists only
/// once a backend produces it: the Windows, macOS, Wayland and X11 keys arrive
/// with their native sources.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DisplayKey {
    /// `Display.getDisplayId` on Android. The OS does not guarantee it across
    /// the reconnection of an external screen, which then gets a new id.
    AndroidId(i32),
    /// A number the iOS source assigns to a `UIScreen` it retains. The screen
    /// is matched to its number by object identity only while the source holds
    /// it, never by address: a released screen's address can be reused by
    /// another screen.
    IosScreen(u64),
    /// The UUID macOS gives a display (`CGDisplayCreateUUIDFromDisplayID`), 16
    /// bytes in RFC 4122 order. It names the display itself, so it survives a
    /// reconnection that hands the display a new `CGDirectDisplayID`.
    MacosUuid([u8; 16]),
    /// A Web display; the retained JS screen object stands behind the string.
    Web(String),
    /// A Windows display target from `QueryDisplayConfig`: the adapter's LUID
    /// (`HighPart` in the upper 32 bits, `LowPart` in the lower) and the target
    /// id on it. A LUID changes across reboots and GPU resets, which a key that
    /// only lives for the session tolerates.
    WindowsTarget {
        /// The adapter LUID, packed as `(HighPart << 32) | LowPart`.
        adapter_luid: u64,
        /// The target id on that adapter.
        target_id: u32,
    },
    /// The monitor interface path Windows reports for a monitor that has no
    /// `QueryDisplayConfig` path (a remote session, a driver without paths).
    WindowsInterface(String),
    /// A value injected by a test platform.
    Injected(u64),
    /// A stable string a window backend offers for its monitors. A label, not
    /// an identity: it exists for backends that have no better source yet and
    /// is replaced when they gain one.
    Backend(String),
    /// An output with no stable native key: every appearance is a new display.
    Unkeyed,
}

/// The id space of a [`DisplayRegistry`] is used up. Permanent: a registry
/// never wraps and never reissues an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("display ids exhausted")]
pub struct DisplayIdsExhausted;

/// Issues [`DisplayId`]s for one platform instance, keyed by native identity.
///
/// Send and Sync: a native source may mint from a worker thread. The lock
/// covers a map lookup and a counter step only; it guards infrastructure, not
/// frame state.
#[derive(Debug)]
pub struct DisplayRegistry {
    /// Distinguishes this registry's ids from every other registry's. Minted
    /// from `flui-foundation`'s process-wide unique-key counter, which is
    /// already a reviewed global (ADR-0097), so no state is added here.
    tag: UniqueKey,
    inner: Mutex<RegistryState>,
}

#[derive(Debug)]
struct RegistryState {
    known: HashMap<DisplayKey, DisplayId>,
    /// The next id to issue; `None` once exhausted, for good.
    next: Option<NonZeroU64>,
}

impl DisplayRegistry {
    /// An empty registry; its first id is 1.
    #[must_use]
    pub fn new() -> Self {
        Self::starting_at(NonZeroU64::MIN)
    }

    fn starting_at(first: NonZeroU64) -> Self {
        Self {
            tag: UniqueKey::new(),
            inner: Mutex::new(RegistryState {
                known: HashMap::new(),
                next: Some(first),
            }),
        }
    }

    /// The id for `key`: the one issued earlier for a known key, a fresh one
    /// for a new key, and a fresh one every time for [`DisplayKey::Unkeyed`].
    ///
    /// # Errors
    /// [`DisplayIdsExhausted`] when a fresh id is needed and the id space is
    /// used up, now and for every later call. A key that was already issued an
    /// id keeps resolving to it.
    pub fn mint(&self, key: &DisplayKey) -> Result<DisplayId, DisplayIdsExhausted> {
        let mut state = self.inner.lock();
        if !matches!(key, DisplayKey::Unkeyed)
            && let Some(id) = state.known.get(key)
        {
            return Ok(*id);
        }
        let issued = state.next.ok_or(DisplayIdsExhausted)?;
        state.next = issued.checked_add(1);
        let id = DisplayId {
            number: issued,
            registry: self.tag,
        };
        if !matches!(key, DisplayKey::Unkeyed) {
            state.known.insert(key.clone(), id);
        }
        Ok(id)
    }
}

impl Default for DisplayRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A raw refresh value that does not describe a cadence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("refresh period must be between 1 ms and 1 s")]
pub struct InvalidRefreshPeriod;

/// The period of a display's current mode, validated: between 1 ms and 1 s
/// inclusive (1 to 1000 Hz). Every constructor refuses what does not describe a
/// cadence, so a consumer holding one never re-checks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RefreshPeriod(Duration);

impl RefreshPeriod {
    /// The shortest accepted period (1000 Hz).
    pub const MIN: Duration = Duration::from_millis(1);
    /// The longest accepted period (1 Hz).
    pub const MAX: Duration = Duration::from_secs(1);

    /// The period as a duration.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }

    /// The period of a rate in hertz.
    ///
    /// # Errors
    /// [`InvalidRefreshPeriod`] for a rate that is not finite, not positive, or
    /// whose period is outside 1 ms to 1 s.
    pub fn from_hz(hz: f64) -> Result<Self, InvalidRefreshPeriod> {
        if !hz.is_finite() || hz <= 0.0 {
            return Err(InvalidRefreshPeriod);
        }
        Duration::try_from_secs_f64(1.0 / hz)
            .map_err(|_| InvalidRefreshPeriod)
            .and_then(Self::try_from)
    }

    /// The period of a rational rate, `numerator / denominator` hertz, as the
    /// operating system reports it (Windows) so that 60000/1001 stays exact.
    /// The division runs in 128-bit integers, rounded to the nearest nanosecond.
    ///
    /// # Errors
    /// [`InvalidRefreshPeriod`] for a zero numerator or denominator, or a
    /// period outside 1 ms to 1 s.
    pub fn from_ratio(numerator: u32, denominator: u32) -> Result<Self, InvalidRefreshPeriod> {
        if numerator == 0 || denominator == 0 {
            return Err(InvalidRefreshPeriod);
        }
        Self::from_nanos_ratio(
            u128::from(denominator) * 1_000_000_000,
            u128::from(numerator),
        )
    }

    /// The period of a rate in millihertz, as Wayland and winit report it.
    ///
    /// # Errors
    /// [`InvalidRefreshPeriod`] for a non-positive rate or a period outside
    /// 1 ms to 1 s.
    pub fn from_millihertz(millihertz: i64) -> Result<Self, InvalidRefreshPeriod> {
        let millihertz = u128::try_from(millihertz).map_err(|_| InvalidRefreshPeriod)?;
        if millihertz == 0 {
            return Err(InvalidRefreshPeriod);
        }
        Self::from_nanos_ratio(1_000_000_000_000, millihertz)
    }

    /// `dividend / divisor` nanoseconds, rounded to nearest. Both operands are
    /// products of `u32` and `u64`-range values, so they fit `u128` exactly.
    fn from_nanos_ratio(dividend: u128, divisor: u128) -> Result<Self, InvalidRefreshPeriod> {
        let nanos = dividend
            .checked_add(divisor / 2)
            .ok_or(InvalidRefreshPeriod)?
            / divisor;
        let nanos = u64::try_from(nanos).map_err(|_| InvalidRefreshPeriod)?;
        Self::try_from(Duration::from_nanos(nanos))
    }
}

impl TryFrom<Duration> for RefreshPeriod {
    type Error = InvalidRefreshPeriod;

    fn try_from(period: Duration) -> Result<Self, Self::Error> {
        if (Self::MIN..=Self::MAX).contains(&period) {
            Ok(Self(period))
        } else {
            Err(InvalidRefreshPeriod)
        }
    }
}

/// One display as the operating system describes it right now.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DisplayInfo {
    /// The framework-issued identity.
    pub id: DisplayId,
    /// The display's name, such as "Built-in Display". A label, never an
    /// identity.
    pub name: String,
    /// Bounds in global screen coordinates, in device pixels.
    pub bounds: Bounds<i32>,
    /// The area windows can occupy without being covered by system chrome.
    pub usable_bounds: Bounds<i32>,
    /// Device pixels per logical pixel.
    pub scale_factor: f64,
    /// The period of the mode the display is running; `None` when unknown.
    pub refresh_period: Option<RefreshPeriod>,
    /// Whether this is the primary display. A platform with no notion of
    /// primary (Wayland) marks none.
    pub is_primary: bool,
}

impl DisplayInfo {
    /// A display with its bounds as usable area, an unknown refresh period and
    /// not primary. Refine it with the `with_*` methods.
    #[must_use]
    pub fn new(
        id: DisplayId,
        name: impl Into<String>,
        bounds: Bounds<i32>,
        scale_factor: f64,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            bounds,
            usable_bounds: bounds,
            scale_factor,
            refresh_period: None,
            is_primary: false,
        }
    }

    /// Set the usable area.
    #[must_use]
    pub fn with_usable_bounds(mut self, usable_bounds: Bounds<i32>) -> Self {
        self.usable_bounds = usable_bounds;
        self
    }

    /// Mark the display primary or not.
    #[must_use]
    pub fn with_primary(mut self, is_primary: bool) -> Self {
        self.is_primary = is_primary;
        self
    }

    /// Record a refresh period the OS reports; one outside 1 ms to 1 s leaves
    /// the period unknown.
    #[must_use]
    pub fn with_refresh_period(mut self, period: Duration) -> Self {
        self.refresh_period = RefreshPeriod::try_from(period).ok();
        self
    }
}

/// The monotone version of a [`DisplaySnapshot`]. Never wraps: a source that
/// reaches the last revision refuses further changes instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayRevision(u64);

impl DisplayRevision {
    /// The revision number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// A revision cannot advance: the last one was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("display revisions exhausted")]
pub struct DisplayRevisionExhausted;

/// Every display at one moment, immutable and cheap to clone.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplaySnapshot {
    revision: DisplayRevision,
    displays: Arc<[DisplayInfo]>,
}

impl DisplaySnapshot {
    /// A snapshot with no displays, at revision 0: what a platform that has not
    /// sampled anything yet, or has no source, reports.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            revision: DisplayRevision(0),
            displays: Arc::from([]),
        }
    }

    /// The snapshot that follows this one with `displays`: this very value when
    /// nothing differs, otherwise the next revision.
    ///
    /// # Errors
    /// [`DisplayRevisionExhausted`] when `displays` differ and the revision is
    /// the last one. The revision never wraps.
    pub fn successor(&self, displays: Vec<DisplayInfo>) -> Result<Self, DisplayRevisionExhausted> {
        if *self.displays == *displays {
            return Ok(self.clone());
        }
        Ok(Self {
            revision: self.revision.next().ok_or(DisplayRevisionExhausted)?,
            displays: displays.into(),
        })
    }

    /// This snapshot's version.
    #[must_use]
    pub fn revision(&self) -> DisplayRevision {
        self.revision
    }

    /// Every display, in the order the source listed them.
    #[must_use]
    pub fn displays(&self) -> &[DisplayInfo] {
        &self.displays
    }

    /// The display with this id, if the snapshot holds it.
    #[must_use]
    pub fn get(&self, id: DisplayId) -> Option<&DisplayInfo> {
        self.displays.iter().find(|display| display.id == id)
    }

    /// The primary display, if the platform has one.
    #[must_use]
    pub fn primary(&self) -> Option<&DisplayInfo> {
        self.displays.iter().find(|display| display.is_primary)
    }

    /// What a window on display `id` needs from this snapshot: `None` when the
    /// window names no display or one the snapshot does not hold.
    #[must_use]
    pub fn window_display(&self, id: Option<DisplayId>) -> Option<WindowDisplay> {
        let display = self.get(id?)?;
        Some(WindowDisplay {
            id: display.id,
            refresh_period: display.refresh_period,
        })
    }
}

/// The part of a display a window pacing itself needs: which display it is on
/// and that display's refresh period. The scale factor is not here; it reaches
/// the window through its own metrics observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct WindowDisplay {
    /// The display the window is on.
    pub id: DisplayId,
    /// That display's refresh period; `None` when unknown.
    pub refresh_period: Option<RefreshPeriod>,
}

impl WindowDisplay {
    /// A window display.
    #[must_use]
    pub const fn new(id: DisplayId, refresh_period: Option<RefreshPeriod>) -> Self {
        Self { id, refresh_period }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The id space is used up at its terminal boundary, so no id wraps or
    /// repeats; this needs the private counter.
    #[test]
    fn exhausted_counter_refuses_without_wrapping() {
        let registry = DisplayRegistry::starting_at(NonZeroU64::MAX);
        let last = registry
            .mint(&DisplayKey::Injected(1))
            .expect("the last id is still issued");
        assert_eq!(last.get(), u64::MAX);
        for attempt in 0..3 {
            assert_eq!(
                registry.mint(&DisplayKey::Injected(2)),
                Err(DisplayIdsExhausted),
                "a new key after exhaustion (attempt {attempt}) must be refused for good"
            );
            assert_eq!(
                registry.mint(&DisplayKey::Unkeyed),
                Err(DisplayIdsExhausted),
                "an unkeyed output after exhaustion (attempt {attempt}) must be refused"
            );
        }
        assert_eq!(
            registry.mint(&DisplayKey::Injected(1)),
            Ok(last),
            "a key issued before exhaustion keeps resolving to its id"
        );
    }

    #[test]
    fn exhausted_revision_refuses_without_wrapping() {
        let at_last = DisplaySnapshot {
            revision: DisplayRevision(u64::MAX),
            displays: Arc::from([]),
        };
        let registry = DisplayRegistry::new();
        let id = registry.mint(&DisplayKey::Injected(1)).expect("id");
        let display = DisplayInfo::new(id, "d", Bounds::default(), 1.0);
        assert_eq!(
            at_last.successor(vec![display]),
            Err(DisplayRevisionExhausted),
            "the revision after the last must be refused, not wrapped"
        );
        let unchanged = at_last
            .successor(Vec::new())
            .expect("an unchanged sample needs no new revision");
        assert_eq!(unchanged.revision(), at_last.revision());
    }
}
