//! [`WidgetState`]/[`WidgetStateProperty`] — the interactive-state vocabulary
//! that lets a widget's visual properties (color, overlay, border, …) depend
//! on whether it's hovered, focused, pressed, and so on.
//!
//! # Where it lives
//!
//! The vocabulary is [`WidgetState`], [`WidgetStateConstraint`],
//! [`WidgetStateProperty`] and [`WidgetStatesController`]. It is hosted in
//! `flui-widgets` rather than `flui-material`, since nothing about it is
//! Material-specific (`flui-material`'s `InkWell` reads a
//! [`WidgetStatesController`] directly).
//!
//! # Set representation: bitflags, not `HashSet`
//!
//! A widget's active states are a [`WidgetStates`], a [`bitflags`] bitset over
//! a `u8` rather than a `HashSet<WidgetState>` — the crate is
//! already a workspace dependency (`flui-rendering`'s dirty-flag storage
//! uses the same house style for a small, closed set of boolean flags). Eight
//! states fit in one byte; a bitset is `Copy`, allocation-free, and
//! trivially compared/combined, none of which a `HashSet<WidgetState>` gives
//! for free. The trade-off: adding a ninth [`WidgetState`] variant is a
//! breaking change to the flag layout, not just an enum growth — acceptable
//! here because the set of interaction states is small and stable.
//!
//! # Named deferrals (not silently dropped)
//!
//! - **`WidgetStateProperty::lerp` / `WidgetStateBorderSide::lerp`** —
//!   arrives when a component first needs `ButtonStyle.lerp`/`AnimatedTheme`;
//!   nothing in this substrate consumes an interpolated property yet.
//! - **One type that is both a plain value and a property** (a color that
//!   is also a `WidgetStateProperty<Color>`) — that needs subclassing a
//!   concrete value type, which Rust lacks (the value types here are not
//!   open to inheritance, and blanket-implementing both roles on one type
//!   is not the goal). A call site that wants either a plain `Color` or a
//!   `WidgetStateProperty<Color>` needs an explicit enum/variant of its
//!   own — permanent, not a deferral. No such enum ships from this module yet (an earlier
//!   `ResolveAs<T>`/`resolve_as` pair was removed for having no consumer);
//!   add one against the first real call site that needs it instead of
//!   speculating on its shape here.
//! - **The full `&`/`|`/`~` constraint algebra** — arbitrary boolean
//!   combinations (`focused | hovered`, `~disabled`, nested `&`/`|`).
//!   [`WidgetStateConstraint`] ships only a single-state
//!   match plus [`WidgetStateConstraint::Any`] — enough to express
//!   first-match-wins resolution with a catch-all. The combinator algebra is
//!   a named deferral, not a rejected design; `WidgetStateConstraint` is
//!   `#[non_exhaustive]` so it can grow `And`/`Or`/`Not` variants without a
//!   breaking change.
//!
//! # The `Option<V>` fallthrough contract
//!
//! A [`WidgetStateProperty::Map`] with no matching entry has no value to
//! produce, and a runtime failure there would be a mode the type system
//! cannot rule out. FLUI makes that failure unrepresentable instead of
//! documenting around it: [`resolve`] requires `T: Default`, and a `Map`
//! with no matching entry (or an empty `Map`) resolves to `T::default()`.
//! For `T = Option<V>` that default is `None`, meaning "defer to the default
//! value of the widget or theme". A button-style consumer
//! reads a `WidgetStateProperty<Option<Color>>` (or similar) and chains
//! `widget_style.prop.resolve(&states).or(theme_value).unwrap_or(component_default)`.
//! Callers whose `T` is not `Option`-shaped still get a total, panic-free
//! `resolve` by relying on that type's own `Default` (e.g. a plain `f64`
//! elevation resolves to `0.0` with no matching entry).
//!
//! [`resolve`]: WidgetStateProperty::resolve

use std::fmt;
use std::sync::Arc;

