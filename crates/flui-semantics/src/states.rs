//! Typed states of a semantics node (ADR-0206, decision 3).
//!
//! Every state is `Option<T>`: `None` is undeclared, `Some(false)` is declared
//! false. The distinction is what merging needs: two configurations that
//! declare the same state describe two things and form separate nodes, while a
//! state one side never mentioned merges freely. The rule that settles a forced
//! merge lives on the state's type (`StateMerge`), not in a shared table.

use flui_foundation::geometry::Axis;

/// The checked state of a checkable role, or the pressed state of a button.
///
/// One value for both, as in AccessKit (`Toggled`) and Chromium
/// (`CheckedState` plus the toggle-button role): the node's role says which it
/// is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Toggled {
    /// Not checked, or not pressed.
    Off,
    /// Checked, or pressed.
    On,
    /// Indeterminate: partly checked.
    Mixed,
}

impl Toggled {
    /// [`Self::On`] for `true`, [`Self::Off`] for `false`.
    #[must_use]
    pub const fn from_bool(on: bool) -> Self {
        if on { Self::On } else { Self::Off }
    }
}

/// Whether a node's value failed validation, and how (`aria-invalid`).
///
/// [`Self::False`] is a declaration, not an absence: a field that validated
/// and passed declares it, so two configurations that both validate form
/// separate nodes as for any other declared state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Invalid {
    /// The value was validated and passed.
    False,
    /// The value failed validation.
    True,
    /// The value contains a grammatical error.
    Grammar,
    /// The value contains a spelling error.
    Spelling,
}

impl Invalid {
    /// [`Self::True`] for `true`, [`Self::False`] for `false`.
    #[must_use]
    pub const fn from_bool(invalid: bool) -> Self {
        if invalid { Self::True } else { Self::False }
    }

    /// Whether the value failed validation in any way.
    #[must_use]
    pub const fn is_invalid(self) -> bool {
        !matches!(self, Self::False)
    }
}

/// Which item of a set is the current one (`aria-current`).
///
/// Undeclared is ARIA's `false`: no variant stands for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Current {
    /// The current page of a set of pages, such as a navigation link.
    Page,
    /// The current step of a process.
    Step,
    /// The current location, such as on a map or in a flow chart.
    Location,
    /// The current date in a calendar.
    Date,
    /// The current time in a timetable.
    Time,
    /// The current item of a set that none of the other kinds describes.
    True,
}

/// What a node opens when activated (`aria-haspopup`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HasPopup {
    /// A menu.
    Menu,
    /// A list box.
    ListBox,
    /// A tree.
    Tree,
    /// A grid.
    Grid,
    /// A dialog.
    Dialog,
}

/// How a text field or combo box completes what the user types
/// (`aria-autocomplete`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AutoComplete {
    /// The rest of a suggestion is inserted after the caret, selected.
    Inline,
    /// Suggestions are shown in a popup.
    List,
    /// Both: a popup, and the selected suggestion inserted after the caret.
    Both,
}

/// How one state behaves when configurations meet.
pub(crate) trait StateMerge: Copy {
    /// Whether two declarations must form separate nodes under an ordinary
    /// merge: both sides declared the state.
    fn conflicts(receiver: Option<Self>, contributor: Option<Self>) -> bool {
        receiver.is_some() && contributor.is_some()
    }

    /// The state of one node built from both under a forced merge
    /// (`MergeSemantics`), where separate nodes are not an option.
    fn merge(receiver: Option<Self>, contributor: Option<Self>) -> Option<Self>;
}

impl StateMerge for Toggled {
    /// The more informative state wins: `Mixed`, then `On`, then `Off`.
    fn merge(receiver: Option<Self>, contributor: Option<Self>) -> Option<Self> {
        let rank = |state: Self| match state {
            Self::Off => 0,
            Self::On => 1,
            Self::Mixed => 2,
        };
        match (receiver, contributor) {
            (Some(a), Some(b)) => Some(if rank(b) > rank(a) { b } else { a }),
            (a, b) => a.or(b),
        }
    }
}

