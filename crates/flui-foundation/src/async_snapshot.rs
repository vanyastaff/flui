//! [`ConnectionState`] and [`AsyncSnapshot`] — the data model for
//! `FutureBuilder` / `StreamBuilder` (ADR-0018, unit U3).
//!
//! Pure data: no futures, no executor, no widget. The state machine lives here
//! so the builders (U4/U5) are thin, and so `flui-material` never has to
//! re-declare it.
//!
//! # Verified against the reference
//!
//! Cross-checked against `.flutter/packages/flutter/lib/src/widgets/async.dart`
//! (`ConnectionState`, `AsyncSnapshot`, `_FutureBuilderState`, `StreamBuilder`'s
//! `after*` overrides) and `.flutter/packages/flutter/test/widgets/async_test.dart`,
//! Flutter master `3.33.0-0.0.pre-6280-g88e87cd963f`.
//!
//! The transition tables in [`AsyncSnapshot`]'s method docs are transcriptions,
//! not inventions. No parity is claimed for the *builders* — they do not exist
//! yet.
//!
//! # Deliberate divergences from Flutter
//!
//! | Flutter | FLUI | Why |
//! |---|---|---|
//! | `error: Object?` + `stackTrace: StackTrace` | generic `E`, **no stack trace** | Rust has no ambient stack traces on error values. `E` comes from `Future<Output = Result<T, E>>` — errors are in the type, not thrown. An infallible future uses `E = Infallible`. |
//! | `T get requireData` throws | **absent** | It exists in Dart because `data` is nullable and there is no `Option`. Use [`AsyncSnapshot::data`] → `Option<&T>` and `expect` at the call site. `docs/PANIC-POLICY.md` reserves panics for internal invariants. |
//! | `AsyncSnapshot` handed to `builder` by value | handed by **reference** | Avoids `T: Clone`. `FOUNDATIONS.md`: "Application state carries no trait bound beyond `'static` — the Druid mistake is the one most dangerous trap." |
//! | `AsyncSnapshot.waiting()` | [`AsyncSnapshot::waiting`] | Same, kept for symmetry even though the folds never need it. |
//!
//! # The data/error invariant
//!
//! Flutter asserts `data == null || error == null`. Here it is upheld **by
//! construction**: the fields are private, and every constructor and fold sets
//! exactly one of them. [`with_data`](AsyncSnapshot::with_data) clears the error;
//! [`with_error`](AsyncSnapshot::with_error) clears the data.

use core::fmt;