use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use parking_lot::Mutex;

// ============================================================================
// WidgetState / WidgetStates
// ============================================================================

/// One interactive state a widget can be in, per the M3 interaction-states
/// spec (<https://m3.material.io/foundations/interaction/states>).
///
/// Not limited to Material widgets — any widget can track a subset of these
/// in a [`WidgetStates`] set.
///
/// `#[non_exhaustive]`: interaction-state vocabularies grow over time
/// (`ScrolledUnder` and `Error` are late additions to the usual set); avoid
/// exhaustive `match` outside this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WidgetState {
    /// The pointer is hovering over the widget.
    Hovered,
    /// The widget holds keyboard focus (or was tapped into focus).
    Focused,
    /// The user is actively pressing down on the widget.
    Pressed,
    /// The widget is being dragged from one place to another.
    Dragged,
    /// The widget has been selected (toggled on, or chosen from a set).
    Selected,
    /// The widget overlaps the content of a scrollable that has scrolled
    /// beneath it (e.g. an app bar during scroll).
    ScrolledUnder,
    /// The widget is disabled and does not respond to interaction.
    Disabled,
    /// The widget has entered an invalid/error state.
    Error,
}

impl WidgetState {
    /// This state's bit in [`WidgetStates`]' backing `u8`.
    const fn flag(self) -> WidgetStates {
        match self {
            Self::Hovered => WidgetStates::HOVERED,
            Self::Focused => WidgetStates::FOCUSED,
            Self::Pressed => WidgetStates::PRESSED,
            Self::Dragged => WidgetStates::DRAGGED,
            Self::Selected => WidgetStates::SELECTED,
            Self::ScrolledUnder => WidgetStates::SCROLLED_UNDER,
            Self::Disabled => WidgetStates::DISABLED,
            Self::Error => WidgetStates::ERROR,
        }
    }
}

bitflags::bitflags! {
    /// A set of [`WidgetState`]s a widget is currently in.
    ///
    /// See the module doc for why this is a bitset rather than a
    /// `HashSet<WidgetState>`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct WidgetStates: u8 {
        /// See [`WidgetState::Hovered`].
        const HOVERED = 1 << 0;
        /// See [`WidgetState::Focused`].
        const FOCUSED = 1 << 1;
        /// See [`WidgetState::Pressed`].
        const PRESSED = 1 << 2;
        /// See [`WidgetState::Dragged`].
        const DRAGGED = 1 << 3;
        /// See [`WidgetState::Selected`].
        const SELECTED = 1 << 4;
        /// See [`WidgetState::ScrolledUnder`].
        const SCROLLED_UNDER = 1 << 5;
        /// See [`WidgetState::Disabled`].
        const DISABLED = 1 << 6;
        /// See [`WidgetState::Error`].
        const ERROR = 1 << 7;
    }
}

impl WidgetStates {
    /// The empty set — no active states.
    pub const NONE: Self = Self::empty();

    /// Whether `state` is a member of this set.
    #[must_use]
    pub const fn contains_state(self, state: WidgetState) -> bool {
        self.contains(state.flag())
    }

    /// Returns a copy of this set with `state` added.
    #[must_use]
    pub const fn with_state(self, state: WidgetState) -> Self {
        self.union(state.flag())
    }

    /// Returns a copy of this set with `state` removed.
    #[must_use]
    pub const fn without_state(self, state: WidgetState) -> Self {
        self.difference(state.flag())
    }
}

impl From<WidgetState> for WidgetStates {
    fn from(state: WidgetState) -> Self {
        state.flag()
    }
}

impl FromIterator<WidgetState> for WidgetStates {
    fn from_iter<I: IntoIterator<Item = WidgetState>>(iter: I) -> Self {
        iter.into_iter().map(WidgetStates::from).collect()
    }
}

// ============================================================================
// WidgetStateConstraint
// ============================================================================

