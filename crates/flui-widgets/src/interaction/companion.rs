//! [`CompanionLink`] — the single-pointer path of a behavioural widget, drawn
//! by another widget (ADR-0216 §1).
//!
//! A behavioural widget whose commands are already one tap away in a widget
//! beside it (a tab bar next to the pager it turns, carousel dots, a stepper,
//! Previous and Next buttons placed where the layout wants them) needs no
//! controls of its own. It declares
//! [`Alternative::Companion`](crate::Alternative::Companion) with a
//! [`CompanionLink`] that whoever owns the state shared by both widgets made,
//! and the companion widget [registers](CompanionLink::register) on the link
//! while it is mounted, publishing for each command the semantics node that
//! performs it.
//!
//! The link makes the claim checkable. With no companion registered the
//! alternatives are unfulfilled: the behavioural widget warns on `flui::a11y`
//! once the frame it mounted in has run, and `assert_gesture_alternatives`
//! fails. A widget therefore cannot claim a single-pointer path that is not
//! there.
//!
//! # Identity and ownership
//!
//! A link is an `Rc`: clones are one link, and two links are equal only when
//! one is a clone of the other. A [`CompanionRegistration`] ends when it is
//! dropped, which is how a companion leaves on unmount. Nothing is borrowed
//! while a companion's resolver, or the closure of a registration being
//! dropped, runs, so a companion may register or leave while the owner builds,
//! and a resolver may touch the link.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use flui_rendering::semantics::SemanticsAnchor;
use smallvec::SmallVec;

use super::actions::Intent;

/// Names the semantics node that performs an intent right now.
type NodeResolver<I> = dyn Fn(&I) -> Option<SemanticsAnchor>;

/// Why a command has no node to activate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum CompanionGap {
    /// No companion is registered on the link: the alternatives are
    /// unfulfilled.
    #[error("no companion is registered on the link")]
    Unregistered,
    /// Companions are registered, but none has a node that performs the
    /// command now: a Next tab past the last one.
    #[error("no registered companion has a node that performs the command now")]
    Unavailable,
}

/// One registration's identity on its link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RegistrationId(u64);

struct Entry<I> {
    id: RegistrationId,
    nodes: Rc<NodeResolver<I>>,
}

struct Registry<I> {
    entries: RefCell<Vec<Entry<I>>>,
    /// The last id handed out; ids are never reused.
    last_id: Cell<u64>,
}

impl<I> Registry<I> {
    /// The registered resolvers in registration order, with the borrow
    /// released before any of them runs.
    fn resolvers(&self) -> SmallVec<[Rc<NodeResolver<I>>; 2]> {
        self.entries
            .borrow()
            .iter()
            .map(|entry| Rc::clone(&entry.nodes))
            .collect()
    }
}

/// What the registry offers a registration to leave through, without naming
/// the link's intent type.
trait Withdraw {
    fn withdraw(&self, id: RegistrationId);
}

impl<I> Withdraw for Registry<I> {
    fn withdraw(&self, id: RegistrationId) {
        let removed = {
            let mut entries = self.entries.borrow_mut();
            entries
                .iter()
                .position(|entry| entry.id == id)
                .map(|index| entries.remove(index))
        };
        // The resolver's captures may be the last owners of anything, and run
        // arbitrary `Drop` code: they go after the borrow.
        drop(removed);
    }
}

/// What a [`CompanionSpec`](crate::CompanionSpec) asks of its link without
/// naming the link's intent type.
pub(super) trait LinkedCompanions {
    /// Whether the link carries intents of the type `intent` has.
    fn accepts(&self, intent: &dyn Any) -> bool;
    /// The node performing `intent` now.
    fn node_for(&self, intent: &dyn Any) -> Result<SemanticsAnchor, CompanionGap>;
    /// Whether any companion is registered.
    fn is_fulfilled(&self) -> bool;
}

impl<I: Intent> LinkedCompanions for Registry<I> {
    fn accepts(&self, intent: &dyn Any) -> bool {
        intent.is::<I>()
    }

    fn node_for(&self, intent: &dyn Any) -> Result<SemanticsAnchor, CompanionGap> {
        let Some(intent) = intent.downcast_ref::<I>() else {
            return Err(CompanionGap::Unavailable);
        };
        let resolvers = self.resolvers();
        if resolvers.is_empty() {
            return Err(CompanionGap::Unregistered);
        }
        resolvers
            .iter()
            .find_map(|nodes| nodes(intent))
            .ok_or(CompanionGap::Unavailable)
    }

