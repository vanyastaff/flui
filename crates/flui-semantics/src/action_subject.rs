//! Which configuration's handler a forced merge's standard action runs
//! (ADR-0392).
//!
//! Each standard action changes one group of a node's attributes, its
//! subject. A forced merge first reads every configuration it folds and
//! plans, per subject, the one whose declaration the node publishes: its
//! owner. An owner that acts on its subject supplies every handler of it, so
//! the state a node announces and the action that changes it come from one
//! control, whatever the order of the parts and whatever a later part changes.

use crate::applicability::{RoleAttribute, applicable};
use crate::configuration::more_specific_role;
use crate::{SemanticsAction, SemanticsConfiguration, SemanticsRole};

/// The group of a node's attributes a standard action changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionSubject {
    /// Nothing the node publishes: the first handler in fold order runs.
    Activation,
    /// `toggled`, which the role gives its meaning (ADR-0206 decision 3).
    Toggle,
    /// `selected`.
    Selection,
    /// `expanded`.
    Disclosure,
    /// The numeric range.
    Range,
    /// The scroll group: position, axis and extents.
    Scroll,
    /// The value.
    Value,
    /// The text content.
    Text,
    /// `focusable` and `focused`, one group.
    Focus,
    /// The tooltip.
    Tooltip,
}

/// Every subject but `Activation`, which has no owner.
const OWNED: [ActionSubject; 9] = [
    ActionSubject::Toggle,
    ActionSubject::Selection,
    ActionSubject::Disclosure,
    ActionSubject::Range,
    ActionSubject::Scroll,
    ActionSubject::Value,
    ActionSubject::Text,
    ActionSubject::Focus,
    ActionSubject::Tooltip,
];

impl ActionSubject {
    /// The subject `action` changes on a node configured as `node`, `None`
    /// for custom actions, which each keep their own handler.
    #[cfg(test)]
    fn of(action: SemanticsAction, node: &SemanticsConfiguration) -> Option<Self> {
        Self::classify(action, Self::click_on(node), node.numeric_range().is_some())
    }

    /// The subject of `action` on a node whose `Click` changes `click` and
    /// which publishes a numeric range when `has_range`.
    fn classify(action: SemanticsAction, click: Self, has_range: bool) -> Option<Self> {
        use SemanticsAction as A;
        // `SemanticsAction` is `#[non_exhaustive]`: `every_action_names_its_subject`
        // lists each action, so a new one fails it until it is placed here.
        Some(match action {
            A::Click => click,
            A::Expand | A::Collapse => Self::Disclosure,
            A::Increment | A::Decrement | A::SetNumericValue => {
                if has_range {
                    Self::Range
                } else {
                    Self::Scroll
                }
            }
            A::ScrollUp | A::ScrollDown | A::ScrollLeft | A::ScrollRight | A::SetScrollOffset => {
                Self::Scroll
            }
            A::SetValue => Self::Value,
            A::SetTextSelection | A::ReplaceSelectedText => Self::Text,
            A::Focus | A::Blur => Self::Focus,
            A::ShowTooltip | A::HideTooltip => Self::Tooltip,
            // Reveal moves the scroll groups of the node and its ancestors.
            A::ScrollIntoView => Self::Scroll,
            A::CustomAction => return None,
            _ => Self::Activation,
        })
    }

    /// What `Click` changes on `node`: the first of `toggled`, `selected`
    /// and a click-only `expanded` (one with `Click` and without `Expand` or
    /// `Collapse`) it declares.
    #[cfg(test)]
    fn click_on(node: &SemanticsConfiguration) -> Self {
        let states = node.states();
        if states.toggled().is_some() {
            Self::Toggle
        } else if states.selected().is_some() {
            Self::Selection
        } else if states.expanded().is_some()
            && node.has_action(SemanticsAction::Click)
            && !node.has_action(SemanticsAction::Expand)
            && !node.has_action(SemanticsAction::Collapse)
        {
            Self::Disclosure
        } else {
            Self::Activation
        }
    }