/// A predicate a [`WidgetStates`] set either satisfies or doesn't — the key
/// type for [`WidgetStateProperty::Map`] entries.
///
/// Only the two cases needed for first-match-wins resolution with a catch-all
/// exist today, not an arbitrary `&`/`|`/`~` boolean algebra — see the module
/// doc's "Named deferrals" section. `#[non_exhaustive]` leaves room to add
/// `And`/`Or`/`Not` variants later without a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WidgetStateConstraint {
    /// Satisfied exactly when the states set contains this one state.
    Is(WidgetState),
    /// Always satisfied — meant as the final entry in a [`WidgetStateProperty::Map`] to guarantee a match.
    Any,
}

impl WidgetStateConstraint {
    /// Whether `states` satisfies this constraint.
    #[must_use]
    pub const fn is_satisfied_by(self, states: WidgetStates) -> bool {
        match self {
            Self::Is(state) => states.contains_state(state),
            Self::Any => true,
        }
    }
}

impl From<WidgetState> for WidgetStateConstraint {
    fn from(state: WidgetState) -> Self {
        Self::Is(state)
    }
}

// ============================================================================
// WidgetStateProperty<T>
// ============================================================================

/// A value of type `T` that depends on a widget's [`WidgetStates`].
///
/// One enum covers the constant, closure and map forms, with no separate
/// allocation per kind.
///
/// See the module doc for the `T: Default` requirement on
/// [`resolve`](Self::resolve) and the constraint-algebra deferral on
/// [`Map`](Self::Map).
///
/// `#[non_exhaustive]`, matching sibling [`WidgetStateConstraint`]'s
/// growth story: `WidgetStateProperty::lerp` (a named deferral, see the
/// module doc) is a likely future fourth variant carrying interpolation
/// state, and this enum should be free to grow one without a breaking
/// change. Use the constructors ([`all`](Self::all), [`resolve_with`](Self::resolve_with),
/// [`from_map`](Self::from_map)) rather than matching variants directly.
#[derive(Clone)]
#[non_exhaustive]
pub enum WidgetStateProperty<T> {
    /// Resolves to the same value regardless of state.
    All(T),
    /// Resolves via an arbitrary function of the current states.
    /// `Send + Sync` so a property built on one
    /// thread can be handed to a render/paint path on another.
    Resolver(Arc<dyn Fn(&WidgetStates) -> T + Send + Sync>),
    /// Resolves via first-match-wins lookup over an ordered list of
    /// constraints. An empty map, or a states set matching none of the entries, resolves
    /// to `T::default()` — see the module doc.
    Map(Vec<(WidgetStateConstraint, T)>),
}

impl<T> WidgetStateProperty<T> {
    /// A property that always resolves to `value`.
    pub const fn all(value: T) -> Self {
        Self::All(value)
    }

    /// A property that resolves via `resolver`.
    pub fn resolve_with<F>(resolver: F) -> Self
    where
        F: Fn(&WidgetStates) -> T + Send + Sync + 'static,
    {
        Self::Resolver(Arc::new(resolver))
    }

    /// A property that resolves by first-match-wins lookup over `entries`.
    pub fn from_map<I>(entries: I) -> Self
    where
        I: IntoIterator<Item = (WidgetStateConstraint, T)>,
    {
        Self::Map(entries.into_iter().collect())
    }
}

impl<T: Clone + Default> WidgetStateProperty<T> {
    /// Resolves this property against `states`.
    ///
    /// Total and panic-free for every `T: Default` — see the module doc for
    /// why a `Map` with no matching entry resolves to `T::default()`.
    #[must_use]
    pub fn resolve(&self, states: &WidgetStates) -> T {
        match self {
            Self::All(value) => value.clone(),
            Self::Resolver(resolver) => resolver(states),
            Self::Map(entries) => entries
                .iter()
                .find(|(constraint, _)| constraint.is_satisfied_by(*states))
                .map_or_else(T::default, |(_, value)| value.clone()),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for WidgetStateProperty<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All(value) => f.debug_tuple("All").field(value).finish(),
            Self::Resolver(_) => f.write_str("Resolver(..)"),
            Self::Map(entries) => f.debug_tuple("Map").field(entries).finish(),
        }
    }
}

