//! [`WidgetState`]/[`WidgetStateProperty`] — the interactive-state vocabulary
//! that lets a widget's visual properties (color, overlay, border, …) depend
//! on whether it's hovered, focused, pressed, and so on.
//!
//! # Flutter parity
//!
//! `widgets/widget_state.dart` (oracle tag `3.44.0`): the `WidgetState` enum,
//! `WidgetStatesConstraint` mixin, `WidgetStateProperty<T>` interface, and
//! `WidgetStatesController`. This is the split Flutter made when it promoted
//! the mechanism out of the Material package (`MaterialState` →
//! `WidgetState`) — FLUI copies that split and hosts it in `flui-widgets`
//! rather than `flui-material`, since nothing about the vocabulary is
//! Material-specific (`ink_well.dart` reads `statesController.value`
//! directly, justifying the widgets-layer home for `flui-material`'s
//! `InkWell`).
//!
//! # Set representation: bitflags, not `HashSet`
//!
//! Flutter represents a widget's active states as `Set<WidgetState>`. FLUI
//! uses [`WidgetStates`], a [`bitflags`] bitset over a `u8` — the crate is
//! already a workspace dependency (`flui-rendering`'s dirty-flag storage
//! uses the same house style for a small, closed set of boolean flags). Eight
//! states fit in one byte; a bitset is `Copy`, allocation-free, and
//! trivially compared/combined, none of which a `HashSet<WidgetState>` gives
//! for free. The trade-off: adding a ninth [`WidgetState`] variant is a
//! breaking change to the flag layout, not just an enum growth — acceptable
//! here because the oracle enum has been stable across major Flutter
//! versions.
//!
//! # Named deferrals (not silently dropped)
//!
//! - **`WidgetStateProperty::lerp` / `WidgetStateBorderSide::lerp`** — arrive
//!   with `ButtonStyle.lerp`/`AnimatedTheme` in a later PR; nothing in this
//!   substrate consumes an interpolated property yet.
//! - **The Dart typed-subtype trick** (`WidgetStateColor extends Color`,
//!   `WidgetStateMouseCursor extends MouseCursor`, `WidgetStateBorderSide
//!   extends BorderSide`, `WidgetStateOutlinedBorder extends OutlinedBorder`,
//!   `WidgetStateTextStyle extends TextStyle`) — Dart lets one object satisfy
//!   both "the plain value" and "a property that resolves to it" via
//!   subclassing a concrete value type. Rust has no equivalent (the value
//!   types here are not open to inheritance, and blanket-implementing both
//!   roles on one type is not the goal). Call sites that want either a plain
//!   `Color` or a `WidgetStateProperty<Color>` take an explicit enum/variant
//!   instead — a documented, permanent divergence, not a deferral.
//! - **The full `&`/`|`/`~` `WidgetStatesConstraint` algebra** — the oracle's
//!   `WidgetStatesConstraint` mixin supports arbitrary boolean combinations
//!   (`WidgetState.focused | WidgetState.hovered`, `~WidgetState.disabled`,
//!   nested `&`/`|`). [`WidgetStateConstraint`] V1 ships only a single-state
//!   match plus [`WidgetStateConstraint::Any`] — enough to express
//!   first-match-wins resolution with a catch-all. The combinator algebra is
//!   a named deferral, not a rejected design; `WidgetStateConstraint` is
//!   `#[non_exhaustive]` so it can grow `And`/`Or`/`Not` variants without a
//!   breaking change.
//!
//! # The `Option<V>` fallthrough contract
//!
//! [`WidgetStateProperty::Map`]'s oracle counterpart
//! (`WidgetStateMapper.resolve`) throws `ArgumentError` when no map entry
//! matches and `T` is non-nullable — a runtime failure mode Dart's type
//! system cannot rule out statically. FLUI makes that failure mode
//! unrepresentable instead of documenting around it: [`resolve`] requires
//! `T: Default`, and a `Map` with no matching entry (or an empty `Map`)
//! resolves to `T::default()`. For `T = Option<V>` that default is `None`,
//! which is exactly the oracle's nullable-fallthrough behavior
//! (`WidgetStateBorderSide.resolve` returning `null` "to defer to the
//! default value of the widget or theme"). PR-2's `ButtonStyleButton` reads
//! a `WidgetStateProperty<Option<Color>>` (or similar) and chains
//! `widget_style.prop.resolve(&states) ?? theme_value ?? component_default`
//! — the Rust expression of the oracle's
//! `widget_style?.prop.resolve(states) ?? theme ?? default` cascade. Callers
//! whose `T` is not `Option`-shaped still get a total, panic-free `resolve`
//! by relying on that type's own `Default` (e.g. a plain `f32` elevation
//! resolves to `0.0` with no matching entry) rather than the oracle's
//! type-erased "throw or don't" split.
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
/// Flutter parity: `WidgetState` (`widget_state.dart`, oracle tag `3.44.0`).
/// Not limited to Material widgets — any widget can track a subset of these
/// in a [`WidgetStates`] set.
///
/// `#[non_exhaustive]`: the oracle enum has grown new members across Flutter
/// releases (`scrolledUnder` and `error` are both later additions); treat
/// this the same way and avoid exhaustive `match` outside this module.
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
    /// Flutter parity: `Set<WidgetState>`. See the module doc for why this
    /// is a bitset rather than a `HashSet<WidgetState>`.
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
    /// The empty set — no active states (Flutter's default `<WidgetState>{}`).
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
/// Flutter parity: `WidgetStatesConstraint` (`widget_state.dart`). The
/// oracle mixin supports an arbitrary `&`/`|`/`~` boolean algebra over
/// `WidgetState` combinations; V1 here ships only the two cases needed for
/// first-match-wins resolution with a catch-all — see the module doc's
/// "Named deferrals" section. `#[non_exhaustive]` leaves room to add
/// `And`/`Or`/`Not` variants later without a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WidgetStateConstraint {
    /// Satisfied exactly when the states set contains this one state.
    Is(WidgetState),
    /// Always satisfied — Flutter's `WidgetState.any`, meant as the final
    /// entry in a [`WidgetStateProperty::Map`] to guarantee a match.
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
/// Flutter parity: `WidgetStateProperty<T>` (`widget_state.dart`). The
/// oracle is an abstract interface with several concrete implementations
/// (`_WidgetStatePropertyWith`, `WidgetStatePropertyAll`,
/// `WidgetStateMapper`); FLUI collapses them into one enum since Rust has no
/// need for the separate allocation Dart's class hierarchy implies.
///
/// See the module doc for the `T: Default` requirement on
/// [`resolve`](Self::resolve) and the constraint-algebra deferral on
/// [`Map`](Self::Map).
#[derive(Clone)]
pub enum WidgetStateProperty<T> {
    /// Resolves to the same value regardless of state. Flutter's
    /// `WidgetStatePropertyAll`.
    All(T),
    /// Resolves via an arbitrary function of the current states. Flutter's
    /// `WidgetStateProperty.resolveWith` /
    /// `WidgetPropertyResolver<T>`. `Send + Sync` so a property built on one
    /// thread can be handed to a render/paint path on another.
    Resolver(Arc<dyn Fn(&WidgetStates) -> T + Send + Sync>),
    /// Resolves via first-match-wins lookup over an ordered list of
    /// constraints. Flutter's `WidgetStateMapper`/`WidgetStateProperty.fromMap`.
    /// An empty map, or a states set matching none of the entries, resolves
    /// to `T::default()` — see the module doc.
    Map(Vec<(WidgetStateConstraint, T)>),
}

