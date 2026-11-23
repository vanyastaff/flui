//! Which configuration's handler a merged node's standard action runs
//! (ADR-0392).
//!
//! Each standard action changes one group of a node's attributes, its
//! subject. The configuration whose declaration of that group the node
//! publishes owns the group, and an owner that acts on its group supplies
//! every handler of it: the state a node announces and the action that
//! changes it come from one control, in whatever order the fold meets them.

use crate::{SemanticsAction, SemanticsConfiguration};

/// The group of a node's attributes a standard action changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionSubject {
    /// Nothing the node publishes: the first handler in reading order runs.
    Activation,
    /// `toggled`, which the role gives its meaning (ADR-0206 decision 3).
    Toggle,
    /// `expanded`.
    Disclosure,
    /// The numeric range.
    Range,
    /// The scroll group: position, axis and extents.
    Scroll,
    /// The value and the text content.
    Text,
    /// `focusable` and `focused`.
    Focus,
    /// The tooltip.
    Tooltip,
}

impl ActionSubject {
    /// The subjects a configuration can own; `Activation` has no owner.
    pub(crate) const OWNED: [Self; 7] = [
        Self::Toggle,
        Self::Disclosure,
        Self::Range,
        Self::Scroll,
        Self::Text,
        Self::Focus,
        Self::Tooltip,
    ];

    /// The subject `action` changes on a node configured as `node`, `None`
    /// for custom actions, which each keep their own handler.
    pub(crate) fn of(action: SemanticsAction, node: &SemanticsConfiguration) -> Option<Self> {
        use SemanticsAction as A;
        // `SemanticsAction` is `#[non_exhaustive]`: `every_action_names_its_subject`
        // lists each action, so a new one fails it until it is placed here.
        Some(match action {
            A::Click => {
                if node.toggled().is_some() {
                    Self::Toggle
                } else if node.expanded().is_some()
                    && !node.has_action(A::Expand)
                    && !node.has_action(A::Collapse)
                {
                    // The click-only disclosure `tap_disclosure_transition`
                    // routes Expand and Collapse through.
                    Self::Disclosure
                } else {
                    Self::Activation
                }
            }
            A::Expand | A::Collapse => Self::Disclosure,
            A::Increment | A::Decrement | A::SetNumericValue => {
                if node.numeric_range().is_some() {
                    Self::Range
                } else {
                    Self::Scroll
                }
            }
            A::ScrollUp | A::ScrollDown | A::ScrollLeft | A::ScrollRight | A::SetScrollOffset => {
                Self::Scroll
            }
            A::SetValue | A::SetTextSelection | A::ReplaceSelectedText => Self::Text,
            A::Focus | A::Blur => Self::Focus,
            A::ShowTooltip | A::HideTooltip => Self::Tooltip,
            A::ScrollIntoView | A::ShowContextMenu => Self::Activation,
            A::CustomAction => return None,
            _ => Self::Activation,
        })
    }

    /// Whether `config` declares the group this subject changes.
    pub(crate) fn is_declared_by(self, config: &SemanticsConfiguration) -> bool {
        let states = config.states();
        match self {
            Self::Activation => false,
            Self::Toggle => states.toggled().is_some(),
            Self::Disclosure => states.expanded().is_some(),
            Self::Range => config.numeric_range().is_some(),
            Self::Scroll => config.declares_scroll_group(),
            Self::Text => config.value().is_some() || config.text_content().is_some(),
            Self::Focus => states.focusable().is_some() || states.focused().is_some(),
            Self::Tooltip => config.tooltip().is_some(),
        }
    }

    /// Whether `config` owns this subject and acts on it: it declares the
    /// group and registers one of its actions, a blocked one included, or
    /// declares itself disabled. Such an owner supplies every handler of the
    /// subject, and one it does not offer is offered by nobody.
    pub(crate) fn is_acted_on_by(self, config: &SemanticsConfiguration) -> bool {
        self.is_declared_by(config)
            && (config.states().enabled() == Some(false)
                || config
                    .registered_actions()
                    .any(|action| Self::of(action, config) == Some(self)))
    }

    /// The position of this subject in a [`SubjectSet`].
    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A set of [`ActionSubject`]s.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SubjectSet(u8);

impl SubjectSet {
    /// The subjects of `OWNED` for which `keep` holds.
    pub(crate) fn of(keep: impl Fn(ActionSubject) -> bool) -> Self {
        Self(
            ActionSubject::OWNED
                .into_iter()
                .filter(|subject| keep(*subject))
                .fold(0, |bits, subject| bits | subject.bit()),
        )
    }

    pub(crate) const fn contains(self, subject: ActionSubject) -> bool {
        self.0 & subject.bit() != 0
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each action's subject on a node that declares nothing, and the
    /// subject a declaration moves it to. The list names every action, so a
    /// new one fails here until `ActionSubject::of` places it.
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
            (A::ScrollIntoView, Some(S::Activation)),
            (A::SetScrollOffset, Some(S::Scroll)),
            (A::SetTextSelection, Some(S::Text)),
            (A::ReplaceSelectedText, Some(S::Text)),
            (A::SetValue, Some(S::Text)),
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
        toggle.set_expanded(false);
        assert_eq!(ActionSubject::of(A::Click, &toggle), Some(S::Toggle));

        let mut disclosure = SemanticsConfiguration::new();
        disclosure.set_expanded(false);
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
