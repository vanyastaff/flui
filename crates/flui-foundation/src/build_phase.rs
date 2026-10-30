//! Whether a `build` is running on this UI runtime (ADR-0086 §7).
//!
//! The signal graph that knows when an element builds lives in `flui-view`,
//! above the crates that define controllers (`flui-animation`, the scroll and
//! tab controllers of the widget packages). A controller refuses a mutation
//! made from `build` the way a signal write is refused, so it needs to see the
//! build without depending on the graph: the graph publishes its builds into a
//! [`BuildPhase`], and a widget that subscribes to a controller binds the
//! controller to that phase from `init_state`.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

/// An owner-local view of "a `build` is running".
///
/// Clones share one counter. The graph enters the phase for each element
/// build ([`enter`](Self::enter)) and leaves it when the returned guard
/// drops, so a build that unwinds leaves the phase too. Everything else only
/// reads it with [`is_building`](Self::is_building).
///
/// `!Send`: the phase belongs to the thread that builds.
///
/// ```
/// use flui_foundation::BuildPhase;
///
/// let phase = BuildPhase::new();
/// assert!(!phase.is_building());
/// let build = phase.enter();
/// assert!(phase.clone().is_building());
/// drop(build);
/// assert!(!phase.is_building());
/// ```
#[derive(Clone, Default)]
pub struct BuildPhase {
    depth: Rc<Cell<u32>>,
}

impl BuildPhase {
    /// A phase with no build running.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a build is running.
    #[must_use]
    pub fn is_building(&self) -> bool {
        self.depth.get() > 0
    }

    /// Enter the phase until the returned guard drops.
    ///
    /// # Panics
    ///
    /// When `u32::MAX` builds are entered at once.
    #[must_use = "the phase is left when the guard drops"]
    pub fn enter(&self) -> BuildPhaseGuard {
        let depth = self
            .depth
            .get()
            .checked_add(1)
            .expect("BUG: build nesting is bounded by the call stack");
        self.depth.set(depth);
        BuildPhaseGuard {
            depth: Rc::clone(&self.depth),
        }
    }

    /// Whether `self` and `other` are the same phase.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.depth, &other.depth)
    }
}

impl fmt::Debug for BuildPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildPhase")
            .field("building", &self.is_building())
            .finish()
    }
}

/// One entered build of a [`BuildPhase`]; leaves the phase on drop.
#[derive(Debug)]
pub struct BuildPhaseGuard {
    depth: Rc<Cell<u32>>,
}

impl Drop for BuildPhaseGuard {
    fn drop(&mut self) {
        self.depth.set(self.depth.get().saturating_sub(1));
    }
}

/// A controller mutator was called while its widget's `build` was running,
/// and refused: the controller's state did not change and its listeners were
/// not notified.
///
/// `build` describes the tree; it does not drive controllers. Mutate from an
/// event callback, `init_state`, `did_update_view` or a post-frame callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{controller} was mutated during build")]
pub struct MutatedDuringBuild {
    /// The controller type that refused, such as `"ScrollController"`.
    pub controller: &'static str,
}

impl MutatedDuringBuild {
    /// Refuse a mutation of `controller` when `building`: an error, also
    /// reported on the `flui::signals` target.
    ///
    /// # Errors
    ///
    /// [`MutatedDuringBuild`] when `building`.
    pub fn check(building: bool, controller: &'static str) -> Result<(), Self> {
        if !building {
            return Ok(());
        }
        let error = Self { controller };
        tracing::warn!(target: "flui::signals", controller, %error, "a controller mutation was refused");
        Err(error)
    }
}