/// The states whose forced merge keeps the receiver's declaration, else the
/// contributor's: the node a forced merge builds is the receiver, so a
/// disabled tile is not enabled by a part that declares itself enabled, and a
/// field that passed validation is not marked failed by a part.
macro_rules! receiver_first {
    ($($state:ty),+) => {
        $(impl StateMerge for $state {
            fn merge(receiver: Option<Self>, contributor: Option<Self>) -> Option<Self> {
                receiver.or(contributor)
            }
        })+
    };
}

receiver_first!(bool, Invalid, Current, HasPopup, AutoComplete, Axis);

/// Every state one node declares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct States {
    pub(crate) toggled: Option<Toggled>,
    pub(crate) selected: Option<bool>,
    pub(crate) expanded: Option<bool>,
    pub(crate) enabled: Option<bool>,
    pub(crate) read_only: Option<bool>,
    pub(crate) focusable: Option<bool>,
    pub(crate) focused: Option<bool>,
    pub(crate) hidden: Option<bool>,
    pub(crate) modal: Option<bool>,
    pub(crate) required: Option<bool>,
    /// The node is being updated (`aria-busy`): its live changes wait.
    pub(crate) busy: Option<bool>,
    pub(crate) invalid: Option<Invalid>,
    pub(crate) current: Option<Current>,
    pub(crate) has_popup: Option<HasPopup>,
    pub(crate) auto_complete: Option<AutoComplete>,
    /// The axis a control runs along (`aria-orientation`).
    pub(crate) orientation: Option<Axis>,
    /// The node scopes a route. Read by the framework, not published.
    pub(crate) scopes_route: Option<bool>,
    /// The node names a route. Read by the framework, not published.
    pub(crate) names_route: Option<bool>,
}

impl States {
    /// The checked or pressed state, `None` when not declared.
    #[must_use]
    pub const fn toggled(&self) -> Option<Toggled> {
        self.toggled
    }

    /// Whether the node is selected, `None` when selection does not apply.
    #[must_use]
    pub const fn selected(&self) -> Option<bool> {
        self.selected
    }

    /// Whether the node is expanded, `None` when it does not expand.
    #[must_use]
    pub const fn expanded(&self) -> Option<bool> {
        self.expanded
    }

    /// Whether the node is enabled, `None` when it has no enabled state.
    #[must_use]
    pub const fn enabled(&self) -> Option<bool> {
        self.enabled
    }

    /// Whether the node's value is read-only, `None` when not declared.
    #[must_use]
    pub const fn read_only(&self) -> Option<bool> {
        self.read_only
    }

    /// Whether the node takes keyboard focus, `None` when not declared.
    #[must_use]
    pub const fn focusable(&self) -> Option<bool> {
        self.focusable
    }

    /// Whether the node has keyboard focus, `None` when not declared.
    #[must_use]
    pub const fn focused(&self) -> Option<bool> {
        self.focused
    }

    /// Whether the node is hidden, `None` when not declared.
    #[must_use]
    pub const fn hidden(&self) -> Option<bool> {
        self.hidden
    }

    /// Whether the node is modal, `None` when not declared.
    #[must_use]
    pub const fn modal(&self) -> Option<bool> {
        self.modal
    }

    /// Whether the form needs the node filled in, `None` when not declared.
    #[must_use]
    pub const fn required(&self) -> Option<bool> {
        self.required
    }

    /// Whether the node's value failed validation, `None` when not declared.
    #[must_use]
    pub const fn invalid(&self) -> Option<Invalid> {
        self.invalid
    }

    /// Whether the node is being updated, `None` when not declared.
    #[must_use]
    pub const fn busy(&self) -> Option<bool> {
        self.busy
    }

    /// Which kind of current item the node is, `None` when it is not one.
    #[must_use]
    pub const fn current(&self) -> Option<Current> {
        self.current
    }