    /// Whether `config` declares the group this subject changes.
    fn is_declared_by(self, config: &SemanticsConfiguration) -> bool {
        let states = config.states();
        match self {
            Self::Activation => false,
            Self::Toggle => states.toggled().is_some(),
            Self::Selection => states.selected().is_some(),
            Self::Disclosure => states.expanded().is_some(),
            Self::Range => config.numeric_range().is_some(),
            Self::Scroll => config.declares_scroll_group(),
            Self::Value => config.value().is_some(),
            Self::Text => config.text_content().is_some(),
            Self::Focus => states.focusable().is_some() || states.focused().is_some(),
            Self::Tooltip => config.tooltip().is_some(),
        }
    }

    /// Whether `owner`, which declares this subject's group, acts on it: it
    /// registers one of the subject's actions (a blocked one included),
    /// declares itself disabled, or is a merge whose owner of the subject
    /// acted. `click` and `has_range` classify actions as on the merged node.
    fn is_acted_on_by(self, owner: &SemanticsConfiguration, click: Self, has_range: bool) -> bool {
        owner.states().enabled() == Some(false)
            || owner.withheld_subjects().contains(self)
            || owner
                .registered_actions()
                .any(|action| Self::classify(action, click, has_range) == Some(self))
    }

    /// The position of this subject in a [`SubjectSet`].
    const fn bit(self) -> u16 {
        1 << self as u16
    }
}

/// A set of [`ActionSubject`]s.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SubjectSet(u16);

impl SubjectSet {
    pub(crate) const fn contains(self, subject: ActionSubject) -> bool {
        self.0 & subject.bit() != 0
    }

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn with(self, subject: ActionSubject) -> Self {
        Self(self.0 | subject.bit())
    }
}

/// A configuration's place in a forced merge: 0 for the receiver, then
/// 1, 2, … for the parts it absorbs, in fold order.
pub(crate) type FoldPlace = usize;

/// Who supplies each standard action of a forced merge (ADR-0392 §2).
///
/// Built from the receiver and every part the merge will absorb, before any
/// is absorbed, so each owner is the one of the merged result; then
/// [`SemanticsConfiguration::forced_merge`] takes each part by it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActionPlan {
    /// The owner's place of each subject of `OWNED`, if any declares it.
    owners: [Option<FoldPlace>; OWNED.len()],
    /// The subjects whose owner acts on them.
    acting: SubjectSet,
    /// What `Click` changes on the merged node.
    click: ActionSubject,
    /// Whether the merged node publishes a numeric range.
    has_range: bool,
    /// The place whose `enabled` the node publishes, when the owner of
    /// `Click`'s subject declares one.
    enabled_from: Option<FoldPlace>,
    /// Whether the merged node announces no `expanded` (see `new`).
    drops_expanded: bool,
}

