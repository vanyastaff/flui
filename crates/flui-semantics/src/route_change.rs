//! Route changes (ADR-0204 §5): the innermost route of the published tree,
//! and the [`RouteChange`] a change of it raises.
//!
//! A route is a node that scopes one (`scopes_route`), or a modal dialog. The
//! innermost is the route last in reading order (pre-order) among the rooted,
//! addressable ones: a route inside another comes after it, and a page pushed
//! over another is its later sibling. Its name is the label of the first node
//! in its scope that names the route (`names_route`); a modal dialog that has
//! none is named by its own label, or else by the labels it is labelled by.

use rustc_hash::FxHashSet;

use crate::identity::AccessibilityNodeId;
use crate::platform::{RouteChange, RouteChangeKind};
use crate::relation::SemanticsRelation;
use crate::tree::SemanticsTree;
use crate::{SemanticsConfiguration, SemanticsNode, SemanticsRole};
use flui_foundation::SemanticsId;

/// The route nodes of the tree, maintained like the owner's focus claimants,
/// and the routes the last delivered update showed.
#[derive(Debug, Default)]
pub(crate) struct RouteTracker {
    /// Arena ids of the nodes that declare a route: rebuilt by a full
    /// publish, kept by an incremental one from the nodes it examines.
    claimants: FxHashSet<SemanticsId>,
    /// `None` until the first update is delivered, whose route raises nothing.
    published: Option<PublishedRoutes>,
}

/// The routes a delivered update showed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PublishedRoutes {
    innermost: Option<AccessibilityNodeId>,
    routes: FxHashSet<AccessibilityNodeId>,
}

/// What the flush in progress will show of the routes, and the change it
/// raises once delivered.
#[derive(Debug)]
pub(crate) struct PendingRoutes {
    pub(crate) next: PublishedRoutes,
    pub(crate) change: Option<RouteChange>,
}

impl RouteTracker {
    /// Rebuilds the route nodes from the whole tree.
    pub(crate) fn rescan(&mut self, tree: &SemanticsTree) {
        self.claimants = tree
            .iter()
            .filter(|(_, node)| is_route(node.config()))
            .map(|(sid, _)| sid)
            .collect();
    }

    /// Records whether `node` now declares a route.
    pub(crate) fn observe(&mut self, sid: SemanticsId, node: &SemanticsNode) {
        if is_route(node.config()) {
            self.claimants.insert(sid);
        } else {
            self.claimants.remove(&sid);
        }
    }

    /// The routes the tree shows now, and the change from the last delivered
    /// update; `None` while the tree has no root, which publishes nothing.
    pub(crate) fn pending(&mut self, tree: &SemanticsTree) -> Option<PendingRoutes> {
        let root = tree.root()?;
        self.claimants.retain(|&sid| tree.contains(sid));
        let mut innermost: Option<(Vec<usize>, SemanticsId, AccessibilityNodeId)> = None;
        let mut routes = FxHashSet::default();
        for &sid in &self.claimants {
            let Some(identity) = tree.get(sid).and_then(SemanticsNode::accessibility_id) else {
                continue;
            };
            let Some(path) = rooted_path(tree, root, sid) else {
                continue;
            };
            routes.insert(identity);
            if innermost
                .as_ref()
                .is_none_or(|(held, _, _)| path.as_slice() > held.as_slice())
            {
                innermost = Some((path, sid, identity));
            }
        }
        let next = PublishedRoutes {
            innermost: innermost.as_ref().map(|(_, _, identity)| *identity),
            routes,
        };
        let change = match (&self.published, &innermost) {
            (Some(published), Some((_, sid, identity)))
                if published.innermost != Some(*identity) =>
            {
                let kind = match published.innermost {
                    Some(previous) if next.routes.contains(&previous) => RouteChangeKind::Pushed,
                    _ if published.routes.contains(identity) => RouteChangeKind::Popped,
                    _ => RouteChangeKind::Replaced,
                };
                Some(RouteChange::new(
                    accesskit::NodeId(identity.as_u64()),
                    route_name(tree, *sid),
                    kind,
                ))
            }
            _ => None,
        };
        Some(PendingRoutes { next, change })
    }

    /// Records what a delivered update showed.
    pub(crate) fn commit(&mut self, published: PublishedRoutes) {
        self.published = Some(published);
    }
}

/// Whether `config` declares a route: it scopes one, or it is a modal dialog.
fn is_route(config: &SemanticsConfiguration) -> bool {
    config.scopes_route()
        || (config.is_modal()
            && matches!(
                config.role(),
                SemanticsRole::Dialog | SemanticsRole::AlertDialog
            ))
}

/// The child indices from `root` down to `sid`, or `None` when no chain of
/// parents reaches the root through addressable nodes only: an unaddressable
/// node is not published, and neither is anything under it.
fn rooted_path(tree: &SemanticsTree, root: SemanticsId, sid: SemanticsId) -> Option<Vec<usize>> {
    let mut path = Vec::new();
    let mut current = sid;
    // A cycle, an assembly defect, never reaches the root.
    for _ in 0..=tree.len() {
        tree.get(current)?.accessibility_id()?;
        if current == root {
            path.reverse();
            return Some(path);
        }
        let parent = tree.parent(current)?;
        let index = tree
            .children(parent)?
            .iter()
            .position(|&child| child == current)?;
        path.push(index);
        current = parent;
    }
    None
}

/// The name of the route `route` scopes, if anything names it.
fn route_name(tree: &SemanticsTree, route: SemanticsId) -> Option<String> {
    let label = |config: &SemanticsConfiguration| {
        config
            .label()
            .filter(|label| !label.is_empty())
            .map(|label| label.as_str().to_owned())
    };
    // The first naming node in reading order. Each node is visited once: a
    // shared child, an assembly defect, is not walked twice.
    let mut visited = FxHashSet::default();
    let mut stack = vec![route];
    while let Some(sid) = stack.pop() {
        if !visited.insert(sid) {
            continue;
        }
        // An unaddressable node is not published, nor anything under it.
        let Some(node) = tree
            .get(sid)
            .filter(|node| node.accessibility_id().is_some())
        else {
            continue;
        };
        if node.config().names_route()
            && let Some(name) = label(node.config())
        {
            return Some(name);
        }
        if let Some(children) = tree.children(sid) {
            stack.extend(children.iter().rev());
        }
    }
    let config = tree.get(route)?.config();
    if !config.is_modal() {
        return None;
    }
    label(config).or_else(|| {
        let anchors: Vec<_> = config
            .relations()
            .iter()
            .filter(|(relation, _)| *relation == SemanticsRelation::LabelledBy)
            .map(|(_, anchor)| anchor)
            .collect();
        if anchors.is_empty() {
            return None;
        }
        // Only a published node names the dialog, as the relation the adapter
        // receives resolves: an unattached one keeps its label to itself.
        let names: Vec<String> = tree
            .rooted_nodes()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|sid| tree.get(sid))
            .filter(|node| {
                node.config()
                    .anchors()
                    .iter()
                    .any(|anchor| anchors.contains(&anchor))
            })
            .filter_map(|node| label(node.config()))
            .collect();
        (!names.is_empty()).then(|| names.join(" "))
    })
}