    /// What the node opens when activated, `None` when not declared.
    #[must_use]
    pub const fn has_popup(&self) -> Option<HasPopup> {
        self.has_popup
    }

    /// How the node completes typed text, `None` when not declared.
    #[must_use]
    pub const fn auto_complete(&self) -> Option<AutoComplete> {
        self.auto_complete
    }

    /// The axis the node runs along, `None` when not declared.
    #[must_use]
    pub const fn orientation(&self) -> Option<Axis> {
        self.orientation
    }

    /// Whether the node scopes a route, `None` when not declared. Read by the
    /// framework, not published.
    #[must_use]
    pub const fn scopes_route(&self) -> Option<bool> {
        self.scopes_route
    }

    /// Whether the node names a route, `None` when not declared. Read by the
    /// framework, not published.
    #[must_use]
    pub const fn names_route(&self) -> Option<bool> {
        self.names_route
    }

    /// Whether any state is declared.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether `self` and `other` declare a state in common.
    pub(crate) fn conflicts(&self, other: &Self) -> bool {
        let Self {
            toggled,
            selected,
            expanded,
            enabled,
            read_only,
            focusable,
            focused,
            hidden,
            modal,
            required,
            busy,
            invalid,
            current,
            has_popup,
            auto_complete,
            orientation,
            scopes_route,
            names_route,
        } = *self;
        Toggled::conflicts(toggled, other.toggled)
            || bool::conflicts(selected, other.selected)
            || bool::conflicts(expanded, other.expanded)
            || bool::conflicts(enabled, other.enabled)
            || bool::conflicts(read_only, other.read_only)
            || bool::conflicts(focusable, other.focusable)
            || bool::conflicts(focused, other.focused)
            || bool::conflicts(hidden, other.hidden)
            || bool::conflicts(modal, other.modal)
            || bool::conflicts(required, other.required)
            || bool::conflicts(busy, other.busy)
            || Invalid::conflicts(invalid, other.invalid)
            || Current::conflicts(current, other.current)
            || HasPopup::conflicts(has_popup, other.has_popup)
            || AutoComplete::conflicts(auto_complete, other.auto_complete)
            || Axis::conflicts(orientation, other.orientation)
            || bool::conflicts(scopes_route, other.scopes_route)
            || bool::conflicts(names_route, other.names_route)
    }

    /// Takes `other`'s states into `self` by each state's merge rule.
    pub(crate) fn merge(&mut self, other: &Self) {
        let Self {
            toggled,
            selected,
            expanded,
            enabled,
            read_only,
            focusable,
            focused,
            hidden,
            modal,
            required,
            busy,
            invalid,
            current,
            has_popup,
            auto_complete,
            orientation,
            scopes_route,
            names_route,
        } = self;
        *toggled = Toggled::merge(*toggled, other.toggled);
        *selected = bool::merge(*selected, other.selected);
        *expanded = bool::merge(*expanded, other.expanded);
        *enabled = bool::merge(*enabled, other.enabled);
        *read_only = bool::merge(*read_only, other.read_only);
        *focusable = bool::merge(*focusable, other.focusable);
        *focused = bool::merge(*focused, other.focused);
        *hidden = bool::merge(*hidden, other.hidden);
        *modal = bool::merge(*modal, other.modal);
        *required = bool::merge(*required, other.required);
        *busy = bool::merge(*busy, other.busy);
        *invalid = Invalid::merge(*invalid, other.invalid);
        *current = Current::merge(*current, other.current);
        *has_popup = HasPopup::merge(*has_popup, other.has_popup);
        *auto_complete = AutoComplete::merge(*auto_complete, other.auto_complete);
        *orientation = Axis::merge(*orientation, other.orientation);
        *scopes_route = bool::merge(*scopes_route, other.scopes_route);
        *names_route = bool::merge(*names_route, other.names_route);
    }
}
