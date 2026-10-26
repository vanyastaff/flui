//! What an agent asks an element to do and what it answers: the semantics action, the
//! [`Effect`] an action had, and the [`Actionability`] facts checked before pointer input.

use crate::geometry::{LogicalPoint, LogicalRect};
use crate::input::InputPath;
use crate::locator::Target;
use crate::snapshot::{Snapshot, SnapshotReading};
use crate::tree::{ElementId, ElementRef, TreeRevision};
use crate::wire::ActionName;

/// One action on one element: a tool the node advertised in its `actions`,
/// addressed by its handle.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ActionRequest {
    /// The element to act on.
    pub element: ElementId,
    /// What to do, one of the element's advertised actions.
    pub action: ActionName,
    /// The new value, for `set_value`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub value: Option<String>,
}

impl ActionRequest {
    /// `action` on `element`, with no value.
    #[must_use]
    pub fn new(element: ElementId, action: ActionName) -> Self {
        Self {
            element,
            action,
            value: None,
        }
    }

    /// `set_value` on `element`, to `value`.
    #[must_use]
    pub fn set_value(element: ElementId, value: impl Into<String>) -> Self {
        Self {
            element,
            action: ActionName::SetValue,
            value: Some(value.into()),
        }
    }
}

vocabulary! {
    /// How much of an action reached the app (ADR-0080 "Errors", R13).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum EffectKind {
        /// It ran: for `os` input, the app's input journal observed the events.
        Ran => "ran",
        /// It may have run: read the tree for its effect rather than acting again.
        MayHaveRun => "may_have_run",
        /// Part of it went out; `sent` of `total` units.
        Partial => "partial",
        /// Something else ran as a side effect, such as focus moving first.
        Incidental => "incidental",
    }
}

vocabulary! {
    /// What a partial effect counts.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum EffectUnit {
        /// Input events.
        Event => "event",
        /// Ticks of an action sequence.
        Tick => "tick",
        /// Characters of typed text.
        Character => "character",
        /// Key chords.
        Chord => "chord",
    }
}

/// ADR-0080's effect object: `{kind, detail?, sent?, total?, unit?}`.
///
/// A count outranks an inner effect: `partial` carries the next unit's own kind in `detail`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Effect {
    /// How much reached the app.
    pub kind: EffectKind,
    /// What went out, for people.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub detail: Option<String>,
    /// Units sent, for `partial`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub sent: Option<u32>,
    /// Units in the action, for `partial`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub total: Option<u32>,
    /// What the units are, for `partial`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub unit: Option<EffectUnit>,
}

impl Effect {
    /// An effect of `kind` with nothing more to say.
    #[must_use]
    pub const fn new(kind: EffectKind) -> Self {
        Self {
            kind,
            detail: None,
            sent: None,
            total: None,
            unit: None,
        }
    }

    /// `sent` of `total` units went out.
    #[must_use]
    pub const fn partial(sent: u32, total: u32, unit: EffectUnit) -> Self {
        Self {
            kind: EffectKind::Partial,
            detail: None,
            sent: Some(sent),
            total: Some(total),
            unit: Some(unit),
        }
    }

    /// The effect, saying what went out.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// `semantics.act`: one semantics action on one element (the `semantic` input path).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ActParams {
    /// The element; a point is refused, since a semantics action has no position.
    pub target: Target,
    /// What to do, one of the element's advertised actions.
    pub action: ActionName,
    /// The new value, for `set_value`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub value: Option<String>,
    /// What to read back once the action settled.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub snapshot: Snapshot,
}

impl ActParams {
    /// `action` on `target`, reading nothing back.
    #[must_use]
    pub fn new(target: impl Into<Target>, action: ActionName) -> Self {
        Self {
            target: target.into(),
            action,
            value: None,
            snapshot: Snapshot::None,
        }
    }

    /// `set_value` on `target`, to `value`.
    #[must_use]
    pub fn set_value(target: impl Into<Target>, value: impl Into<String>) -> Self {
        Self {
            value: Some(value.into()),
            ..Self::new(target, ActionName::SetValue)
        }
    }

