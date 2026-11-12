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
//! a `u16` rather than a `HashSet<WidgetState>` — the crate is
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

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use flui_foundation::{ChangeNotifier, Listenable};

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
    /// The widget holds focus, by any cause: a key, traversal, a pointer
    /// press or a request from code. A reader that shows focus (a state
    /// layer, a ring, a halo) resolves on [`FocusVisible`](Self::FocusVisible).
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
    /// The widget holds focus and the focus is shown: it was moved or last
    /// used by a key, traversal or assistive technology (ADR-0079). A
    /// pointer press that focuses the widget sets [`Focused`](Self::Focused)
    /// alone.
    FocusVisible,
}

impl WidgetState {
    /// This state's bit in [`WidgetStates`]' backing `u16`.
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
            Self::FocusVisible => WidgetStates::FOCUS_VISIBLE,
        }
    }
}

bitflags::bitflags! {
    /// A set of [`WidgetState`]s a widget is currently in.
    ///
    /// See the module doc for why this is a bitset rather than a
    /// `HashSet<WidgetState>`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct WidgetStates: u16 {
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
        /// See [`WidgetState::FocusVisible`].
        const FOCUS_VISIBLE = 1 << 8;
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
    Resolver(Rc<dyn Fn(&WidgetStates) -> T>),
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
        F: Fn(&WidgetStates) -> T + 'static,
    {
        Self::Resolver(Rc::new(resolver))
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
    /// compares by [`Rc::ptr_eq`] (closure identity), while
    /// [`All`](Self::All) and [`Map`](Self::Map) compare structurally since
    /// `T: PartialEq` makes that possible.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::All(a), Self::All(b)) => a == b,
            (Self::Resolver(a), Self::Resolver(b)) => Rc::ptr_eq(a, b),
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
/// `Rc`-backed and `Clone`-shared) with a shared
/// [`WidgetStates`] cell — "a value cell of `WidgetStates` over
/// `flui-foundation`'s `ChangeNotifier` idiom."
///
/// [`update`](Self::update) is the only mutator, and notifies listeners only
/// when the set actually changes, so a redundant `update` never wakes
/// listeners.
#[derive(Clone)]
pub struct WidgetStatesController {
    /// One handle: a control keeps a controller in its view, its state and
    /// its recognizers.
    shared: Rc<SharedStates>,
}

struct SharedStates {
    value: Cell<WidgetStates>,
    notifier: ChangeNotifier,
    /// How many pointers are inside the control that writes `Hovered` here,
    /// counted by its enters and exits.
    pointers_inside: Cell<u32>,
    /// How many [`PressHold`]s are live: the activations a control holds
    /// `Pressed` for past their contact. Bounded far below `u8::MAX` (a
    /// control holds one at a time), so it sits in the padding beside
    /// `value` and `pointers_inside`.
    holds: Cell<u8>,
    /// Whether the contact a recognizer of the writing control tracks is
    /// down: the other source of `Pressed`.
    contact: Cell<bool>,
}

impl WidgetStatesController {
    /// Creates a controller starting at `initial` (pass
    /// [`WidgetStates::NONE`] for the empty set).
    #[must_use]
    pub fn new(initial: WidgetStates) -> Self {
        Self {
            shared: Rc::new(SharedStates {
                value: Cell::new(initial),
                notifier: ChangeNotifier::new(),
                pointers_inside: Cell::new(0),
                holds: Cell::new(0),
                contact: Cell::new(false),
            }),
        }
    }

    /// The current set of active states.
    #[must_use]
    pub fn value(&self) -> WidgetStates {
        self.shared.value.get()
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
        Rc::ptr_eq(&self.shared, &other.shared)
    }

    /// Adds `state` to the set if `add` is `true`, removes it otherwise.
    /// Notifies listeners only if the set actually changed.
    pub fn update(&self, state: WidgetState, add: bool) {
        let before = self.shared.value.get();
        let after = if add {
            before.with_state(state)
        } else {
            before.without_state(state)
        };
        self.shared.value.set(after);
        if after != before {
            self.shared.notifier.notify_listeners();
        }
    }
}

impl WidgetStatesController {
    /// Replaces the states of `mask` with those `values` has of them, and
    /// notifies once if the set changed. The owner of several states writes
    /// them together this way, so a listener never sees half of a change.
    pub(crate) fn write(&self, mask: WidgetStates, values: WidgetStates) {
        let before = self.shared.value.get();
        let after = before.difference(mask).union(values.intersection(mask));
        self.shared.value.set(after);
        if after != before {
            self.shared.notifier.notify_listeners();
        }
    }

    /// A pointer entered the control that writes here: it is hovered.
    pub(crate) fn pointer_entered(&self) {
        let inside = &self.shared.pointers_inside;
        inside.set(inside.get().saturating_add(1));
        self.update(WidgetState::Hovered, true);
    }

    /// A pointer left it: the hover ends with the last one, so a mouse that
    /// leaves while a pen still hovers changes nothing.
    pub(crate) fn pointer_left(&self) {
        let inside = &self.shared.pointers_inside;
        let remaining = inside.get().saturating_sub(1);
        inside.set(remaining);
        if remaining == 0 {
            self.update(WidgetState::Hovered, false);
        }
    }

    /// Takes the count of pointers inside, leaving none: the control stopped
    /// tracking them (its exits are not waited for), or hands the count to
    /// the controller that replaces this one.
    pub(crate) fn take_pointers_inside(&self) -> u32 {
        self.shared.pointers_inside.replace(0)
    }

    /// Starts the count at what the replaced controller held.
    pub(crate) fn set_pointers_inside(&self, inside: u32) {
        self.shared.pointers_inside.set(inside);
    }

    /// The contact a recognizer of the writing control tracks went down or
    /// ended. `Pressed` follows it unless an activation still holds it.
    pub(crate) fn set_contact_pressed(&self, down: bool) {
        self.shared.contact.set(down);
        self.publish_pressed();
    }

    /// Takes whether a contact is down, leaving none: the control stopped
    /// writing here, or hands the contact to the controller that replaces
    /// this one.
    pub(crate) fn take_contact_pressed(&self) -> bool {
        self.shared.contact.replace(false)
    }

    /// Hold `Pressed` past the contact, for the activation a control runs
    /// (Flutter's `InkWell` holds it for `_activationDuration`): `Pressed` is
    /// set while any hold is live or the writing control's contact is down,
    /// and not while the control is disabled. Dropping the hold releases it.
    ///
    /// For the official catalogs (ADR-0322 §3); an application writes its
    /// own states with [`update`](Self::update).
    #[doc(hidden)]
    #[must_use = "the hold ends when it is dropped"]
    pub fn hold_pressed(&self) -> PressHold {
        let holds = &self.shared.holds;
        holds.set(
            holds
                .get()
                .checked_add(1)
                .expect("BUG: a control holds one activation at a time, far below u8::MAX"),
        );
        // The token exists before the listeners run: one that panics unwinds
        // through the token's `Drop`, which gives the count back.
        let hold = PressHold {
            states: self.clone(),
        };
        self.publish_pressed();
        hold
    }

    /// Whether a source of `Pressed` is live: the writing control's contact
    /// or an activation hold.
    pub(crate) fn held_pressed(&self) -> bool {
        self.shared.contact.get() || self.shared.holds.get() > 0
    }

    /// Writes `Pressed` from its sources: committed before the listeners
    /// run (ADR-0287).
    fn publish_pressed(&self) {
        let pressed = !self.value().contains_state(WidgetState::Disabled) && self.held_pressed();
        self.update(WidgetState::Pressed, pressed);
    }
}

/// An activation's hold on [`WidgetState::Pressed`], from
/// [`WidgetStatesController::hold_pressed`]. Dropping it releases the hold;
/// the last one released clears `Pressed` unless a contact is still down.
#[doc(hidden)]
#[must_use = "the hold ends when it is dropped"]
pub struct PressHold {
    states: WidgetStatesController,
}

impl fmt::Debug for PressHold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PressHold").finish_non_exhaustive()
    }
}

impl Drop for PressHold {
    /// The count is committed before the listeners run (ADR-0287).
    fn drop(&mut self) {
        let holds = &self.states.shared.holds;
        holds.set(
            holds
                .get()
                .checked_sub(1)
                .expect("BUG: every hold was counted when it was taken"),
        );
        self.states.publish_pressed();
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
    fn listen(&self, listener: flui_foundation::ListenerCallback) -> flui_foundation::Subscription {
        self.shared.notifier.listen(listener)
    }
    fn listen_with_recovery(
        &self,
        relay: flui_foundation::ListenerObserver,
    ) -> flui_foundation::Subscription {
        self.shared.notifier.listen_with_recovery(relay)
    }
}
