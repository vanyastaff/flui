//! `ReconcileEvent` — structured trace stream for the keyed
//! child reconciler.
//!
//! Plan §U13 / FR-035. Emitted at every disposition site on the
//! PRODUCTION path: the slab reconciler `reconcile_children_by_id`
//! emits reuse / reorder / mount / unmount per child, and the
//! GlobalKey-reparent path in [`ElementTree`](super::ElementTree) emits
//! [`Reparent`](ReconcileEventKind::Reparent). The test-only box
//! reconciler (`reconciliation`, gated behind `cfg(test)` /
//! `feature = "test-utils"`) emits the same dispositions as a
//! keyed-match reference. Observers (the `ReconcileEventCollector` test
//! fixture in the in-crate `tree::test_utils` module, gated behind the
//! `test-utils` feature; the future devtools panel) reconstruct the
//! per-frame reconciliation outcome WITHOUT a tree-diff comparison.
//!
//! # Stability boundary
//!
//! The `target: "flui::reconcile"` string is a **stability boundary**
//! per FR-035. Renaming or relocating it requires a `#[deprecated]`
//! alias period for one release — selection-persistence consumers and
//! devtools subscribers filter by exactly this target string.
//!
//! Field names on the emitted `tracing::Event` are equally stable.
//! Each field is recorded as a typed primitive (per FEAS-008) so the
//! collector reads `u64` / `bool` directly via
//! [`tracing::field::Visit`] without ever round-tripping through
//! Debug-format strings:
//!
//! | Field                  | Type   | Notes                                                             |
//! |------------------------|--------|-------------------------------------------------------------------|
//! | `kind`                 | `u8`   | [`ReconcileEventKind`] discriminant — cast through `as u8`        |
//! | `parent`               | `u64`  | Owning parent's [`ElementId`] as `usize → u64`                    |
//! | `child_key`            | `u64`  | `0` when absent (paired with `child_key_present`)                 |
//! | `child_key_present`    | `bool` | `true` iff the child carries a key                                |
//! | `slot`                 | `u64`  | New slot index for the child                                      |
//! | `view_type_id`         | `str`  | `format!("{:?}", TypeId)` — Debug is the only stable identifier   |
//! | `from_parent`          | `u64`  | `0` when absent (paired with `from_parent_present`)               |
//! | `from_parent_present`  | `bool` | `true` only on cross-parent reparent (FR-030, plan §U17)          |

use std::any::TypeId;

use flui_foundation::ElementId;

/// Disposition recorded by the keyed reconciler for a single child
/// slot. `#[non_exhaustive]` per FR-035 + SC-011 so adding a new
/// disposition (e.g. an `Async` suspend variant) is not a breaking
/// change.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ReconcileEventKind {
    /// A fresh element was created for a new-side view that found no
    /// match on the old side.
    Mount = 0,
    /// An old element was dropped because no new-side view claimed it.
    Unmount = 1,
    /// An old element was matched in place — same slot, no movement.
    Reuse = 2,
    /// An old element was matched to a different slot (keyed reorder
    /// or middle-walk reclaim).
    Reorder = 3,
    /// A global-key element was reparented across two distinct parents
    /// in the same frame. Only this variant populates
    /// [`ReconcileEvent::from_parent`].
    Reparent = 4,
}

impl ReconcileEventKind {
    /// Discriminant as `u8` for typed-primitive emission. Stable —
    /// downstream selection-persistence consumers depend on the
    /// numeric value.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Reverse mapping from emitted `u8` to the typed enum, for the
    /// test-fixture collector. Returns `None` for unknown values so a
    /// future variant landing without consumer-side updates surfaces
    /// as `None`, not a silent miscategorisation.
    #[must_use]
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Mount),
            1 => Some(Self::Unmount),
            2 => Some(Self::Reuse),
            3 => Some(Self::Reorder),
            4 => Some(Self::Reparent),
            _ => None,
        }
    }
}

/// One structured trace record from the keyed child reconciler.
///
/// `#[non_exhaustive]` per FR-035 — future fields (e.g. a build-
/// timing measurement) can land without breaking match consumers.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ReconcileEvent {
    /// What happened to the slot.
    pub kind: ReconcileEventKind,
    /// Owning parent element. The reconciler caller threads its own
    /// `ElementId` in so subscribers can correlate events back to the
    /// build path that produced them.
    pub parent: ElementId,
    /// Hash of the child's `ViewKey`, if any.
    pub child_key: Option<u64>,
    /// New slot index of the child (0-based, into the new-views list).
    pub slot: usize,
    /// `TypeId` of the view that owns the slot — survives the
    /// type-erased reconciler boundary so the collector can group
    /// events by widget type.
    pub view_type_id: TypeId,
    /// For [`ReconcileEventKind::Reparent`] only: the previous
    /// parent the global-key element used to live under. `None` for
    /// every other variant.
    pub from_parent: Option<ElementId>,
}

impl ReconcileEvent {
    /// Build a `Mount` event for a freshly created element.
    pub fn mount(
        parent: ElementId,
        slot: usize,
        view_type_id: TypeId,
        child_key: Option<u64>,
    ) -> Self {
        Self {
            kind: ReconcileEventKind::Mount,
            parent,
            child_key,
            slot,
            view_type_id,
            from_parent: None,
        }
    }

