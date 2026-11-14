//! A presentation's one URL-owner slot, which its outermost `Router` registers
//! into (ADR-0330 §5).

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use super::{NavigationIntent, NavigationOp, NavigationRefused, RoutePath, UrlOwner};

/// A presentation's URL-owner slot. The runtime makes one per presentation
/// and installs it among the presentation's capabilities, which a lifecycle
/// hook reads with `LifecycleContext::url_owner`; the
/// outermost `Router` registers itself into it, and the runtime's
/// `navigate` reaches that Router through it.
///
/// The slot holds its owners weakly, so it keeps no Router alive. When two
/// Routers register (siblings, neither nested in the other), the first that
/// is still mounted owns the URL, and the next takes over when it leaves.
///
/// A URL the platform opens before the presentation's first frame (a cold
/// start) finds no Router yet: the slot keeps its location as the initial
/// one, which the Router that registers first opens with instead of its own,
/// as Flutter's `defaultRouteName` gives the first route. The first frame
/// that builds a tree ends the cold start, and an initial location no Router
/// took then is dropped, so a Router mounted later never opens on an old link.
#[derive(Clone, Default)]
pub struct UrlOwnerSlot(Rc<SlotInner>);

/// What the slot warns when a URL from the cold start is dropped untaken.
const DROPPED_COLD_URL: &str =
    "a URL opened before the first frame found no Router to open it, and is dropped";

#[derive(Default)]
struct SlotInner {
    /// Registrations, oldest first.
    owners: RefCell<Vec<Weak<dyn UrlOwner>>>,
    /// The location a URL opened during the cold start, for the first Router
    /// that registers.
    initial: RefCell<Option<RoutePath>>,
    /// Whether the presentation's first frame with a tree has run, ending the
    /// cold start.
    warm: Cell<bool>,
}

impl Drop for SlotInner {
    /// A presentation that closes during its cold start drops the URL it
    /// held with the warning the end of the cold start gives.
    fn drop(&mut self) {
        if let Some(path) = self.initial.get_mut().take() {
            tracing::warn!(
                %path,
                DROPPED_COLD_URL
            );
        }
    }
}

impl UrlOwnerSlot {
    /// An empty slot, in its cold start.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open `path`, the location of a URL the platform opened: its owner
    /// applies it as a [`NavigationOp::Go`], and during the cold start, with
    /// no owner registered yet, it becomes the initial location of the first
    /// Router that registers. A second URL in the cold start replaces the
    /// first, as a later link does in a running application.
    ///
    /// # Errors
    ///
    /// [`NavigationRefused::NoUrlOwner`] after the cold start when no owner
    /// is registered, and whatever the owner refuses with.
    pub fn open_at(&self, path: RoutePath) -> Result<(), NavigationRefused> {
        let owner = self.0.owners.borrow().iter().find_map(Weak::upgrade);
        if let Some(owner) = owner {
            return owner.navigate(NavigationIntent::new(NavigationOp::Go(path)));
        }
        if self.0.warm.get() {
            return Err(NavigationRefused::NoUrlOwner);
        }
        // A replaced `RoutePath` runs no user code as it drops.
        let replaced = self.0.initial.replace(Some(path));
        drop(replaced);
        Ok(())
    }

    /// The initial location a URL opened during the cold start, taken by the
    /// Router about to register as the first owner; `None` when an owner is
    /// registered already, so a sibling Router does not open on it.
    #[must_use]
    pub fn take_initial(&self) -> Option<RoutePath> {
        let owned = self
            .0
            .owners
            .borrow()
            .iter()
            .any(|registered| registered.strong_count() > 0);
        if owned {
            return None;
        }
        self.0.initial.take()
    }

    /// Whether the cold start is over.
    #[must_use]
    pub fn is_warm(&self) -> bool {
        self.0.warm.get()
    }

    /// End the cold start: the runtime calls this after the presentation's
    /// first frame that built a tree. An initial location no Router took is
    /// dropped with a warning.
    pub fn end_cold_start(&self) {
        if self.0.warm.replace(true) {
            return;
        }
        if let Some(path) = self.0.initial.take() {
            tracing::warn!(
                %path,
                DROPPED_COLD_URL
            );
        }
    }

    /// Register `owner`. It owns the URL while it is the oldest registration
    /// still alive, and leaves the slot when the returned registration drops.
    #[must_use = "the owner leaves the slot when the registration drops"]
    pub fn register(&self, owner: &Rc<dyn UrlOwner>) -> UrlOwnerRegistration {
        let others = {
            let mut owners = self.0.owners.borrow_mut();
            owners.retain(|registered| registered.strong_count() > 0);
            let others = owners.len();
            owners.push(Rc::downgrade(owner));
            others
        };
        if others > 0 {
            tracing::warn!(
                "two Routers that are not nested claim one presentation's URL; the first \
                 mounted owns it, and the next takes over when it leaves"
            );
        }
        UrlOwnerRegistration {
            slot: Rc::downgrade(&self.0),
            owner: Rc::downgrade(owner),
        }
    }

    /// Hand `intent` to the URL owner. Its owner runs with the slot released,
    /// so it may register, leave or navigate again.
    ///
    /// # Errors
    ///
    /// [`NavigationRefused::NoUrlOwner`] when no owner is registered, and
    /// whatever the owner refuses with.
    pub fn navigate(&self, intent: NavigationIntent) -> Result<(), NavigationRefused> {
        let owner = self.0.owners.borrow().iter().find_map(Weak::upgrade);
        let Some(owner) = owner else {
            return Err(NavigationRefused::NoUrlOwner);
        };
        owner.navigate(intent)
    }

    /// Whether `self` and `other` are one slot.
    #[must_use]
    pub fn is_same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for UrlOwnerSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let owners = self
            .0
            .owners
            .try_borrow()
            .map(|owners| owners.iter().filter(|o| o.strong_count() > 0).count());
        f.debug_struct("UrlOwnerSlot")
            .field("owners", &owners.ok())
            .field("warm", &self.0.warm.get())
            .finish()
    }
}

/// An owner's place in a [`UrlOwnerSlot`]; dropping it takes the owner out.
/// It removes only its own registration, so a Router that leaves after
/// another took the slot clears nothing of the other's.
pub struct UrlOwnerRegistration {
    slot: Weak<SlotInner>,
    owner: Weak<dyn UrlOwner>,
}

impl UrlOwnerRegistration {
    /// Whether this registration is in `slot`.
    #[must_use]
    pub fn is_in(&self, slot: &UrlOwnerSlot) -> bool {
        std::ptr::eq(self.slot.as_ptr(), Rc::as_ptr(&slot.0))
    }
}

impl Drop for UrlOwnerRegistration {
    fn drop(&mut self) {
        let Some(slot) = self.slot.upgrade() else {
            return;
        };
        // Dropping a `Weak` runs no destructor of the owner, so the slot's
        // borrow covers no user code.
        let removed = {
            let mut owners = slot.owners.borrow_mut();
            let at = owners
                .iter()
                .position(|registered| Weak::ptr_eq(registered, &self.owner));
            at.map(|at| owners.remove(at))
        };
        drop(removed);
    }
}

impl fmt::Debug for UrlOwnerRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UrlOwnerRegistration")
            .field("owner_alive", &(self.owner.strong_count() > 0))
            .finish_non_exhaustive()
    }
}
