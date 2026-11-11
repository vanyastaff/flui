//! Watches on a leader's laid-out geometry, delivered after the frame that
//! changed it.
//!
//! A follower's *position* tracks its leader at composite time. A follower
//! whose *build* depends on its leader (a selection handle sized to its end's
//! line, a menu that opens above or below its anchor) needs to hear when that
//! geometry changes (its size, or where it stands after a scroll), including
//! after a relayout no build caused. A
//! [`LeaderWatch`] is that signal: the owner compares each live watch against
//! the layer tree each painted frame commits and queues only the watches whose
//! value changed into the frame's post-frame lane. A steady layout queues
//! nothing and requests no frame.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use flui_foundation::ListenerCallback;
use flui_foundation::geometry::{Point, Size};
use flui_foundation::notifier::{ChangeNotifier, Listenable, ListenerObserver};
use flui_layer::{LayerLink, SceneVersion};
use flui_scheduler::PostFrameHandle;

/// A leader's geometry in a committed frame.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LeaderGeometry {
    /// The leader's laid-out size: the extent its followers' anchors align
    /// within.
    pub size: Size<f64>,
    /// The leader's top-left corner in the coordinates of the layer tree's
    /// root, which a scroll or a transform above the leader moves while its
    /// size stays; `None` when a transform on its chain is singular or not
    /// finite.
    pub origin: Option<Point<f64>>,
}

/// A watch on the leader that publishes under one [`LayerLink`] in one
/// pipeline's frames ([`PipelineCell::watch_leader`]).
///
/// Its listeners run in the post-frame lane of the frame whose paint changed
/// [`Self::geometry`], outside every render-tree borrow. A listener that
/// rebuilds from the new value is seen one frame later: the change is known
/// only once paint has committed, and the rebuild it asks for runs in the
/// next frame, whose paint reports the same value and so queues nothing.
///
/// Clones share one watch; it ends when the last clone drops.
///
/// [`PipelineCell::watch_leader`]: super::PipelineCell::watch_leader
#[derive(Clone)]
pub struct LeaderWatch(Rc<WatchEntry>);

struct WatchEntry {
    link: LayerLink,
    geometry: Cell<Option<LeaderGeometry>>,
    /// Whether a delivery waits in the post-frame lane. Cleared before the
    /// listeners run, so a change during them queues another delivery.
    queued: Cell<bool>,
    lane: PostFrameHandle,
    notifier: ChangeNotifier,
}

impl LeaderWatch {
    /// The link this watch follows.
    #[must_use]
    pub fn link(&self) -> LayerLink {
        self.0.link
    }

    /// The leader's geometry in the last committed frame that painted; `None`
    /// while no leader publishes under the link.
    #[must_use]
    pub fn geometry(&self) -> Option<LeaderGeometry> {
        self.0.geometry.get()
    }
}

impl fmt::Debug for LeaderWatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LeaderWatch")
            .field("link", &self.0.link)
            .field("geometry", &self.0.geometry.get())
            .finish_non_exhaustive()
    }
}

impl Listenable for LeaderWatch {
    fn listen(&self, listener: ListenerCallback) -> flui_foundation::Subscription {
        self.0.notifier.listen(listener)
    }
    fn listen_with_recovery(&self, relay: ListenerObserver) -> flui_foundation::Subscription {
        self.0.notifier.listen_with_recovery(relay)
    }
}

/// The live watches of one pipeline, shared by its cell (which registers
/// them) and its owner (which checks them against each applied scene), with
/// the last applied version, so a watch made later starts from its leaders.
#[derive(Default)]
pub(crate) struct LeaderWatches {
    entries: RefCell<Vec<Weak<WatchEntry>>>,
    committed: RefCell<Option<SceneVersion>>,
}

impl LeaderWatches {
    pub(crate) fn watch(&self, link: LayerLink, lane: PostFrameHandle) -> LeaderWatch {
        let entry = Rc::new(WatchEntry {
            link,
            geometry: Cell::new(
                self.committed
                    .borrow()
                    .as_ref()
                    .and_then(|version| leader_geometry(version, link)),
            ),
            queued: Cell::new(false),
            lane,
            notifier: ChangeNotifier::new(),
        });
        let mut entries = self.entries.borrow_mut();
        entries.retain(|entry| entry.strong_count() > 0);
        entries.push(Rc::downgrade(&entry));
        LeaderWatch(entry)
    }

    /// Reads the leaders of `version`, the scene a frame's paint was just
    /// applied as, and queues a delivery for each live watch whose geometry
    /// changed. Runs no user code: listeners run in the post-frame lane.
    pub(crate) fn check(&self, version: &SceneVersion) {
        let live: Vec<Rc<WatchEntry>> = {
            let mut entries = self.entries.borrow_mut();
            entries.retain(|entry| entry.strong_count() > 0);
            entries.iter().filter_map(Weak::upgrade).collect()
        };
        for entry in live {
            let geometry = leader_geometry(version, entry.link);
            if entry.geometry.get() == geometry {
                continue;
            }
            entry.geometry.set(geometry);
            if entry.queued.replace(true) {
                continue;
            }
            let delivered = Rc::downgrade(&entry);
            let scheduled = entry.lane.schedule(move |_| {
                if let Some(entry) = delivered.upgrade() {
                    entry.queued.set(false);
                    entry.notifier.notify_listeners();
                }
            });
            if let Err(error) = scheduled {
                entry.queued.set(false);
                tracing::warn!(
                    ?error,
                    link = entry.link.id(),
                    "LeaderWatch: the post-frame lane is closed; the change is not delivered",
                );
            }
        }
        // The version replaced drops once the cell is free: an annotation's
        // `Drop` is user code.
        let replaced = self.committed.replace(Some(version.clone()));
        drop(replaced);
    }
}

/// The geometry `version` gives the leader publishing `link`, `None` without
/// an attached one.
fn leader_geometry(version: &SceneVersion, link: LayerLink) -> Option<LeaderGeometry> {
    version.leader_size(link).map(|size| LeaderGeometry {
        size,
        origin: version.leader_origin(link),
    })
}