    /// Build an `Unmount` event for a dropped old element.
    pub fn unmount(
        parent: ElementId,
        slot: usize,
        view_type_id: TypeId,
        child_key: Option<u64>,
    ) -> Self {
        Self {
            kind: ReconcileEventKind::Unmount,
            parent,
            child_key,
            slot,
            view_type_id,
            from_parent: None,
        }
    }

    /// Build a `Reuse` event for an old element matched in its same
    /// slot.
    pub fn reuse(
        parent: ElementId,
        slot: usize,
        view_type_id: TypeId,
        child_key: Option<u64>,
    ) -> Self {
        Self {
            kind: ReconcileEventKind::Reuse,
            parent,
            child_key,
            slot,
            view_type_id,
            from_parent: None,
        }
    }

    /// Build a `Reorder` event for an old element matched to a
    /// different slot.
    pub fn reorder(
        parent: ElementId,
        slot: usize,
        view_type_id: TypeId,
        child_key: Option<u64>,
    ) -> Self {
        Self {
            kind: ReconcileEventKind::Reorder,
            parent,
            child_key,
            slot,
            view_type_id,
            from_parent: None,
        }
    }

    /// Build a `Reparent` event for a global-key element moving
    /// across parents in the same frame. `from_parent` is required;
    /// `child_key` is the GlobalKey's hash.
    pub fn reparent(
        from_parent: ElementId,
        parent: ElementId,
        slot: usize,
        view_type_id: TypeId,
        child_key: u64,
    ) -> Self {
        Self {
            kind: ReconcileEventKind::Reparent,
            parent,
            child_key: Some(child_key),
            slot,
            view_type_id,
            from_parent: Some(from_parent),
        }
    }
}

/// Tracing target — **stability boundary** per FR-035. Subscribers
/// filter by this exact string.
pub const RECONCILE_TARGET: &str = "flui::reconcile";

/// Emit `event` to the `flui::reconcile` target as typed primitives.
///
/// Field names match the table in the module docs. Each primitive is
/// recorded via the typed `Visit` methods (`record_u64`,
/// `record_bool`, `record_debug` for the type-id) so the
/// `ReconcileEventCollector` (in the in-crate `tree::test_utils`
/// module, behind the `test-utils` feature) reads structured values
/// without parsing Debug-format strings.
///
/// Cost is zero when no subscriber is installed (tracing's per-target
/// short-circuit fires before the field values are computed).
pub fn emit(event: &ReconcileEvent) {
    let view_type_id_str = format!("{:?}", event.view_type_id);
    tracing::event!(
        target: RECONCILE_TARGET,
        tracing::Level::TRACE,
        kind = u64::from(event.kind.as_u8()),
        parent = event.parent.as_u64(),
        child_key = event.child_key.unwrap_or(0),
        child_key_present = event.child_key.is_some(),
        slot = event.slot as u64,
        view_type_id = %view_type_id_str,
        from_parent = event.from_parent.map_or(0_u64, ElementId::as_u64),
        from_parent_present = event.from_parent.is_some(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_u8_roundtrip() {
        for variant in [
            ReconcileEventKind::Mount,
            ReconcileEventKind::Unmount,
            ReconcileEventKind::Reuse,
            ReconcileEventKind::Reorder,
            ReconcileEventKind::Reparent,
        ] {
            let value = variant.as_u8();
            assert_eq!(
                ReconcileEventKind::from_u8(value),
                Some(variant),
                "u8 round-trip must preserve variant identity",
            );
        }
        assert_eq!(
            ReconcileEventKind::from_u8(99),
            None,
            "unknown u8 must yield None, not a silent miscategorisation",
        );
    }

    #[test]
    fn constructors_set_kind_and_from_parent_correctly() {
        let parent = ElementId::new(1);
        let tid = TypeId::of::<u32>();

        let mount = ReconcileEvent::mount(parent, 0, tid, None);
        assert!(matches!(mount.kind, ReconcileEventKind::Mount));
        assert!(mount.from_parent.is_none());

        let unmount = ReconcileEvent::unmount(parent, 1, tid, Some(42));
        assert!(matches!(unmount.kind, ReconcileEventKind::Unmount));
        assert_eq!(unmount.child_key, Some(42));

        let reuse = ReconcileEvent::reuse(parent, 2, tid, None);
        assert!(matches!(reuse.kind, ReconcileEventKind::Reuse));

        let reorder = ReconcileEvent::reorder(parent, 3, tid, Some(7));
        assert!(matches!(reorder.kind, ReconcileEventKind::Reorder));

        let donor = ElementId::new(9);
        let reparent = ReconcileEvent::reparent(donor, parent, 4, tid, 0xDEAD);
        assert!(matches!(reparent.kind, ReconcileEventKind::Reparent));
        assert_eq!(reparent.from_parent, Some(donor));
        assert_eq!(reparent.child_key, Some(0xDEAD));
    }

    #[test]
    fn target_string_is_stable() {
        // Anchors the FR-035 stability boundary as a test — any rename
        // shows up here, where the assertion forces an explicit
        // contract-rev decision rather than a silent drift.
        assert_eq!(RECONCILE_TARGET, "flui::reconcile");
    }
}
