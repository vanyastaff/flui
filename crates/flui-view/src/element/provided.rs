//! Values an element of any kind publishes to its subtree (ADR-0322 §5).
//!
//! An [`InheritedView`](crate::InheritedView) is an element whose only job is
//! to publish itself. A view or a state that already has an element publishes
//! from it instead: its `provide` hook writes each value into a [`Provided`]
//! sink, the element registers the values' types in its node's inherited
//! scope, and a descendant's `ctx.depend_on::<T, _>(..)` resolves the nearest
//! provider of `T` whichever kind of element it is.
//!
//! Dependents live in the same [`Dependents`] ledger an inherited element
//! keeps. Each published type owns one bit of the dependent's
//! [`FieldSet`](crate::view::FieldSet), so a value that changes notifies only
//! the elements that read that value.

use std::any::{Any, TypeId};
use std::collections::HashMap;

use flui_foundation::ElementId;
use smallvec::SmallVec;

use crate::view::FieldSet;

/// A value an element publishes to its subtree.
///
/// Descendants read it with `ctx.depend_on::<T, _>(..)` and rebuild when a
/// later value answers `true` here.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be published to a subtree",
    label = "a published value says when its dependents rebuild",
    note = "implement `ProvidedValue`: `fn update_should_notify(&self, old: &Self) -> bool`"
)]
pub trait ProvidedValue: 'static {
    /// Whether the elements that read `old` must rebuild now that the value
    /// is `self`.
    fn update_should_notify(&self, old: &Self) -> bool;
}

/// The object-safe face of one published value.
trait ErasedValue {
    fn as_any(&self) -> &dyn Any;
    /// `old` is the previous value of the same type.
    fn should_notify(&self, old: &dyn Any) -> bool;
}

impl<T: ProvidedValue> ErasedValue for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn should_notify(&self, old: &dyn Any) -> bool {
        // A slot is replaced only by a value of its own type: the caller pairs
        // values by `TypeId`.
        old.downcast_ref::<T>()
            .is_none_or(|old| self.update_should_notify(old))
    }
}

struct Slot {
    type_id: TypeId,
    value: Box<dyn ErasedValue>,
}

/// Where a `provide` hook writes the values its element publishes.
///
/// Publishing two values of one type keeps the last.
pub struct Provided<'a> {
    staged: &'a mut SmallVec<[Slot; 2]>,
}

impl std::fmt::Debug for Provided<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provided")
            .field("values", &self.staged.len())
            .finish()
    }
}

impl Provided<'_> {
    /// Publish `value` to the subtree under this element.
    pub fn value<T: ProvidedValue>(&mut self, value: T) {
        let type_id = TypeId::of::<T>();
        let value: Box<dyn ErasedValue> = Box::new(value);
        if let Some(slot) = self.staged.iter_mut().find(|slot| slot.type_id == type_id) {
            slot.value = value;
        } else {
            self.staged.push(Slot { type_id, value });
        }
    }
}

/// One dependent of a provider: where it sits and what it read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DependentEntry {
    /// The dependent's depth in the element tree, for the dirty heap.
    pub depth: usize,
    /// The fields read in the dependent's latest build.
    pub(crate) mask: FieldSet,
    /// The fields read in a lifecycle hook, kept until the dependent unmounts.
    pub(crate) lifecycle_mask: FieldSet,
    /// Registration order, so same-depth dependents are scheduled in the
    /// order they first read the provider.
    pub(crate) registered: u64,
}

impl DependentEntry {
    /// Every field this dependent is notified for: build reads plus
    /// lifecycle reads.
    #[must_use]
    pub(crate) fn fields(&self) -> FieldSet {
        self.mask | self.lifecycle_mask
    }
}

/// The forward half of the dependency ledger: who reads this provider.
///
/// One per provider element, whatever it publishes. The reverse half is
/// `BuildOwner`'s `InheritedDependencies`.
#[derive(Debug, Default)]
pub(crate) struct Dependents {
    entries: HashMap<ElementId, DependentEntry>,
    /// The registration stamp the next new dependent receives.
    next_registration: u64,
}

impl Dependents {
    fn entry(&mut self, element: ElementId, depth: usize) -> &mut DependentEntry {
        let next_registration = &mut self.next_registration;
        let entry = self.entries.entry(element).or_insert_with(|| {
            let registered = *next_registration;
            // Saturating: past 2^64 registrations the element id orders the tie.
            *next_registration = registered.saturating_add(1);
            DependentEntry {
                depth,
                mask: FieldSet::NONE,
                lifecycle_mask: FieldSet::NONE,
                registered,
            }
        });
        entry.depth = depth;
        entry
    }

    /// Record a read made in `build`. Re-registering overwrites the depth and
    /// unions the fields.
    pub(crate) fn add(&mut self, element: ElementId, depth: usize, mask: FieldSet) {
        self.entry(element, depth).mask |= mask;
    }

