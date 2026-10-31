//! Route changes (ADR-0204 §5): a change of the innermost route raises one
//! event once the update showing it is delivered, through the adapter's sink
//! where it has a native notice, or as a polite announcement where it has
//! none.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::geometry::Rect;
use flui_foundation::{RenderId, SemanticsId};
use flui_semantics::platform::{RouteChange, RouteChangeKind};
use flui_semantics::{
    Announcements, SemanticsAnchor, SemanticsConfiguration, SemanticsIdentitySource, SemanticsNode,
    SemanticsOwner, SemanticsRelation, SemanticsRole, TreeUpdate, Urgency,
};
use parking_lot::Mutex;

/// An owner whose tree is a window (node 1), with what it delivered and the
/// route changes its sink was told.
struct Routes {
    owner: SemanticsOwner,
    ids: Vec<(usize, SemanticsId)>,
    updates: Arc<Mutex<Vec<TreeUpdate>>>,
    told: Arc<Mutex<Vec<RouteChange>>>,
    /// Makes the next deliveries panic, as a failing adapter does.
    failing: Arc<AtomicBool>,
    announcements: Announcements,
}

impl Routes {
    /// With a sink when `native`; announcing the routes when not.
    fn new(native: bool) -> Self {
        let updates: Arc<Mutex<Vec<TreeUpdate>>> = Arc::default();
        let failing = Arc::new(AtomicBool::new(false));
        let (sink, fail) = (Arc::clone(&updates), Arc::clone(&failing));
        let mut owner = SemanticsOwner::new(Arc::new(move |update: &TreeUpdate| {
            assert!(
                !fail.load(Ordering::SeqCst),
                "the adapter rejects the update"
            );
            sink.lock().push(update.clone());
        }));
        let told: Arc<Mutex<Vec<RouteChange>>> = Arc::default();
        if native {
            let told = Arc::clone(&told);
            owner.set_route_change_sink(Some(Arc::new(move |change| told.lock().push(change))));
        }
        let ids = SemanticsIdentitySource::default()
            .allocate(2)
            .expect("a fresh source has identities");
        let announcements = Announcements::new(ids[0], ids[1]);
        owner.set_announcements(announcements.clone());
        let mut routes = Self {
            owner,
            ids: Vec::new(),
            updates,
            told,
            failing,
            announcements,
        };
        routes.add(None, 1, |c| c.set_label("Window"));
        routes
    }

    /// Node `index` under `parent` (the root when `None`), configured by
    /// `configure`.
    fn add(
        &mut self,
        parent: Option<usize>,
        index: usize,
        configure: impl FnOnce(&mut SemanticsConfiguration),
    ) -> &mut Self {
        let mut node = SemanticsNode::new().with_source_render_id(RenderId::new(index));
        node.set_rect(Rect::from_ltwh(0.0, 0.0, 100.0, 100.0));
        configure(node.config_mut());
        let id = self.owner.insert(node);
        match parent {
            Some(parent) => self.owner.add_child(self.id_of(parent), id),
            None => self.owner.set_root(Some(id)),
        }
        self.ids.push((index, id));
        self
    }

    /// A page under the window, scoping a route that `name` names.
    fn page(&mut self, index: usize, name: Option<&str>) -> &mut Self {
        self.add(Some(1), index, |c| {
            c.set_scopes_route(true);
            if let Some(name) = name {
                c.set_names_route(true);
                c.set_label(name);
            }
        })
    }

    /// Takes node `index` and its subtree out of the tree.
    fn drop_node(&mut self, index: usize) -> &mut Self {
        let id = self.id_of(index);
        let parent = self.owner.tree().parent(id).expect("a child node");
        self.owner.remove_child(parent, id);
        self.owner.remove(id);
        self.ids.retain(|(candidate, _)| *candidate != index);
        self
    }

    fn id_of(&self, index: usize) -> SemanticsId {
        self.ids
            .iter()
            .find(|(candidate, _)| *candidate == index)
            .map(|(_, id)| *id)
            .expect("the fixture has that node")
    }

    /// The route changes the sink was told since the last call, as the
    /// render index of their route, name and kind.
    fn told(&self) -> Vec<(u64, Option<String>, RouteChangeKind)> {
        self.told
            .lock()
            .drain(..)
            .map(|change| {
                (
                    change.route().0,
                    change.name().map(str::to_owned),
                    change.kind(),
                )
            })
            .collect()
    }

    /// The published identity of node `index`.
    fn published_id(&self, index: usize) -> u64 {
        self.owner
            .tree()
            .get(self.id_of(index))
            .and_then(SemanticsNode::accessibility_id)
            .expect("an addressable node")
            .as_u64()
    }

    /// The polite announcer's value in the last delivered update, if it
    /// carried the announcer.
    fn announced(&self) -> Option<String> {
        let polite = accesskit::NodeId(self.announcements.node_id(Urgency::Polite).as_u64());
        self.updates
            .lock()
            .last()?
            .nodes
            .iter()
            .find(|(id, _)| *id == polite)
            .and_then(|(_, node)| node.value().map(str::to_owned))
    }
}