impl<T: PartialEq> PartialEq for WidgetStateProperty<T> {
    /// Closures have no structural equality, so [`Resolver`](Self::Resolver)
    /// compares by [`Arc::ptr_eq`] (closure identity), while
    /// [`All`](Self::All) and [`Map`](Self::Map) compare structurally since
    /// `T: PartialEq` makes that possible.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::All(a), Self::All(b)) => a == b,
            (Self::Resolver(a), Self::Resolver(b)) => Arc::ptr_eq(a, b),
            (Self::Map(a), Self::Map(b)) => a == b,
            _ => false,
        }
    }
}

// ============================================================================
// WidgetStatesController
// ============================================================================

/// Manages a [`WidgetStates`] set and notifies listeners when it changes.
///
/// This does not reuse [`flui_foundation::ValueNotifier`]:
/// that type is single-owner (`&mut self` mutation), but a states
/// controller must be a shared, `Clone`-able handle — an app hands the same
/// controller to a custom widget and an `InkWell`/button below it
/// simultaneously. Instead this composes `flui-foundation`'s `ChangeNotifier` (already
/// `Arc`-backed and `Clone`-shared) with a `parking_lot`-guarded
/// [`WidgetStates`] cell — "a value cell of `WidgetStates` over
/// `flui-foundation`'s `ChangeNotifier` idiom."
///
/// [`update`](Self::update) is the only mutator, and notifies listeners only
/// when the set actually changes, so a redundant `update` never wakes
/// listeners.
#[derive(Clone)]
pub struct WidgetStatesController {
    value: Arc<Mutex<WidgetStates>>,
    notifier: ChangeNotifier,
}

impl WidgetStatesController {
    /// Creates a controller starting at `initial` (pass
    /// [`WidgetStates::NONE`] for the empty set).
    #[must_use]
    pub fn new(initial: WidgetStates) -> Self {
        Self {
            value: Arc::new(Mutex::new(initial)),
            notifier: ChangeNotifier::new(),
        }
    }

    /// The current set of active states.
    #[must_use]
    pub fn value(&self) -> WidgetStates {
        *self.value.lock()
    }

    /// Whether `self` and `other` share the same underlying state cell —
    /// identity, not value equality (two independently-`new`'d controllers
    /// with the same current value are NOT `is_same`). Mirrors
    /// [`flui_animation::Vsync::is_same`]'s precedent for the same "same
    /// ambient registry vs. a fresh clone with equal value" question.
    ///
    /// Used to detect "the caller swapped in a different
    /// `WidgetStatesController` on rebuild" (a `Some` -> `Some` change to a
    /// *different* controller, as opposed to a rebuild re-cloning the same
    /// one) — see `flui_material::InkWell`'s `did_update_view`, which
    /// re-homes its listener on such a swap.
    #[must_use]
    pub fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.value, &other.value)
    }

    /// Adds `state` to the set if `add` is `true`, removes it otherwise.
    /// Notifies listeners only if the set actually changed.
    pub fn update(&self, state: WidgetState, add: bool) {
        let changed = {
            let mut guard = self.value.lock();
            let before = *guard;
            *guard = if add {
                guard.with_state(state)
            } else {
                guard.without_state(state)
            };
            *guard != before
        };
        if changed {
            self.notifier.notify_listeners();
        }
    }
}

impl Default for WidgetStatesController {
    fn default() -> Self {
        Self::new(WidgetStates::NONE)
    }
}

impl fmt::Debug for WidgetStatesController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WidgetStatesController")
            .field("value", &self.value())
            .finish_non_exhaustive()
    }
}

impl Listenable for WidgetStatesController {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}