    /// Read `snapshot` back.
    #[must_use]
    pub fn with_snapshot(mut self, snapshot: Snapshot) -> Self {
        self.snapshot = snapshot;
        self
    }
}

/// A range of input-journal sequence numbers, `from..=to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct JournalSpan {
    /// The first entry.
    pub from: u64,
    /// The last entry.
    pub to: u64,
}

/// What an action answers: the path it took, how much ran, and the reading asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ActOutcome {
    /// The input path the action took.
    pub via: InputPath,
    /// How much of it reached the app.
    pub effect: Effect,
    /// The element acted on, once a locator was resolved.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub element: Option<ElementRef>,
    /// The input-journal entries the action produced.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub observed: Option<JournalSpan>,
    /// The reading asked for.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub snapshot: Option<SnapshotReading>,
    /// The action ran but the tree could not be read back afterwards (ADR-0080): a success.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub readback_failed: bool,
}

impl ActOutcome {
    /// An action through `via` with `effect`, and nothing else to report.
    #[must_use]
    pub const fn new(via: InputPath, effect: Effect) -> Self {
        Self {
            via,
            effect,
            element: None,
            observed: None,
            snapshot: None,
            readback_failed: false,
        }
    }
}

vocabulary! {
    /// One check of actionability (ADR-0238 §6), in the order the driver applies them.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ActionabilityCheck {
        /// The element is in the published tree.
        Attached => "attached",
        /// Non-empty, inside the window's visible region and every clipping ancestor.
        Visible => "visible",
        /// The same bounds as on the previous frame.
        Stable => "stable",
        /// Neither it nor an ancestor is disabled.
        Enabled => "enabled",
        /// The app's hit-test at the action point reaches it.
        Receives => "receives",
    }
}

/// `element.actionability`: the facts about one element, all from one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ActionabilityParams {
    /// The element.
    pub element: ElementRef,
    /// Where the action would land; the centre of the element's visible part when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub point: Option<LogicalPoint>,
}

impl ActionabilityParams {
    /// The facts about `element` at its centre.
    #[must_use]
    pub const fn new(element: ElementRef) -> Self {
        Self {
            element,
            point: None,
        }
    }
}

/// What `element.actionability` answers.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Actionability {
    /// The element is in the published tree; the other facts are false when it is not.
    pub attached: bool,
    /// Its bounds in the window's surface.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub rect: Option<LogicalRect>,
    /// Non-empty and inside the window's visible region and every clipping ancestor.
    pub visible: bool,
    /// The same bounds as on the previous frame.
    pub stable: bool,
    /// Neither it nor an ancestor is disabled.
    pub enabled: bool,
    /// The hit-test at `point` reaches it or a node under it.
    pub receives: bool,
    /// The point the hit-test used.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub point: Option<LogicalPoint>,
    /// What the hit-test reached instead, as its outline line, when it does not receive.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub covered_by: Option<String>,
    /// The revision the facts were read on.
    pub revision: TreeRevision,
}

impl Actionability {
    /// The facts of an element at `revision` with none of them holding yet: `attached` as given,
    /// and no bounds, point or cover.
    #[must_use]
    pub const fn new(attached: bool, revision: TreeRevision) -> Self {
        Self {
            attached,
            rect: None,
            visible: false,
            stable: false,
            enabled: false,
            receives: false,
            point: None,
            covered_by: None,
            revision,
        }
    }

    /// The first check that fails, in the driver's order; `None` when the element is
    /// actionable.
    #[must_use]
    pub fn first_failed(&self) -> Option<ActionabilityCheck> {
        [
            (self.attached, ActionabilityCheck::Attached),
            (self.visible, ActionabilityCheck::Visible),
            (self.stable, ActionabilityCheck::Stable),
            (self.enabled, ActionabilityCheck::Enabled),
            (self.receives, ActionabilityCheck::Receives),
        ]
        .into_iter()
        .find_map(|(holds, check)| (!holds).then_some(check))
    }
}
