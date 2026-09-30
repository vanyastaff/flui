//! [`ConnectionState`] and [`AsyncSnapshot`] — the data model for
//! `FutureBuilder` / `StreamBuilder`.
//!
//! Pure data: no futures, no executor, no widget. The state machine lives here
//! so the builders stay thin, and so `flui-material` never has to
//! re-declare it.
//!
//! The builders themselves do not exist yet; this module is only the state
//! machine they will share.
//!
//! # Design notes
//!
//! | Dynamic-language shape | FLUI | Why |
//! |---|---|---|
//! | `error: Object?` + `stackTrace: StackTrace` | generic `E`, **no stack trace** | Rust has no ambient stack traces on error values. `E` comes from `Future<Output = Result<T, E>>` — errors are in the type, not thrown. An infallible future uses `E = Infallible`. |
//! | `T get requireData` throws | **absent** | It only makes sense where `data` is nullable and there is no `Option`. Use [`AsyncSnapshot::data`] → `Option<&T>` and `expect` at the call site. `docs/PANIC-POLICY.md` reserves panics for internal invariants. |
//! | `AsyncSnapshot` handed to `builder` by value | handed by **reference** | Avoids `T: Clone`. `FOUNDATIONS.md`: "Application state carries no trait bound beyond `'static` — the Druid mistake is the one most dangerous trap." |
//! | `AsyncSnapshot.waiting()` | [`AsyncSnapshot::waiting`] | Same, kept for symmetry even though the folds never need it. |
//!
//! # The data/error invariant
//!
//! A snapshot never holds both data and an error. This is upheld **by
//! construction**: the fields are private, and every constructor and fold sets
//! exactly one of them. [`with_data`](AsyncSnapshot::with_data) clears the error;
//! [`with_error`](AsyncSnapshot::with_error) clears the data.

use core::fmt;

/// The state of connection to an asynchronous computation.
///
/// The usual flow is `None` → `Waiting` → `Active` → `Done`; a `Future` skips
/// `Active`, going straight from `Waiting` to `Done`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ConnectionState {
    /// Not currently connected to any asynchronous computation.
    ///
    /// For example, a `FutureBuilder` whose future is absent — or one whose
    /// future was just replaced, for the instant before it resubscribes.
    #[default]
    None,

    /// Connected to an asynchronous computation, awaiting interaction.
    Waiting,

    /// Connected to an active asynchronous computation.
    ///
    /// A stream that has yielded at least one event but is not yet done. A
    /// future is never `Active`.
    Active,

    /// Connected to a terminated asynchronous computation.
    Done,
}

/// Immutable summary of the most recent interaction with an asynchronous
/// computation.
///
/// Carries a [`ConnectionState`] and **either** data or an error, never both.
/// `T` and `E` need no bounds: reading a snapshot borrows, so neither has to be
/// `Clone`.
///
/// # Example
///
/// ```
/// use flui_foundation::{AsyncSnapshot, ConnectionState};
///
/// let snapshot: AsyncSnapshot<i32, String> = AsyncSnapshot::nothing();
/// assert_eq!(snapshot.connection_state(), ConnectionState::None);
/// assert!(!snapshot.has_data());
///
/// let done = AsyncSnapshot::<i32, String>::with_data(ConnectionState::Done, 7);
/// assert_eq!(done.data(), Some(&7));
/// assert!(done.error().is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AsyncSnapshot<T, E> {
    /// Current state of connection to the asynchronous computation.
    connection_state: ConnectionState,
    /// The latest value received. `Some` implies [`error`](Self::error) is `None`.
    data: Option<T>,
    /// The latest error received. `Some` implies [`data`](Self::data) is `None`.
    error: Option<E>,
}

impl<T, E> AsyncSnapshot<T, E> {
    // ── constructors ────────────────────────────────────────────────────────