impl ActionPlan {
    /// Plans a forced merge of `receiver` with `parts`, in fold order.
    ///
    /// `parts` is walked a few times; it is the fragments assembly already
    /// holds, so a walk allocates nothing.
    pub(crate) fn new<'a, I>(receiver: &SemanticsConfiguration, parts: I) -> Self
    where
        I: Iterator<Item = &'a SemanticsConfiguration> + Clone,
    {
        let all = || {
            std::iter::once(receiver).chain(
                parts
                    .clone()
                    .map(|part| -> &SemanticsConfiguration { part }),
            )
        };
        let at = |place: FoldPlace| all().nth(place);
        // The place whose role the node publishes: a later part replaces the
        // role only with a more specific one, as `absorb` does.
        let mut role = SemanticsRole::None;
        let mut role_place = None;
        for (place, config) in all().enumerate() {
            let merged = more_specific_role(role, config.role());
            if merged != role {
                role = merged;
                role_place = Some(place);
            }
        }
        let mut owners = [None; OWNED.len()];
        for (slot, subject) in owners.iter_mut().zip(OWNED) {
            // A state the role gives its meaning comes with the role, else
            // from the first part that declares it.
            let with_role = matches!(subject, ActionSubject::Toggle | ActionSubject::Selection)
                .then_some(role_place)
                .flatten()
                .filter(|place| at(*place).is_some_and(|config| subject.is_declared_by(config)));
            *slot = with_role.or_else(|| all().position(|config| subject.is_declared_by(config)));
        }
        let owner_of = |subject: ActionSubject| {
            let index = OWNED.iter().position(|owned| *owned == subject)?;
            at(owners[index]?)
        };
        let has_range = owner_of(ActionSubject::Range).is_some();
        // A state counts only where the published role carries it: what the
        // node does not announce cannot be what its click changes.
        let announced = |subject: ActionSubject, attribute: RoleAttribute| {
            owner_of(subject).is_some() && applicable(role, attribute)
        };
        // A click-only disclosure is one its click expands: a header that
        // registers nothing only because it is disabled is not one, so it
        // does not take the click of an enabled part beside it.
        let click_only_disclosure = owner_of(ActionSubject::Disclosure).is_some_and(|owner| {
            (owner.has_action(SemanticsAction::Click)
                || owner
                    .withheld_subjects()
                    .contains(ActionSubject::Disclosure))
                && !owner.has_action(SemanticsAction::Expand)
                && !owner.has_action(SemanticsAction::Collapse)
        });
        let click = if announced(ActionSubject::Toggle, RoleAttribute::Toggled) {
            ActionSubject::Toggle
        } else if announced(ActionSubject::Selection, RoleAttribute::Selected) {
            ActionSubject::Selection
        } else if click_only_disclosure
            && announced(ActionSubject::Disclosure, RoleAttribute::Expanded)
        {
            ActionSubject::Disclosure
        } else {
            ActionSubject::Activation
        };
        // A click-only disclosure whose click another state owns cannot be
        // expanded through the node, which then announces no `expanded`.
        let drops_expanded = click_only_disclosure
            && matches!(click, ActionSubject::Toggle | ActionSubject::Selection);
        let acting = OWNED
            .into_iter()
            .filter(|subject| {
                owner_of(*subject)
                    .is_some_and(|owner| subject.is_acted_on_by(owner, click, has_range))
            })
            .fold(SubjectSet::default(), SubjectSet::with);
        // The node announces enabled as the part whose handler its click
        // runs: the owner of the click's subject when it acts on it, else the
        // first part registering `Click`.
        let click_place = match OWNED.iter().position(|owned| *owned == click) {
            Some(index) if acting.contains(click) => owners[index],
            _ => all().position(|config| config.has_action(SemanticsAction::Click)),
        };
        let enabled_from = click_place
            .filter(|place| at(*place).is_some_and(|owner| owner.states().enabled().is_some()));
        Self {
            owners,
            acting,
            click,
            has_range,
            enabled_from,
            drops_expanded,
        }
    }

    /// The subject `action` changes on the merged node.
    fn subject(&self, action: SemanticsAction) -> Option<ActionSubject> {
        ActionSubject::classify(action, self.click, self.has_range)
    }

    /// The owner's place of `subject`, if a configuration declares it.
    pub(crate) fn owner(&self, subject: ActionSubject) -> Option<FoldPlace> {
        let index = OWNED.iter().position(|owned| *owned == subject)?;
        self.owners[index]
    }

    /// Whether the configuration at `place` may supply `action`'s handler:
    /// the acting owner of its subject alone, or any configuration when the
    /// subject has no acting owner (the first to register it keeps it).
    pub(crate) fn admits(&self, action: SemanticsAction, place: FoldPlace) -> bool {
        match self.subject(action) {
            None => false,
            Some(subject) if self.acting.contains(subject) => self.owner(subject) == Some(place),
            Some(_) => true,
        }
    }

    /// The place whose `enabled` the merged node publishes, when it is not
    /// ADR-0206's receiver-first one.
    pub(crate) const fn enabled_from(&self) -> Option<FoldPlace> {
        self.enabled_from
    }

    /// The subjects whose owner acted but whose handlers `merged`, the
    /// node this plan built, offers none of: an outer merge still reads it as
    /// acting on them (ADR-0392 §3).
    fn withheld(&self, merged: &SemanticsConfiguration) -> SubjectSet {
        OWNED
            .into_iter()
            .filter(|subject| self.acting.contains(*subject))
            .filter(|subject| {
                !merged
                    .registered_actions()
                    .any(|action| self.subject(action) == Some(*subject))
            })
            .fold(SubjectSet::default(), SubjectSet::with)
    }