    fn is_fulfilled(&self) -> bool {
        !self.entries.borrow().is_empty()
    }
}

/// The meeting point of a behavioural widget and the widgets that draw its
/// single-pointer path, shared by the state both read.
///
/// `I` is the intent the behavioural widget's commands perform
/// ([`PageIntent`](crate::PageIntent) for a pager). The owner of the state
/// that both widgets read (a tab controller) makes the link, hands one clone to
/// the behavioural widget in
/// [`CompanionSpec::new`](crate::CompanionSpec::new) and another to the
/// companions.
///
/// ```
/// use flui_widgets::{CompanionLink, PageIntent, SemanticsAnchor};
///
/// let link = CompanionLink::<PageIntent>::new();
/// let next_page = SemanticsAnchor::new();
/// assert!(!link.is_fulfilled());
///
/// let named = next_page.clone();
/// let registration = link.register(move |intent| match intent {
///     PageIntent::Next => Some(named.clone()),
///     PageIntent::Previous => None,
/// });
/// assert!(link.is_fulfilled());
///
/// drop(registration);
/// assert!(!link.is_fulfilled());
/// ```
pub struct CompanionLink<I> {
    registry: Rc<Registry<I>>,
}

impl<I> Clone for CompanionLink<I> {
    fn clone(&self) -> Self {
        Self {
            registry: Rc::clone(&self.registry),
        }
    }
}

impl<I: Intent> Default for CompanionLink<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Intent> CompanionLink<I> {
    /// A link no companion is registered on.
    #[must_use]
    pub fn new() -> Self {
        Self {
            registry: Rc::new(Registry {
                entries: RefCell::new(Vec::new()),
                last_id: Cell::new(0),
            }),
        }
    }

    /// Register a companion until the returned registration is dropped.
    ///
    /// `nodes` names the semantics node that performs an intent right now, or
    /// `None` when the companion has none for it: a Next tab past the last
    /// one. It is read each time the check asks, never cached, so it may
    /// follow the state both widgets share. A node is named by the
    /// [`SemanticsAnchor`] its widget declares with
    /// [`Semantics::anchor`](crate::Semantics::anchor), made once in the
    /// companion's state. With several companions registered, the first to
    /// name a node performs the command.
    pub fn register(
        &self,
        nodes: impl Fn(&I) -> Option<SemanticsAnchor> + 'static,
    ) -> CompanionRegistration {
        let id = self
            .registry
            .last_id
            .get()
            .checked_add(1)
            .expect("BUG: companion registration ids are exhausted");
        self.registry.last_id.set(id);
        let id = RegistrationId(id);
        self.registry.entries.borrow_mut().push(Entry {
            id,
            nodes: Rc::new(nodes),
        });
        let registry: Weak<Registry<I>> = Rc::downgrade(&self.registry);
        CompanionRegistration { registry, id }
    }

    /// Whether any companion is registered: the alternatives are fulfilled.
    #[must_use]
    pub fn is_fulfilled(&self) -> bool {
        LinkedCompanions::is_fulfilled(&*self.registry)
    }

    pub(super) fn linked(&self) -> Rc<dyn LinkedCompanions> {
        Rc::clone(&self.registry) as Rc<dyn LinkedCompanions>
    }
}

impl<I> PartialEq for CompanionLink<I> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.registry, &other.registry)
    }
}

impl<I> Eq for CompanionLink<I> {}

impl<I> fmt::Debug for CompanionLink<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompanionLink")
            .field("registrations", &self.registry.entries.borrow().len())
            .finish()
    }
}

/// A companion's registration on a [`CompanionLink`]: the companion is
/// registered until this is dropped, which is how it leaves on unmount.
#[must_use = "a companion is registered only while its registration is held"]
pub struct CompanionRegistration {
    registry: Weak<dyn Withdraw>,
    id: RegistrationId,
}

impl Drop for CompanionRegistration {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.withdraw(self.id);
        }
    }
}

impl fmt::Debug for CompanionRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompanionRegistration")
            .field("id", &self.id.0)
            .finish_non_exhaustive()
    }
}