    /// Record a read made in a lifecycle hook: kept until the dependent
    /// unmounts.
    pub(crate) fn add_lifecycle(&mut self, element: ElementId, depth: usize, mask: FieldSet) {
        self.entry(element, depth).lifecycle_mask |= mask;
    }

    pub(crate) fn reset_mask(&mut self, element: ElementId) {
        if let Some(entry) = self.entries.get_mut(&element) {
            entry.mask = FieldSet::NONE;
        }
    }

    /// Drop `element` if it no longer reads any field. Returns whether an
    /// entry was removed.
    pub(crate) fn prune_unread(&mut self, element: ElementId) -> bool {
        match self.entries.get(&element) {
            Some(entry) if entry.fields().is_empty() => {
                self.entries.remove(&element);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn remove(&mut self, element: ElementId) {
        self.entries.remove(&element);
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn map(&self) -> &HashMap<ElementId, DependentEntry> {
        &self.entries
    }

    /// Schedule every dependent that read a field of `changed`, in
    /// registration order, with its `did_change_dependencies` pending.
    pub(crate) fn notify(&self, changed: FieldSet, owner: &mut crate::ElementOwner<'_>) {
        if changed.is_empty() {
            return;
        }
        // The map iterates in its hash order, which changes from map to map.
        // Scheduling in registration order (the dependents' first build
        // order) lets the drain rebuild same-depth dependents in the same
        // order every time.
        let mut notified: Vec<(u64, ElementId, usize)> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.fields().intersects(changed))
            .map(|(&id, entry)| (entry.registered, id, entry.depth))
            .collect();
        notified.sort_unstable();
        for (_, id, depth) in notified {
            // The flag is set here and the hook fires inside
            // `BuildOwner::build_scope`, right before the dependent's build:
            // `did_change_dependencies` runs once per change, before the build.
            owner.note_dependency_change(id);
            owner.schedule_build_for(id, depth, crate::RebuildReason::DependencyChange);
        }
    }

    /// The provider is unmounting: drop both halves of every edge.
    pub(crate) fn release(
        &mut self,
        provider: Option<ElementId>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        if let Some(provider) = provider {
            for dependent in self.entries.keys().copied() {
                owner.unregister_inherited_dependency(dependent, provider);
            }
        }
        self.entries.clear();
    }
}

/// What a run of `provide` changed, for the element that ran it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProvideOutcome {
    /// The fields of the values whose dependents must rebuild.
    changed: FieldSet,
    /// A type appeared or went: the subtree's scope is stale.
    set_changed: bool,
}

/// The values one element publishes, and the elements that read them.
///
/// A behavior holds it behind an `Option<Box<_>>`, so an element that
/// publishes nothing pays one null pointer.
#[derive(Default)]
pub(crate) struct ProvidedValues {
    slots: SmallVec<[Slot; 2]>,
    /// The bit each type owns in a dependent's field set. A type keeps its bit
    /// for the element's life, so a lifecycle read recorded before the type
    /// went and came back still names it.
    bits: SmallVec<[TypeId; 2]>,
    dependents: Dependents,
}

impl std::fmt::Debug for ProvidedValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProvidedValues")
            .field("values", &self.slots.len())
            .field("dependents", &self.dependents.len())
            .finish_non_exhaustive()
    }
}

impl ProvidedValues {
    /// The field a dependent of `type_id` records.
    ///
    /// The first 64 types an element ever publishes own a bit each; later
    /// ones share the whole set, so their dependents rebuild on any change.
    fn field_of(&mut self, type_id: TypeId) -> FieldSet {
        let index = self
            .bits
            .iter()
            .position(|known| *known == type_id)
            .unwrap_or_else(|| {
                self.bits.push(type_id);
                self.bits.len() - 1
            });
        u32::try_from(index)
            .ok()
            .and_then(|index| 1u64.checked_shl(index))
            .map_or(FieldSet::ALL, FieldSet::from_bits)
    }

    fn known_field_of(&self, type_id: TypeId) -> FieldSet {
        self.bits
            .iter()
            .position(|known| *known == type_id)
            .and_then(|index| u32::try_from(index).ok())
            .and_then(|index| 1u64.checked_shl(index))
            .map_or(FieldSet::ALL, FieldSet::from_bits)
    }