impl<T> WidgetStateProperty<T> {
    /// A property that always resolves to `value`. Flutter's
    /// `WidgetStateProperty.all`/`WidgetStatePropertyAll`.
    pub const fn all(value: T) -> Self {
        Self::All(value)
    }

    /// A property that resolves via `resolver`. Flutter's
    /// `WidgetStateProperty.resolveWith`.
    pub fn resolve_with<F>(resolver: F) -> Self
    where
        F: Fn(&WidgetStates) -> T + Send + Sync + 'static,
    {
        Self::Resolver(Arc::new(resolver))
    }

    /// A property that resolves by first-match-wins lookup over `entries`.
    /// Flutter's `WidgetStateProperty.fromMap`.
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
    /// why this diverges from the oracle's throw-on-no-match `WidgetStateMapper`.
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

/// Resolves `value` against `states` if it is a [`WidgetStateProperty<T>`],
/// otherwise returns `value` unchanged.
///
/// Flutter parity: `WidgetStateProperty.resolveAs`. Useful for a parameter
/// that can optionally vary by state (e.g. a plain `Color` or a
/// `WidgetStateProperty<Color>`) without the caller branching.
pub fn resolve_as<T: Clone + Default>(value: &ResolveAs<T>, states: &WidgetStates) -> T {
    match value {
        ResolveAs::Fixed(v) => v.clone(),
        ResolveAs::Property(p) => p.resolve(states),
    }
}

/// Either a fixed `T` or a [`WidgetStateProperty<T>`] — the argument shape
/// [`resolve_as`] accepts. Flutter expresses this as "either a `Color` or a
/// `WidgetStateProperty<Color>`" via the typed-subtype trick (see the module
/// doc); this enum is the explicit Rust equivalent.
#[derive(Clone)]
pub enum ResolveAs<T> {
    /// A value that does not vary by state.
    Fixed(T),
    /// A value that varies by state.
    Property(WidgetStateProperty<T>),
}

impl<T: fmt::Debug> fmt::Debug for ResolveAs<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(value) => f.debug_tuple("Fixed").field(value).finish(),
            Self::Property(property) => f.debug_tuple("Property").field(property).finish(),
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
    /// Flutter parity note: the oracle documents that two `WidgetStateProperty`
    /// objects are only recognized as equal when they are `const` or define
    /// `operator==` themselves — otherwise comparisons (e.g. `ThemeData`
    /// equality) silently fall back to identity. FLUI makes that fallback
    /// explicit rather than ambient: [`Resolver`](Self::Resolver) compares by
    /// [`Arc::ptr_eq`] (closure identity, matching Dart's own closure-identity
    /// semantics for non-const resolvers), while [`All`](Self::All) and
    /// [`Map`](Self::Map) compare structurally since `T: PartialEq` makes
    /// that possible in Rust (unlike Dart, which needs a hand-written
    /// `operator==`).
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
/// Flutter parity: `WidgetStatesController extends ValueNotifier<Set<WidgetState>>`
/// (`widget_state.dart`). FLUI does not reuse [`flui_foundation::ValueNotifier`]
/// here: that type is single-owner (`&mut self` mutation), but a states
/// controller must be a shared, `Clone`-able handle — an app hands the same
/// controller to a custom widget and an `InkWell`/button below it
/// simultaneously (`ink_well.dart`'s `statesController` parameter is exactly
/// this). Instead this composes `flui-foundation`'s `ChangeNotifier` (already
/// `Arc`-backed and `Clone`-shared) with a `parking_lot`-guarded
/// [`WidgetStates`] cell — "a value cell of `WidgetStates` over
/// `flui-foundation`'s `ChangeNotifier` idiom."
///
/// [`update`](Self::update) is the only mutator, and notifies listeners only
/// when the set actually changes — Flutter parity: `ValueNotifier`
/// (`update`'s oracle, `WidgetStatesController.update`) only calls
/// `notifyListeners()` when `Set.add`/`Set.remove` reports a real change.
#[derive(Clone)]
pub struct WidgetStatesController {
    value: Arc<Mutex<WidgetStates>>,
    notifier: ChangeNotifier,
}

impl WidgetStatesController {
    /// Creates a controller starting at `initial` (Flutter's optional
    /// constructor argument; pass [`WidgetStates::NONE`] for the oracle's
    /// default empty set).
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