    /// `ConnectionState::None`, with neither data nor error.
    #[must_use]
    pub const fn nothing() -> Self {
        Self {
            connection_state: ConnectionState::None,
            data: None,
            error: None,
        }
    }

    /// `ConnectionState::Waiting`, with neither data nor error.
    #[must_use]
    pub const fn waiting() -> Self {
        Self {
            connection_state: ConnectionState::Waiting,
            data: None,
            error: None,
        }
    }

    /// `state` with `data`, clearing any error.
    #[must_use]
    pub const fn with_data(state: ConnectionState, data: T) -> Self {
        Self {
            connection_state: state,
            data: Some(data),
            error: None,
        }
    }

    /// `state` with `error`, clearing any data.
    #[must_use]
    pub const fn with_error(state: ConnectionState, error: E) -> Self {
        Self {
            connection_state: state,
            data: None,
            error: Some(error),
        }
    }

    /// The snapshot a builder starts from: `with_data(None, d)` when
    /// `initial_data` is given, else [`nothing`](Self::nothing).
    #[must_use]
    pub fn initial(initial_data: Option<T>) -> Self {
        match initial_data {
            Some(data) => Self::with_data(ConnectionState::None, data),
            None => Self::nothing(),
        }
    }

    // ── accessors ───────────────────────────────────────────────────────────

    /// Current state of connection to the asynchronous computation.
    #[must_use]
    pub const fn connection_state(&self) -> ConnectionState {
        self.connection_state
    }

    /// The latest data received, borrowed — so `T` needs no `Clone`.
    #[must_use]
    pub const fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }

    /// The latest error received, borrowed — so `E` needs no `Clone`.
    #[must_use]
    pub const fn error(&self) -> Option<&E> {
        self.error.as_ref()
    }

    /// Whether this snapshot carries data.
    ///
    /// This cannot be false for a successfully-completed `Future<()>`: a unit
    /// value is still `Some(())`.
    #[must_use]
    pub const fn has_data(&self) -> bool {
        self.data.is_some()
    }

    /// Whether this snapshot carries an error.
    #[must_use]
    pub const fn has_error(&self) -> bool {
        self.error.is_some()
    }

    /// Consume the snapshot, yielding its data.
    #[must_use]
    pub fn into_data(self) -> Option<T> {
        self.data
    }

    /// Consume the snapshot, yielding its error.
    #[must_use]
    pub fn into_error(self) -> Option<E> {
        self.error
    }

    // ── transitions ─────────────────────────────────────────────────────────

    /// The same snapshot in a different [`ConnectionState`].
    ///
    /// **Data and error persist unmodified**, even when moving to
    /// `ConnectionState::None`. That preservation is load-bearing: it is why a
    /// `FutureBuilder` handed a new future keeps showing the old value while the
    /// new one is `Waiting`, and why `initial_data` is *not* re-applied on
    /// reconfigure.
    #[must_use]
    pub fn in_state(self, state: ConnectionState) -> Self {
        Self {
            connection_state: state,
            data: self.data,
            error: self.error,
        }
    }

    // ── FutureBuilder helpers (`_FutureBuilderState`) ───────────────────────

    /// After subscribing to a future: `Waiting`, **unless already `Done`**.
    ///
    /// The guard exists for a future that is `Ready` on its first poll: without
    /// it, an immediately-ready future would flash `Waiting`.
    #[must_use]
    pub fn after_subscribe(self) -> Self {
        if self.connection_state == ConnectionState::Done {
            self
        } else {
            self.in_state(ConnectionState::Waiting)
        }
    }

    /// A future completed with a value: `Done` + data.
    #[must_use]
    pub fn after_success(self, data: T) -> Self {
        Self::with_data(ConnectionState::Done, data)
    }

    /// A future completed with an error: `Done` + error.
    #[must_use]
    pub fn after_failure(self, error: E) -> Self {
        Self::with_error(ConnectionState::Done, error)
    }

    // ── StreamBuilder folds (`StreamBuilder`'s `after*` overrides) ──────────

    /// Connected to a stream: `Waiting`, preserving data/error.
    #[must_use]
    pub fn after_connected(self) -> Self {
        self.in_state(ConnectionState::Waiting)
    }

    /// A stream event: `Active` + data. **Clears any previous error.**
    #[must_use]
    pub fn after_data(self, data: T) -> Self {
        Self::with_data(ConnectionState::Active, data)
    }

    /// A stream error: `Active` + error. **Clears any previous data.**
    ///
    /// A Dart stream continues after an error unless `cancelOnError`; a Rust
    /// `Stream<Item = Result<T, E>>` does the same, so the state stays `Active`.
    #[must_use]
    pub fn after_error(self, error: E) -> Self {
        Self::with_error(ConnectionState::Active, error)
    }

    /// The stream ended: `Done`, preserving the last data **or** error.
    #[must_use]
    pub fn after_done(self) -> Self {
        self.in_state(ConnectionState::Done)
    }

    /// Disconnected from the stream: `None`, preserving the last data **or**
    /// error.
    ///
    /// Also the first half of a future/stream swap: the snapshot moves to
    /// `ConnectionState::None` before resubscribing.
    #[must_use]
    pub fn after_disconnected(self) -> Self {
        self.in_state(ConnectionState::None)
    }
}