/// Each change of the innermost route is told once, with its name and kind:
/// not the first route, nor a republish of the same one; a push, two pushes
/// in one flush (the innermost only), a pop back, a replacement, a route
/// nothing names, and a modal dialog named by its title.
#[test]
fn each_change_of_the_innermost_route_is_told_once() {
    let mut routes = Routes::new(true);
    routes.page(2, Some("Home"));
    routes.owner.flush();
    assert_eq!(routes.told(), [], "the first route raises nothing");

    routes.page(3, Some("Settings"));
    routes.owner.flush();
    let settings = routes.published_id(3);
    assert_eq!(
        routes.told(),
        [(settings, Some("Settings".into()), RouteChangeKind::Pushed)]
    );

    routes.page(4, Some("Account")).page(5, Some("Password"));
    routes.owner.flush();
    let password = routes.published_id(5);
    assert_eq!(
        routes.told(),
        [(password, Some("Password".into()), RouteChangeKind::Pushed)],
        "two pushes in one flush tell the innermost once"
    );

    routes.drop_node(5).drop_node(4);
    routes.owner.flush();
    assert_eq!(
        routes.told(),
        [(settings, Some("Settings".into()), RouteChangeKind::Popped)]
    );

    routes.drop_node(3).page(6, Some("Profile"));
    routes.owner.flush();
    let profile = routes.published_id(6);
    assert_eq!(
        routes.told(),
        [(profile, Some("Profile".into()), RouteChangeKind::Replaced)]
    );

    routes.page(7, None);
    routes.owner.flush();
    let unnamed = routes.published_id(7);
    assert_eq!(routes.told(), [(unnamed, None, RouteChangeKind::Pushed)]);

    let title = SemanticsAnchor::new();
    let anchor = title.clone();
    routes
        .add(Some(1), 8, |c| {
            c.set_role(SemanticsRole::AlertDialog);
            c.set_modal(true);
            c.add_relation(SemanticsRelation::LabelledBy, title);
        })
        .add(Some(8), 9, |c| {
            c.set_label("Delete the account?");
            c.add_anchor(anchor);
        });
    routes.owner.flush();
    let dialog = routes.published_id(8);
    assert_eq!(
        routes.told(),
        [(
            dialog,
            Some("Delete the account?".into()),
            RouteChangeKind::Pushed
        )],
        "a modal dialog is a route, named by its title"
    );

    routes.owner.schedule_full_publish();
    routes.owner.flush();
    assert_eq!(routes.told(), [], "a republish shows the same route");
}

/// Without a sink, a named route is announced politely in the update that
/// shows it, and a route nothing names says nothing.
#[test]
fn without_a_sink_a_named_route_is_announced() {
    let mut routes = Routes::new(false);
    routes.page(2, Some("Home"));
    routes.owner.flush();
    assert_eq!(
        routes.announced().as_deref(),
        Some(""),
        "the first route is not announced"
    );

    routes.page(3, Some("Settings"));
    routes.owner.flush();
    assert_eq!(routes.announced().as_deref(), Some("Settings"));

    let before = routes.updates.lock().len();
    routes.page(4, None);
    routes.owner.flush();
    let updates = routes.updates.lock();
    assert!(
        updates[before..].iter().all(|update| update
            .nodes
            .iter()
            .all(|(_, node)| node.value() != Some("Settings\u{200B}"))),
        "an unnamed route repeats no announcement"
    );
}

/// A sink that panics is contained, nothing is announced in its place, and
/// the next change still reaches it; a delivery that fails raises its change
/// on the retry, once.
#[test]
fn a_failed_sink_or_delivery_loses_no_change_and_speaks_once() {
    let mut routes = Routes::new(false);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&calls);
    routes
        .owner
        .set_route_change_sink(Some(Arc::new(move |change: RouteChange| {
            log.lock().push(change.name().map(str::to_owned));
            assert!(log.lock().len() > 1, "the adapter's notice fails");
        })));
    routes.page(2, Some("Home"));
    routes.owner.flush();

    routes.page(3, Some("Settings"));
    routes.owner.flush();
    assert_eq!(*calls.lock(), [Some("Settings".to_owned())]);
    assert_ne!(
        routes.announced(),
        Some("Settings".to_owned()),
        "a panicking sink is not replaced by an announcement"
    );

    routes.failing.store(true, Ordering::SeqCst);
    routes.page(4, Some("Account"));
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| routes.owner.flush()));
    assert!(failed.is_err(), "the delivery failed");
    assert_eq!(calls.lock().len(), 1, "an undelivered route is not told");

    routes.failing.store(false, Ordering::SeqCst);
    routes.owner.flush();
    assert_eq!(
        *calls.lock(),
        [Some("Settings".to_owned()), Some("Account".to_owned())]
    );
}