    /// Adds `state` to the set if `add` is `true`, removes it otherwise.
    /// Notifies listeners only if the set actually changed.
    ///
    /// Flutter parity: `WidgetStatesController.update`.
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    // ------------------------------------------------------------------
    // WidgetStates
    // ------------------------------------------------------------------

    #[test]
    fn states_contains_state_round_trips_through_with_and_without() {
        let states = WidgetStates::NONE
            .with_state(WidgetState::Hovered)
            .with_state(WidgetState::Focused);
        assert!(states.contains_state(WidgetState::Hovered));
        assert!(states.contains_state(WidgetState::Focused));
        assert!(!states.contains_state(WidgetState::Pressed));

        let without_hover = states.without_state(WidgetState::Hovered);
        assert!(!without_hover.contains_state(WidgetState::Hovered));
        assert!(without_hover.contains_state(WidgetState::Focused));
    }

    #[test]
    fn states_from_iterator_unions_every_member() {
        let states: WidgetStates = [WidgetState::Selected, WidgetState::Disabled]
            .into_iter()
            .collect();
        assert!(states.contains_state(WidgetState::Selected));
        assert!(states.contains_state(WidgetState::Disabled));
        assert!(!states.contains_state(WidgetState::Error));
    }

    // ------------------------------------------------------------------
    // WidgetStateProperty::resolve — precedence across variants
    // ------------------------------------------------------------------