impl<T, E> Default for AsyncSnapshot<T, E> {
    fn default() -> Self {
        Self::nothing()
    }
}

impl<T: fmt::Display, E: fmt::Display> fmt::Display for AsyncSnapshot<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AsyncSnapshot({:?}", self.connection_state)?;
        if let Some(data) = &self.data {
            write!(f, ", data: {data}")?;
        }
        if let Some(error) = &self.error {
            write!(f, ", error: {error}")?;
        }
        f.write_str(")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A payload that is deliberately NOT `Clone` and NOT `Copy`, to prove the
    /// snapshot's normal surface never requires those bounds.
    #[derive(Debug, PartialEq)]
    struct NoClone(i32);

    /// Likewise for the error type.
    #[derive(Debug, PartialEq)]
    struct Oops(&'static str);

    type Snap = AsyncSnapshot<NoClone, Oops>;

    // ── bounds ──────────────────────────────────────────────────────────────

    // ── invariant ───────────────────────────────────────────────────────────

    // ── FutureBuilder transition table ───────────────────────────────────────

    /// `'tracks life-cycle of Future to success'`: `None` → `Waiting` → `Done + data`.
    fn future_life_cycle_to_success() {
        let snapshot = Snap::initial(None);
        assert_eq!(snapshot.connection_state(), ConnectionState::None);

        let snapshot = snapshot.after_subscribe();
        assert_eq!(snapshot.connection_state(), ConnectionState::Waiting);

        let snapshot = snapshot.after_success(NoClone(42));
        assert_eq!(snapshot.connection_state(), ConnectionState::Done);
        assert_eq!(snapshot.data(), Some(&NoClone(42)));
        assert!(!snapshot.has_error());
    }

    // ── StreamBuilder fold table ─────────────────────────────────────────────

    /// Swapping streams: `after_disconnected` then `after_connected`, old value
    /// visible throughout. (`'gracefully handles transition to other stream'`.)
    fn stream_reconnect_preserves_the_last_value() {
        let snapshot = Snap::initial(None)
            .after_connected()
            .after_data(NoClone(1))
            .after_disconnected()
            .after_connected();

        assert_eq!(snapshot.connection_state(), ConnectionState::Waiting);
        assert_eq!(snapshot.data(), Some(&NoClone(1)));
    }

    // ── misc ────────────────────────────────────────────────────────────────

    #[test]
    fn async_snapshot_transitions() {
        crate::test_cases::run_cases(&[
            ("future life cycle to success", future_life_cycle_to_success),
            (
                "stream reconnect preserves the last value",
                stream_reconnect_preserves_the_last_value,
            ),
        ]);
    }
}