    /// Settles `merged`, the node this plan built: it records the subjects
    /// whose owner acted but whose handlers it offers none of, so that an
    /// outer fold still reads it as acting on them (ADR-0392 §3), and drops
    /// an `expanded` its click cannot change.
    pub(crate) fn settle(&self, merged: &mut SemanticsConfiguration) {
        let withheld = self.withheld(merged);
        merged.withhold_subjects(withheld);
        if self.drops_expanded {
            merged.clear_expanded();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each action's subject on a node that declares nothing, and the
    /// subject a declaration moves it to. The list names every action, so a
    /// new one fails here until `ActionSubject::classify` places it.
    #[test]
    fn every_action_names_its_subject() {
        use ActionSubject as S;
        use SemanticsAction as A;
        let plain = SemanticsConfiguration::new();
        let expected = [
            (A::Click, Some(S::Activation)),
            (A::Focus, Some(S::Focus)),
            (A::Blur, Some(S::Focus)),
            (A::Expand, Some(S::Disclosure)),
            (A::Collapse, Some(S::Disclosure)),
            (A::Increment, Some(S::Scroll)),
            (A::Decrement, Some(S::Scroll)),
            (A::ScrollUp, Some(S::Scroll)),
            (A::ScrollDown, Some(S::Scroll)),
            (A::ScrollLeft, Some(S::Scroll)),
            (A::ScrollRight, Some(S::Scroll)),
            (A::ScrollIntoView, Some(S::Scroll)),
            (A::SetScrollOffset, Some(S::Scroll)),
            (A::SetTextSelection, Some(S::Text)),
            (A::ReplaceSelectedText, Some(S::Text)),
            (A::SetValue, Some(S::Value)),
            (A::SetNumericValue, Some(S::Scroll)),
            (A::ShowContextMenu, Some(S::Activation)),
            (A::ShowTooltip, Some(S::Tooltip)),
            (A::HideTooltip, Some(S::Tooltip)),
            (A::CustomAction, None),
        ];
        assert_eq!(
            expected
                .iter()
                .map(|(action, _)| *action)
                .collect::<Vec<_>>(),
            A::ALL,
            "every action is listed once, in declaration order"
        );
        for (action, subject) in expected {
            assert_eq!(ActionSubject::of(action, &plain), subject, "{action:?}");
        }

        let mut toggle = SemanticsConfiguration::new();
        toggle.set_toggled(Some(crate::Toggled::Off));
        toggle.set_selected(false);
        toggle.set_expanded(false);
        assert_eq!(ActionSubject::of(A::Click, &toggle), Some(S::Toggle));

        let mut selectable = SemanticsConfiguration::new();
        selectable.set_selected(false);
        selectable.set_expanded(false);
        assert_eq!(ActionSubject::of(A::Click, &selectable), Some(S::Selection));

        let mut disclosure = SemanticsConfiguration::new();
        disclosure.set_expanded(false);
        assert_eq!(
            ActionSubject::of(A::Click, &disclosure),
            Some(S::Activation),
            "an expanded state nothing clicks is not a click-only disclosure"
        );
        disclosure.add_action(A::Click, std::rc::Rc::new(|_| {}));
        assert_eq!(
            ActionSubject::of(A::Click, &disclosure),
            Some(S::Disclosure)
        );
        disclosure.add_action(A::Expand, std::rc::Rc::new(|_| {}));
        assert_eq!(
            ActionSubject::of(A::Click, &disclosure),
            Some(S::Activation),
            "a disclosure with its own Expand does not route it through Click"
        );

        let mut range = SemanticsConfiguration::new();
        range.set_numeric_range(
            crate::NumericRange::try_new(0.0, 0.0, 1.0, 0.1).expect("a valid range"),
        );
        for action in [A::Increment, A::Decrement, A::SetNumericValue] {
            assert_eq!(ActionSubject::of(action, &range), Some(S::Range));
        }
    }
}