    #[test]
    fn all_resolves_to_the_same_value_for_every_state_set() {
        let property = WidgetStateProperty::all(7_u32);
        assert_eq!(property.resolve(&WidgetStates::NONE), 7);
        assert_eq!(
            property.resolve(&WidgetStates::from(WidgetState::Pressed)),
            7
        );
    }

    #[test]
    fn resolver_receives_the_live_states_set() {
        let property = WidgetStateProperty::resolve_with(|states: &WidgetStates| {
            u32::from(states.contains_state(WidgetState::Pressed))
        });
        assert_eq!(property.resolve(&WidgetStates::NONE), 0);
        assert_eq!(
            property.resolve(&WidgetStates::from(WidgetState::Pressed)),
            1
        );
    }

    #[test]
    fn map_resolves_first_match_wins_in_entry_order() {
        // Two entries both match a Pressed+Focused set; the first-listed
        // entry must win (Flutter parity: `WidgetStateMapper.resolve` walks
        // `_map.entries` and returns the first satisfied key).
        let property = WidgetStateProperty::from_map([
            (WidgetStateConstraint::Is(WidgetState::Focused), "focused"),
            (WidgetStateConstraint::Is(WidgetState::Pressed), "pressed"),
            (WidgetStateConstraint::Any, "default"),
        ]);
        let states = WidgetStates::from(WidgetState::Focused).with_state(WidgetState::Pressed);
        assert_eq!(property.resolve(&states), "focused");
    }

    #[test]
    fn map_falls_through_to_any_when_no_specific_entry_matches() {
        let property = WidgetStateProperty::from_map([
            (WidgetStateConstraint::Is(WidgetState::Disabled), "disabled"),
            (WidgetStateConstraint::Any, "default"),
        ]);
        assert_eq!(property.resolve(&WidgetStates::NONE), "default");
    }

    #[test]
    fn map_resolves_to_default_when_nothing_matches_and_there_is_no_any() {
        let property: WidgetStateProperty<u32> =
            WidgetStateProperty::from_map([(WidgetStateConstraint::Is(WidgetState::Error), 9)]);
        assert_eq!(property.resolve(&WidgetStates::NONE), 0);
    }

    // ------------------------------------------------------------------
    // Option<V> fallthrough (PR-2's resolve-then-coalesce contract)
    // ------------------------------------------------------------------

    #[test]
    fn option_property_falls_through_to_none_on_no_match() {
        let property: WidgetStateProperty<Option<u32>> = WidgetStateProperty::from_map([(
            WidgetStateConstraint::Is(WidgetState::Selected),
            Some(42),
        )]);
        assert_eq!(property.resolve(&WidgetStates::NONE), None);
        assert_eq!(
            property.resolve(&WidgetStates::from(WidgetState::Selected)),
            Some(42)
        );
    }

    #[test]
    fn option_property_coalesce_chain_mirrors_button_style_button() {
        let widget_style: WidgetStateProperty<Option<&'static str>> =
            WidgetStateProperty::from_map([(
                WidgetStateConstraint::Is(WidgetState::Pressed),
                Some("widget-pressed-color"),
            )]);
        let theme_default = "theme-color";

        let resolved = widget_style
            .resolve(&WidgetStates::NONE)
            .unwrap_or(theme_default);
        assert_eq!(resolved, "theme-color");

        let resolved_pressed = widget_style
            .resolve(&WidgetStates::from(WidgetState::Pressed))
            .unwrap_or(theme_default);
        assert_eq!(resolved_pressed, "widget-pressed-color");
    }