/// The state of connection to an asynchronous computation.
///
/// The usual flow is `None` → `Waiting` → `Active` → `Done`; a `Future` skips
/// `Active`, going straight from `Waiting` to `Done`.
///
/// Transcribed from Flutter's `ConnectionState`.
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
    ///
    /// Flutter: `AsyncSnapshot.nothing()`.
    #[must_use]
    pub const fn nothing() -> Self {
        Self {
            connection_state: ConnectionState::None,
            data: None,
            error: None,
        }
    }

    /// `ConnectionState::Waiting`, with neither data nor error.
    ///
    /// Flutter: `AsyncSnapshot.waiting()`.
    #[must_use]
    pub const fn waiting() -> Self {
        Self {
            connection_state: ConnectionState::Waiting,
            data: None,
            error: None,
        }
    }

    /// `state` with `data`, clearing any error.
    ///
    /// Flutter: `AsyncSnapshot.withData(state, data)`.
    #[must_use]
    pub const fn with_data(state: ConnectionState, data: T) -> Self {
        Self {
            connection_state: state,
            data: Some(data),
            error: None,
        }
    }

    /// `state` with `error`, clearing any data.
    ///
    /// Flutter: `AsyncSnapshot.withError(state, error)` — minus the stack trace.
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
    ///
    /// Flutter: `_FutureBuilderState.initState` / `StreamBuilder.initial`.
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
    /// Unlike Flutter, this cannot be false for a successfully-completed
    /// `Future<()>`: a unit value is still `Some(())`. Dart's `hasData` is
    /// `data != null`, so a `Future<void>` completes with `hasData == false`.
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
    /// reconfigure (Flutter's `'ignores initialData when reconfiguring'`).
    ///
    /// Flutter: `AsyncSnapshot.inState(state)`.
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
    /// The guard is Flutter's `if (_snapshot.connectionState != ConnectionState.done)`,
    /// which exists for `SynchronousFuture` — a future whose `.then` runs inline.
    /// Its Rust analogue is a future that is `Ready` on its first poll. Without
    /// the guard, an immediately-ready future would flash `Waiting`.
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
    ///
    /// Flutter: `afterConnected`.
    #[must_use]
    pub fn after_connected(self) -> Self {
        self.in_state(ConnectionState::Waiting)
    }

    /// A stream event: `Active` + data. **Clears any previous error.**
    ///
    /// Flutter: `afterData`.
    #[must_use]
    pub fn after_data(self, data: T) -> Self {
        Self::with_data(ConnectionState::Active, data)
    }

    /// A stream error: `Active` + error. **Clears any previous data.**
    ///
    /// A Dart stream continues after an error unless `cancelOnError`; a Rust
    /// `Stream<Item = Result<T, E>>` does the same, so the state stays `Active`.
    ///
    /// Flutter: `afterError`.
    #[must_use]
    pub fn after_error(self, error: E) -> Self {
        Self::with_error(ConnectionState::Active, error)
    }

    /// The stream ended: `Done`, preserving the last data **or** error.
    ///
    /// Flutter: `afterDone`.
    #[must_use]
    pub fn after_done(self) -> Self {
        self.in_state(ConnectionState::Done)
    }

    /// Disconnected from the stream: `None`, preserving the last data **or**
    /// error.
    ///
    /// Also the first half of a future/stream swap: Flutter's `didUpdateWidget`
    /// does `_snapshot.inState(ConnectionState.none)` before resubscribing.
    ///
    /// Flutter: `afterDisconnected`.
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

    /// The real compile-proof: a function generic over `T` and `E` with **no
    /// bounds at all**, driving every constructor, fold, and accessor. If any of
    /// them required `T: Clone` / `E: Clone` (or `Copy`), this would not compile.
    fn exercise_every_fold_without_bounds<T, E>(data: T, error: E) -> ConnectionState {
        let snapshot = AsyncSnapshot::<T, E>::nothing()
            .after_subscribe()
            .after_data(data)
            .after_error(error)
            .after_connected()
            .after_disconnected()
            .after_done()
            .in_state(ConnectionState::Active);
        let _ = snapshot.data();
        let _ = snapshot.error();
        let _ = snapshot.has_data();
        let _ = snapshot.has_error();
        let _ = AsyncSnapshot::<T, E>::waiting();
        let _ = AsyncSnapshot::<T, E>::initial(None);
        snapshot.connection_state()
    }

    /// Compile-proof: constructing, folding, and reading a snapshot works with a
    /// payload and error that implement neither `Clone` nor `Copy`.
    #[test]
    fn async_snapshot_needs_no_clone_bound_on_t_or_e() {
        // Instantiate the unbounded generic with non-Clone, non-Copy types.
        assert_eq!(
            exercise_every_fold_without_bounds(NoClone(1), Oops("e")),
            ConnectionState::Active
        );

        let snapshot: Snap = Snap::nothing()
            .after_subscribe()
            .after_data(NoClone(1))
            .after_error(Oops("boom"))
            .after_done();

        assert_eq!(snapshot.connection_state(), ConnectionState::Done);
        assert_eq!(snapshot.error(), Some(&Oops("boom")));
        assert!(!snapshot.has_data());
        assert_eq!(snapshot.into_error(), Some(Oops("boom")));
    }

    #[test]
    fn async_snapshot_default_is_nothing() {
        assert_eq!(Snap::default().connection_state(), ConnectionState::None);
        assert_eq!(ConnectionState::default(), ConnectionState::None);
    }

    // ── invariant ───────────────────────────────────────────────────────────

    /// Flutter asserts `data == null || error == null`. Here it holds by
    /// construction — every constructor and fold sets exactly one.
    #[test]
    fn async_snapshot_data_and_error_are_mutually_exclusive() {
        let with_data = Snap::with_data(ConnectionState::Done, NoClone(1));
        assert!(with_data.has_data() && !with_data.has_error());

        let with_error = Snap::with_error(ConnectionState::Done, Oops("e"));
        assert!(with_error.has_error() && !with_error.has_data());

        // A fold from one to the other clears the previous payload.
        let data_to_error =
            Snap::with_data(ConnectionState::Active, NoClone(1)).after_error(Oops("e"));
        assert!(!data_to_error.has_data(), "after_error clears data");

        let error_to_data =
            Snap::with_error(ConnectionState::Active, Oops("e")).after_data(NoClone(2));
        assert!(!error_to_data.has_error(), "after_data clears error");
    }

    // ── FutureBuilder transition table (ADR-0018 D4) ────────────────────────
    //
    // Transcribed from `_FutureBuilderState` and the oracles in
    // `.flutter/packages/flutter/test/widgets/async_test.dart`.

    /// `initState` with no `initialData`: `nothing()`.
    #[test]
    fn future_initial_without_initial_data_is_nothing() {
        let snapshot = Snap::initial(None);
        assert_eq!(snapshot.connection_state(), ConnectionState::None);
        assert!(!snapshot.has_data() && !snapshot.has_error());
    }

    /// `'runs the builder using given initial data'`: `with_data(None, d)`.
    #[test]
    fn future_initial_with_initial_data_is_none_plus_data() {
        let snapshot = Snap::initial(Some(NoClone(7)));
        assert_eq!(snapshot.connection_state(), ConnectionState::None);
        assert_eq!(snapshot.data(), Some(&NoClone(7)));
    }

    /// Subscribing moves to `Waiting` and **preserves** the initial data.
    #[test]
    fn future_after_subscribe_is_waiting_preserving_data() {
        let snapshot = Snap::initial(Some(NoClone(7))).after_subscribe();
        assert_eq!(snapshot.connection_state(), ConnectionState::Waiting);
        assert_eq!(snapshot.data(), Some(&NoClone(7)), "initial data survives");
    }

    /// `'tracks life-cycle of Future to success'`: `None` → `Waiting` → `Done + data`.
    #[test]
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

    /// `'tracks life-cycle of Future to error'`: `None` → `Waiting` → `Done + error`.
    #[test]
    fn future_life_cycle_to_error() {
        let snapshot = Snap::initial(None)
            .after_subscribe()
            .after_failure(Oops("x"));
        assert_eq!(snapshot.connection_state(), ConnectionState::Done);
        assert_eq!(snapshot.error(), Some(&Oops("x")));
        assert!(!snapshot.has_data());
    }

    /// `'gives expected snapshot with SynchronousFuture'`: a future already
    /// `Done` when `after_subscribe` runs must **not** be dragged back to
    /// `Waiting`.
    #[test]
    fn future_synchronous_completion_never_shows_waiting() {
        let snapshot = Snap::initial(None)
            .after_success(NoClone(1)) // completed inline, before after_subscribe
            .after_subscribe();

        assert_eq!(
            snapshot.connection_state(),
            ConnectionState::Done,
            "an already-Done snapshot must not regress to Waiting"
        );
        assert_eq!(snapshot.data(), Some(&NoClone(1)));
    }

    /// `'gracefully handles transition to other future'` +
    /// `'ignores initialData when reconfiguring'`: swapping the future does
    /// `in_state(None)` → `after_subscribe()` → `Waiting`, **keeping the old
    /// data** throughout. `initial_data` is never re-applied.
    #[test]
    fn future_new_key_preserves_old_data_through_none_and_waiting() {
        let settled = Snap::initial(None)
            .after_subscribe()
            .after_success(NoClone(1));
        assert_eq!(settled.connection_state(), ConnectionState::Done);

        // didUpdateWidget: unsubscribe, then `_snapshot.inState(none)`.
        let disconnected = settled.after_disconnected();
        assert_eq!(disconnected.connection_state(), ConnectionState::None);
        assert_eq!(
            disconnected.data(),
            Some(&NoClone(1)),
            "in_state(None) preserves data"
        );

        // …then resubscribe.
        let resubscribed = disconnected.after_subscribe();
        assert_eq!(resubscribed.connection_state(), ConnectionState::Waiting);
        assert_eq!(
            resubscribed.data(),
            Some(&NoClone(1)),
            "the old value is still shown while the new future is Waiting; \
             initialData is NOT re-applied"
        );
    }

    /// The same hop, starting from an error rather than data.
    #[test]
    fn future_new_key_preserves_old_error_through_none_and_waiting() {
        let failed = Snap::initial(None)
            .after_subscribe()
            .after_failure(Oops("x"));
        let resubscribed = failed.after_disconnected().after_subscribe();

        assert_eq!(resubscribed.connection_state(), ConnectionState::Waiting);
        assert_eq!(resubscribed.error(), Some(&Oops("x")));
    }

    /// A null future never subscribes: the snapshot stays where `initial` put it.
    /// (`'gracefully handles transition to null future'`.)
    #[test]
    fn future_absent_future_stays_in_initial_state() {
        let snapshot = Snap::initial(Some(NoClone(3)));
        assert_eq!(snapshot.connection_state(), ConnectionState::None);
        assert_eq!(snapshot.data(), Some(&NoClone(3)));
    }

    /// The "same future" case is represented by *not calling any fold*: an
    /// unchanged key means `didUpdateWidget` early-returns, so the snapshot is
    /// untouched.
    #[test]
    fn future_same_key_leaves_the_snapshot_untouched() {
        let settled = Snap::initial(None)
            .after_subscribe()
            .after_success(NoClone(9));
        assert_eq!(settled.connection_state(), ConnectionState::Done);
        assert_eq!(settled.data(), Some(&NoClone(9)));
    }

    // ── StreamBuilder fold table (ADR-0018 D4) ──────────────────────────────

    #[test]
    fn stream_initial_without_initial_data_is_nothing() {
        let snapshot = Snap::initial(None);
        assert_eq!(snapshot.connection_state(), ConnectionState::None);
        assert!(!snapshot.has_data());
    }

    #[test]
    fn stream_initial_with_initial_data_is_none_plus_data() {
        let snapshot = Snap::initial(Some(NoClone(5)));
        assert_eq!(snapshot.connection_state(), ConnectionState::None);
        assert_eq!(snapshot.data(), Some(&NoClone(5)));
    }

    #[test]
    fn stream_after_connected_is_waiting_preserving_data() {
        let snapshot = Snap::initial(Some(NoClone(5))).after_connected();
        assert_eq!(snapshot.connection_state(), ConnectionState::Waiting);
        assert_eq!(snapshot.data(), Some(&NoClone(5)));
    }

    #[test]
    fn stream_after_data_is_active_and_clears_error() {
        let snapshot =
            Snap::with_error(ConnectionState::Active, Oops("old")).after_data(NoClone(1));
        assert_eq!(snapshot.connection_state(), ConnectionState::Active);
        assert_eq!(snapshot.data(), Some(&NoClone(1)));
        assert!(!snapshot.has_error());
    }

    #[test]
    fn stream_after_error_is_active_and_clears_data() {
        let snapshot =
            Snap::with_data(ConnectionState::Active, NoClone(1)).after_error(Oops("boom"));
        assert_eq!(snapshot.connection_state(), ConnectionState::Active);
        assert_eq!(snapshot.error(), Some(&Oops("boom")));
        assert!(!snapshot.has_data());
    }

    #[test]
    fn stream_after_done_preserves_last_data() {
        let snapshot = Snap::initial(None)
            .after_connected()
            .after_data(NoClone(2))
            .after_done();
        assert_eq!(snapshot.connection_state(), ConnectionState::Done);
        assert_eq!(snapshot.data(), Some(&NoClone(2)));
    }

    #[test]
    fn stream_after_done_preserves_last_error() {
        let snapshot = Snap::initial(None)
            .after_connected()
            .after_error(Oops("e"))
            .after_done();
        assert_eq!(snapshot.connection_state(), ConnectionState::Done);
        assert_eq!(snapshot.error(), Some(&Oops("e")));
    }

    #[test]
    fn stream_after_disconnected_preserves_last_data() {
        let snapshot = Snap::initial(None)
            .after_connected()
            .after_data(NoClone(4))
            .after_disconnected();
        assert_eq!(snapshot.connection_state(), ConnectionState::None);
        assert_eq!(snapshot.data(), Some(&NoClone(4)));
    }

    #[test]
    fn stream_after_disconnected_preserves_last_error() {
        let snapshot = Snap::initial(None)
            .after_connected()
            .after_error(Oops("e"))
            .after_disconnected();
        assert_eq!(snapshot.connection_state(), ConnectionState::None);
        assert_eq!(snapshot.error(), Some(&Oops("e")));
    }

    /// `'tracks events and errors of stream until completion'`:
    /// `Waiting` → `Active(d)` → `Active(err)` → `Active(d)` → `Done`.
    #[test]
    fn stream_life_cycle_events_errors_then_done() {
        let snapshot = Snap::initial(None).after_connected();
        assert_eq!(snapshot.connection_state(), ConnectionState::Waiting);

        let snapshot = snapshot.after_data(NoClone(1));
        assert_eq!(snapshot.connection_state(), ConnectionState::Active);
        assert_eq!(snapshot.data(), Some(&NoClone(1)));

        let snapshot = snapshot.after_error(Oops("mid"));
        assert_eq!(snapshot.connection_state(), ConnectionState::Active);
        assert_eq!(snapshot.error(), Some(&Oops("mid")));
        assert!(!snapshot.has_data(), "an error clears the stale value");

        let snapshot = snapshot.after_data(NoClone(2));
        assert!(!snapshot.has_error(), "a value clears the stale error");

        let snapshot = snapshot.after_done();
        assert_eq!(snapshot.connection_state(), ConnectionState::Done);
        assert_eq!(snapshot.data(), Some(&NoClone(2)));
    }

    /// Swapping streams: `after_disconnected` then `after_connected`, old value
    /// visible throughout. (`'gracefully handles transition to other stream'`.)
    #[test]
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
    fn async_snapshot_display_reports_state_and_payload() {
        let data = AsyncSnapshot::<i32, String>::with_data(ConnectionState::Done, 3);
        assert_eq!(data.to_string(), "AsyncSnapshot(Done, data: 3)");

        let error =
            AsyncSnapshot::<i32, String>::with_error(ConnectionState::Done, "bad".to_owned());
        assert_eq!(error.to_string(), "AsyncSnapshot(Done, error: bad)");

        let nothing = AsyncSnapshot::<i32, String>::nothing();
        assert_eq!(nothing.to_string(), "AsyncSnapshot(None)");
    }

    #[test]
    fn async_snapshot_waiting_has_no_payload() {
        let snapshot = Snap::waiting();
        assert_eq!(snapshot.connection_state(), ConnectionState::Waiting);
        assert!(!snapshot.has_data() && !snapshot.has_error());
    }

    #[test]
    fn async_snapshot_in_state_preserves_payload() {
        let snapshot = Snap::with_data(ConnectionState::Active, NoClone(1))
            .in_state(ConnectionState::None)
            .in_state(ConnectionState::Waiting)
            .in_state(ConnectionState::Done);
        assert_eq!(snapshot.data(), Some(&NoClone(1)));
    }
}