    /// Compare `staged` with the published values, then publish it.
    ///
    /// Every `update_should_notify` runs before anything is replaced: a panic
    /// in one leaves the published values as they were. Returns the values
    /// it replaced, for the caller to drop.
    fn commit(&mut self, staged: SmallVec<[Slot; 2]>) -> (ProvideOutcome, SmallVec<[Slot; 2]>) {
        for slot in &staged {
            // Mint the bit of a type seen for the first time.
            let _: FieldSet = self.field_of(slot.type_id);
        }
        let mut changed = FieldSet::NONE;
        let mut set_changed = staged.len() != self.slots.len();
        for slot in &staged {
            match self.slots.iter().find(|old| old.type_id == slot.type_id) {
                Some(old) => {
                    if slot.value.should_notify(old.value.as_any()) {
                        changed |= self.known_field_of(slot.type_id);
                    }
                }
                None => set_changed = true,
            }
        }
        if set_changed {
            // Readers of a type that went resolve another provider now.
            for old in &self.slots {
                if !staged.iter().any(|slot| slot.type_id == old.type_id) {
                    changed |= self.known_field_of(old.type_id);
                }
            }
        }
        // The replaced values go back to the caller: their `Drop` is user
        // code and runs after what this commit owes is on record.
        let replaced = std::mem::replace(&mut self.slots, staged);
        (
            ProvideOutcome {
                changed,
                set_changed,
            },
            replaced,
        )
    }
}

impl crate::element::InheritedElementAccess for ProvidedValues {
    fn provided_types(&self, visit: &mut dyn FnMut(TypeId)) {
        for slot in &self.slots {
            visit(slot.type_id);
        }
    }

    fn provided(&self, type_id: TypeId) -> Option<crate::element::ProvidedRef<'_>> {
        let slot = self.slots.iter().find(|slot| slot.type_id == type_id)?;
        Some(crate::element::ProvidedRef {
            value: slot.value.as_any(),
            field: Some(self.known_field_of(type_id)),
        })
    }

    fn dependent_count(&self) -> usize {
        self.dependents.len()
    }

    fn record_dependent(
        &mut self,
        _token: crate::context::CrateToken,
        dependent: ElementId,
        depth: usize,
        mask: FieldSet,
    ) {
        self.dependents.add(dependent, depth, mask);
    }

    fn record_lifecycle_dependent(
        &mut self,
        _token: crate::context::CrateToken,
        dependent: ElementId,
        depth: usize,
        mask: FieldSet,
    ) {
        self.dependents.add_lifecycle(dependent, depth, mask);
    }

    fn reset_dependent_mask(&mut self, _token: crate::context::CrateToken, dependent: ElementId) {
        self.dependents.reset_mask(dependent);
    }

    fn prune_unread_dependent(
        &mut self,
        _token: crate::context::CrateToken,
        dependent: ElementId,
    ) -> bool {
        self.dependents.prune_unread(dependent)
    }

    fn remove_dependent(&mut self, _token: crate::context::CrateToken, dependent: ElementId) {
        self.dependents.remove(dependent);
    }
}

/// What a behavior keeps for the values its element publishes.
///
/// An element that publishes nothing pays one null pointer and a flag.
#[derive(Debug, Default)]
pub(crate) struct Publication {
    values: Option<Box<ProvidedValues>>,
    /// The set of published types changed since the tree last read it: the
    /// node's inherited scope, and its subtree's, are stale.
    scope_stale: bool,
}

impl Publication {
    /// Run a `provide` hook and publish what it wrote.
    ///
    /// The hook writes into a staging list, so a panic in it, or in an
    /// `update_should_notify` it leads to, leaves the published values as they
    /// were. Dependents of a value that changed are scheduled after the
    /// commit.
    pub(crate) fn publish(
        &mut self,
        owner: &mut crate::ElementOwner<'_>,
        provide: impl FnOnce(&mut Provided<'_>),
    ) {
        let mut staged: SmallVec<[Slot; 2]> = SmallVec::new();
        provide(&mut Provided {
            staged: &mut staged,
        });
        let values = match &mut self.values {
            Some(values) => values,
            None if staged.is_empty() => return,
            None => self.values.insert(Box::default()),
        };
        let (outcome, replaced) = values.commit(staged);
        self.scope_stale |= outcome.set_changed;
        values.dependents.notify(outcome.changed, owner);
        // A value's `Drop` that panics unwinds from here with the new values
        // published, the scope marked and the dependents scheduled.
        drop(replaced);
    }

    /// Whether the published types changed since the last call.
    pub(crate) fn take_scope_stale(&mut self) -> bool {
        std::mem::take(&mut self.scope_stale)
    }

    pub(crate) fn access(&self) -> Option<&dyn crate::element::InheritedElementAccess> {
        self.values
            .as_deref()
            .map(|values| values as &dyn crate::element::InheritedElementAccess)
    }

    pub(crate) fn access_mut(&mut self) -> Option<&mut dyn crate::element::InheritedElementAccess> {
        self.values
            .as_deref_mut()
            .map(|values| values as &mut dyn crate::element::InheritedElementAccess)
    }

    /// The element is unmounting: drop both halves of every dependency edge.
    pub(crate) fn release(
        &mut self,
        provider: Option<ElementId>,
        owner: &mut crate::ElementOwner<'_>,
    ) {
        if let Some(values) = &mut self.values {
            values.dependents.release(provider, owner);
        }
    }
}