    // ------------------------------------------------------------------
    // PartialEq — Arc::ptr_eq for Resolver
    // ------------------------------------------------------------------

    #[test]
    fn resolver_equality_is_pointer_identity_not_behavior() {
        let a = WidgetStateProperty::resolve_with(|_: &WidgetStates| 1_u32);
        let b = WidgetStateProperty::resolve_with(|_: &WidgetStates| 1_u32);
        // Same behavior, different closure identity: NOT equal.
        assert_ne!(a, b);

        let c = a.clone();
        // Same Arc, cloned: equal.
        assert_eq!(a, c);
    }

    #[test]
    fn all_and_map_equality_is_structural() {
        assert_eq!(
            WidgetStateProperty::all(1_u32),
            WidgetStateProperty::all(1_u32)
        );
        assert_ne!(
            WidgetStateProperty::all(1_u32),
            WidgetStateProperty::all(2_u32)
        );

        let map_a = WidgetStateProperty::from_map([(WidgetStateConstraint::Any, 1_u32)]);
        let map_b = WidgetStateProperty::from_map([(WidgetStateConstraint::Any, 1_u32)]);
        assert_eq!(map_a, map_b);
    }

    // ------------------------------------------------------------------
    // resolve_as
    // ------------------------------------------------------------------

    #[test]
    fn resolve_as_passes_through_a_fixed_value() {
        let value: ResolveAs<u32> = ResolveAs::Fixed(5);
        assert_eq!(resolve_as(&value, &WidgetStates::NONE), 5);
    }

    #[test]
    fn resolve_as_resolves_a_property_value() {
        let value: ResolveAs<u32> = ResolveAs::Property(WidgetStateProperty::all(9));
        assert_eq!(resolve_as(&value, &WidgetStates::NONE), 9);
    }

    // ------------------------------------------------------------------
    // WidgetStatesController
    // ------------------------------------------------------------------

    fn counting_listener(counter: &Arc<AtomicU32>) -> ListenerCallback {
        let counter = Arc::clone(counter);
        Arc::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        })
    }

    #[test]
    fn controller_starts_at_the_provided_initial_value() {
        let controller = WidgetStatesController::new(WidgetStates::from(WidgetState::Disabled));
        assert!(controller.value().contains_state(WidgetState::Disabled));
    }

    #[test]
    fn controller_default_starts_empty() {
        assert_eq!(
            WidgetStatesController::default().value(),
            WidgetStates::NONE
        );
    }

    #[test]
    fn controller_notifies_only_on_an_actual_change() {
        let controller = WidgetStatesController::default();
        let counter = Arc::new(AtomicU32::new(0));
        controller.add_listener(counting_listener(&counter));

        // First `add(Hovered, true)`: a real change, notifies once.
        controller.update(WidgetState::Hovered, true);
        assert_eq!(counter.load(Ordering::Relaxed), 1);
        assert!(controller.value().contains_state(WidgetState::Hovered));

        // Redundant `add(Hovered, true)`: already set, no notification.
        controller.update(WidgetState::Hovered, true);
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        // Redundant `remove(Pressed, false)`: was never set, no notification.
        controller.update(WidgetState::Pressed, false);
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        // `remove(Hovered, false)`: a real change, notifies again.
        controller.update(WidgetState::Hovered, false);
        assert_eq!(counter.load(Ordering::Relaxed), 2);
        assert!(!controller.value().contains_state(WidgetState::Hovered));
    }

    #[test]
    fn controller_clones_share_the_same_underlying_state() {
        let controller = WidgetStatesController::default();
        let clone = controller.clone();

        controller.update(WidgetState::Focused, true);

        assert!(clone.value().contains_state(WidgetState::Focused));
    }

    #[test]
    fn controller_debug_does_not_panic() {
        let controller = WidgetStatesController::default();
        let debug = format!("{controller:?}");
        assert!(debug.contains("WidgetStatesController"));
    }
}
