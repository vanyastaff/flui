//! Linear policy groups and spatial navigation over the existing focus tree.

use super::focus::FocusClosePanic;
use super::{
    FocusManager, FocusNode, FocusNodeId, FocusScopeNode, FocusTraversalPolicy, ResolvedStep,
    TraversalDirection, TraversalEdgeBehavior,
};
use flui_foundation::geometry::Rect;
use flui_painting::typography::TextDirection;
use std::{
    cmp::Ordering,
    rc::{Rc, Weak},
};

/// A spatial direction, independent of reading-order traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    /// Toward smaller vertical coordinates.
    Up,
    /// Toward larger vertical coordinates.
    Down,
    /// Toward smaller horizontal coordinates.
    Left,
    /// Toward larger horizontal coordinates.
    Right,
}

/// Weak explicit links for linear traversal; invalid links fall back to policy order.
#[derive(Debug, Clone, Default)]
pub struct FocusTraversalOverrides {
    next: Weak<FocusNode>,
    previous: Weak<FocusNode>,
}

impl FocusTraversalOverrides {
    /// Set the next target without keeping it alive.
    #[must_use]
    pub fn with_next(mut self, target: &Rc<FocusNode>) -> Self {
        self.next = Rc::downgrade(target);
        self
    }
    /// Set the previous target without keeping it alive.
    #[must_use]
    pub fn with_previous(mut self, target: &Rc<FocusNode>) -> Self {
        self.previous = Rc::downgrade(target);
        self
    }
    pub(super) fn target(&self, direction: TraversalDirection) -> Option<Rc<FocusNode>> {
        match direction {
            TraversalDirection::Forward => self.next.upgrade(),
            TraversalDirection::Backward => self.previous.upgrade(),
        }
    }
}

#[derive(Clone)]
pub(super) struct GroupConfig {
    pub policy: Rc<dyn FocusTraversalPolicy>,
    pub direction: TextDirection,
    pub edge: TraversalEdgeBehavior,
}

pub(super) type GroupOrderCache = Vec<(FocusNodeId, Vec<FocusNodeId>)>;

struct GroupSnapshot {
    id: FocusNodeId,
    node: Rc<FocusNode>,
    config: GroupConfig,
}

/// Membership is frozen before any policy callback can mutate the tree.
pub(super) struct GroupOrderSnapshot {
    groups: Vec<GroupSnapshot>,
    paths: Vec<(FocusNodeId, Vec<FocusNodeId>)>,
}

impl GroupOrderSnapshot {
    pub(super) fn new(nodes: &[Rc<FocusNode>], boundary: &Rc<FocusNode>) -> Self {
        let groups = boundary
            .descendants()
            .filter_map(|node| {
                node.traversal_group_snapshot().map(|config| GroupSnapshot {
                    id: node.id(),
                    node,
                    config,
                })
            })
            .collect();
        let paths = nodes
            .iter()
            .map(|node| {
                let mut path: Vec<_> = node
                    .ancestors()
                    .take_while(|parent| !Rc::ptr_eq(parent, boundary))
                    .filter(|parent| parent.traversal_group_snapshot().is_some())
                    .map(|parent| parent.id())
                    .collect();
                path.reverse();
                (node.id(), path)
            })
            .collect();
        Self { groups, paths }
    }

    pub(super) fn order(
        &self,
        nodes: &mut Vec<Rc<FocusNode>>,
        cache: &mut GroupOrderCache,
        failure: &mut FocusClosePanic,
    ) {
        self.order_at(nodes, 0, cache, failure);
    }

    fn order_at(
        &self,
        nodes: &mut Vec<Rc<FocusNode>>,
        depth: usize,
        cache: &mut GroupOrderCache,
        failure: &mut FocusClosePanic,
    ) {
        let mut output = Vec::with_capacity(nodes.len());
        let mut visited = Vec::new();
        for node in nodes.iter() {
            let group = self
                .paths
                .iter()
                .find(|(id, _)| *id == node.id())
                .and_then(|(_, path)| path.get(depth))
                .copied();
            let Some(id) = group else {
                output.push(Rc::clone(node));
                continue;
            };
            if visited.contains(&id) {
                continue;
            }
            visited.push(id);
            let mut members: Vec<_> = nodes
                .iter()
                .filter(|candidate| {
                    self.paths.iter().any(|(node_id, path)| {
                        *node_id == candidate.id() && path.get(depth) == Some(&id)
                    })
                })
                .cloned()
                .collect();
            if let Some((_, order)) = cache.iter().find(|(cached, _)| *cached == id) {
                members.sort_by_key(|node| {
                    order
                        .iter()
                        .position(|id| *id == node.id())
                        .unwrap_or(usize::MAX)
                });
            } else {
                if let Some(group) = self.groups.iter().find(|group| group.id == id) {
                    failure.run(|| {
                        group
                            .config
                            .policy
                            .order(&mut members, group.config.direction);
                    });
                }
                self.order_at(&mut members, depth + 1, cache, failure);
                cache.push((id, members.iter().map(|node| node.id()).collect()));
            }
            output.append(&mut members);
        }
        *nodes = output;
    }

    pub(super) fn retire(self, failure: &mut FocusClosePanic) {
        for group in self.groups {
            failure.retire(group.config.policy);
            failure.retire(group.node);
        }
    }
}

fn group_of(node: &Rc<FocusNode>) -> Option<Rc<FocusNode>> {
    node.ancestors()
        .take_while(|parent| !parent.is_scope())
        .find(|parent| parent.traversal_group_snapshot().is_some())
}

fn same_boundary(a: &Rc<FocusNode>, b: &Rc<FocusNode>) -> bool {
    match (group_of(a), group_of(b)) {
        (Some(a), Some(b)) => Rc::ptr_eq(&a, &b),
        (None, None) => match (a.enclosing_scope(), b.enclosing_scope()) {
            (Some(a), Some(b)) => Rc::ptr_eq(&a, &b),
            _ => false,
        },
        _ => false,
    }
}

fn eligible(manager: &FocusManager, node: &Rc<FocusNode>) -> bool {
    node.is_attached()
        && !node.is_scope()
        && node.can_request_focus()
        && !node.skip_traversal()
        && node
            .manager()
            .is_some_and(|owner| std::ptr::eq(owner.as_ref(), manager))
}

fn belongs_to(node: &Rc<FocusNode>, boundary: &Rc<FocusNode>) -> bool {
    node.ancestors().any(|parent| Rc::ptr_eq(&parent, boundary))
}

fn group_order(
    group: &Rc<FocusNode>,
    current: &Rc<FocusNode>,
    config: GroupConfig,
    cache: &mut GroupOrderCache,
    failure: &mut FocusClosePanic,
) -> Vec<Rc<FocusNode>> {
    let mut nodes: Vec<_> = group
        .descendants()
        .filter(|node| !node.is_scope() && node.can_request_focus() && !node.skip_traversal())
        .collect();
    if !nodes.iter().any(|node| Rc::ptr_eq(node, current)) && belongs_to(current, group) {
        nodes.push(Rc::clone(current));
    }
    let snapshot = GroupOrderSnapshot::new(&nodes, group);
    if let Some((_, order)) = cache.iter().find(|(id, _)| *id == group.id()) {
        nodes.sort_by_key(|node| {
            order
                .iter()
                .position(|id| *id == node.id())
                .unwrap_or(usize::MAX)
        });
    } else {
        failure.run(|| config.policy.order(&mut nodes, config.direction));
        snapshot.order(&mut nodes, cache, failure);
        cache.push((group.id(), nodes.iter().map(|node| node.id()).collect()));
    }
    snapshot.retire(failure);
    failure.retire(config.policy);
    nodes
}

enum Boundary {
    Group(Rc<FocusNode>, GroupConfig),
    Scope(Rc<FocusScopeNode>),
}

pub(super) fn linear_step(
    manager: &FocusManager,
    current: Option<&Rc<FocusNode>>,
    scope: &Rc<FocusScopeNode>,
    direction: TraversalDirection,
) -> ResolvedStep {
    let Some(current) = current else {
        return scope.step(None, direction);
    };
    if let Some(target) = current.traversal_override_target(direction)
        && !Rc::ptr_eq(current, &target)
        && eligible(manager, &target)
        && same_boundary(current, &target)
    {
        return ResolvedStep::Focus(target);
    }
    let boundaries: Vec<_> = current
        .ancestors()
        .filter_map(|node| {
            if let Some(scope) = node.as_scope() {
                Some(Boundary::Scope(scope))
            } else {
                node.traversal_group_snapshot()
                    .map(|config| Boundary::Group(node, config))
            }
        })
        .collect();
    let mut failure = FocusClosePanic::for_rejection(current.traversal_close_mode());
    let mut cache = Vec::new();
    let mut step = ResolvedStep::None;
    let mut selected_boundary = None;
    for boundary in &boundaries {
        if failure.preserving() {
            break;
        }
        step = match boundary {
            Boundary::Scope(scope) => failure
                .invoke(|| scope.resolve_traversal_with_cache(Some(current), direction, &mut cache))
                .unwrap_or_default(),
            Boundary::Group(group, config) => {
                let nodes = group_order(group, current, config.clone(), &mut cache, &mut failure);
                let index = nodes.iter().position(|node| Rc::ptr_eq(node, current));
                let target = match direction {
                    TraversalDirection::Forward => index.and_then(|index| nodes.get(index + 1)),
                    TraversalDirection::Backward => index
                        .and_then(|index| index.checked_sub(1).and_then(|index| nodes.get(index))),
                };
                let result = if let Some(target) = target {
                    ResolvedStep::Focus(Rc::clone(target))
                } else {
                    match config.edge {
                        TraversalEdgeBehavior::ClosedLoop => match direction {
                            TraversalDirection::Forward => {
                                nodes.iter().find(|node| eligible(manager, node))
                            }
                            TraversalDirection::Backward => {
                                nodes.iter().rev().find(|node| eligible(manager, node))
                            }
                        }
                        .map_or(ResolvedStep::None, |node| {
                            ResolvedStep::Focus(Rc::clone(node))
                        }),
                        TraversalEdgeBehavior::Stop => ResolvedStep::None,
                        TraversalEdgeBehavior::LeaveView => ResolvedStep::Unfocus,
                        TraversalEdgeBehavior::ParentScope => ResolvedStep::RetryInParent,
                    }
                };
                for node in nodes {
                    failure.retire(node);
                }
                result
            }
        };
        if !matches!(step, ResolvedStep::RetryInParent) {
            selected_boundary = Some(match boundary {
                Boundary::Group(node, _) => node.id(),
                Boundary::Scope(scope) => scope.as_focus_node().id(),
            });
            break;
        }
    }
    for boundary in boundaries {
        match boundary {
            Boundary::Group(node, config) => {
                failure.retire(config.policy);
                failure.retire(node);
            }
            Boundary::Scope(scope) => failure.retire(scope),
        }
    }
    let step = failure.finish_with(step);
    if !manager
        .primary_focus()
        .is_some_and(|focused| Rc::ptr_eq(&focused, current))
    {
        return ResolvedStep::None;
    }
    if let ResolvedStep::Focus(target) = &step
        && (!eligible(manager, target)
            || !selected_boundary
                .is_some_and(|id| target.ancestors().any(|parent| parent.id() == id)))
    {
        return ResolvedStep::None;
    }
    step
}

#[derive(Clone, Copy)]
struct Geometry {
    primary_min: f64,
    primary_max: f64,
    secondary_min: f64,
    secondary_max: f64,
}

impl Geometry {
    fn from_rect(rect: Rect<f64>, direction: FocusDirection) -> Option<Self> {
        let (left, top, right, bottom) = (rect.left(), rect.top(), rect.right(), rect.bottom());
        if ![left, top, right, bottom]
            .iter()
            .all(|value| value.is_finite())
            || left >= right
            || top >= bottom
            || !(right - left).is_finite()
            || !(bottom - top).is_finite()
        {
            return None;
        }
        let (primary_min, primary_max, secondary_min, secondary_max) = match direction {
            FocusDirection::Right => (left, right, top, bottom),
            FocusDirection::Left => (-right, -left, top, bottom),
            FocusDirection::Down => (top, bottom, left, right),
            FocusDirection::Up => (-bottom, -top, left, right),
        };
        Some(Self {
            primary_min,
            primary_max,
            secondary_min,
            secondary_max,
        })
    }
    fn center(min: f64, max: f64) -> f64 {
        min + (max - min) * 0.5
    }
    fn primary_center(self) -> f64 {
        Self::center(self.primary_min, self.primary_max)
    }
    fn secondary_center(self) -> f64 {
        Self::center(self.secondary_min, self.secondary_max)
    }
    fn in_beam(self, other: Self) -> bool {
        self.secondary_min < other.secondary_max && other.secondary_min < self.secondary_max
    }
}

/// Compare nonnegative differences without overflowing finite endpoints.
#[derive(Clone, Copy)]
struct Distance {
    overflow: bool,
    value: f64,
}
impl Distance {
    fn between(a: f64, b: f64) -> Self {
        let value = (a - b).abs();
        if value.is_finite() {
            Self {
                overflow: false,
                value,
            }
        } else {
            Self {
                overflow: true,
                value: (a * 0.5 - b * 0.5).abs(),
            }
        }
    }
    fn cmp(self, other: Self) -> Ordering {
        self.overflow
            .cmp(&other.overflow)
            .then_with(|| self.value.total_cmp(&other.value))
    }
}

fn geometry(
    node: &Rc<FocusNode>,
    direction: FocusDirection,
    failure: &mut FocusClosePanic,
) -> Option<Geometry> {
    let (provider, fallback) = node.traversal_geometry_snapshot();
    let rect = if let Some(provider) = provider {
        let rect = if failure.preserving() {
            None
        } else {
            failure.invoke(|| provider()).flatten()
        }
        .unwrap_or(fallback);
        failure.retire(provider);
        rect
    } else {
        fallback
    };
    Geometry::from_rect(rect, direction)
}

/// Cache the provider's result for this input, including unavailable geometry.
enum GeometrySnapshot {
    Unqueried,
    Sampled(Option<Geometry>),
}

struct SpatialCandidate {
    node: Rc<FocusNode>,
    geometry: GeometrySnapshot,
}

struct SpatialBoundary {
    node: Rc<FocusNode>,
    edge: TraversalEdgeBehavior,
}

struct SpatialSearch<'a> {
    manager: &'a FocusManager,
    current: &'a Rc<FocusNode>,
    source: Geometry,
    direction: FocusDirection,
}

fn spatial_target(
    search: &SpatialSearch<'_>,
    boundary: &Rc<FocusNode>,
    candidates: &mut [SpatialCandidate],
    wrap: bool,
    failure: &mut FocusClosePanic,
) -> Option<Rc<FocusNode>> {
    let mut best: Option<(Rc<FocusNode>, Geometry)> = None;
    for candidate in candidates {
        if Rc::ptr_eq(&candidate.node, search.current)
            || !eligible(search.manager, &candidate.node)
            || !belongs_to(&candidate.node, boundary)
        {
            continue;
        }
        let rect = match candidate.geometry {
            GeometrySnapshot::Unqueried => {
                let rect = geometry(&candidate.node, search.direction, failure);
                candidate.geometry = GeometrySnapshot::Sampled(rect);
                rect
            }
            GeometrySnapshot::Sampled(rect) => rect,
        };
        let Some(rect) = rect else {
            continue;
        };
        if !wrap && rect.primary_center() <= search.source.primary_center() {
            continue;
        }
        let better = best.as_ref().is_none_or(|(_, previous)| {
            let beam = previous
                .in_beam(search.source)
                .cmp(&rect.in_beam(search.source));
            let primary = if wrap {
                rect.primary_center().total_cmp(&previous.primary_center())
            } else {
                Distance::between(
                    rect.primary_min.max(search.source.primary_max),
                    search.source.primary_max,
                )
                .cmp(Distance::between(
                    previous.primary_min.max(search.source.primary_max),
                    search.source.primary_max,
                ))
            };
            let secondary =
                Distance::between(rect.secondary_center(), search.source.secondary_center()).cmp(
                    Distance::between(
                        previous.secondary_center(),
                        search.source.secondary_center(),
                    ),
                );
            beam.then(primary).then(secondary) == Ordering::Less
        });
        if better {
            best = Some((Rc::clone(&candidate.node), rect));
        }
    }
    best.map(|(node, _)| node)
}

pub(super) fn directional_step(
    manager: &FocusManager,
    current: &Rc<FocusNode>,
    direction: FocusDirection,
) -> ResolvedStep {
    // Retain every backing node and freeze edges before a provider can replace
    // registrations, release the last external owner, or reparent a subtree.
    let mut scopes = Vec::new();
    let mut boundaries = Vec::new();
    for node in current.ancestors() {
        if let Some(scope) = node.as_scope() {
            boundaries.push(SpatialBoundary {
                node,
                edge: scope.traversal_edge_behavior(),
            });
            scopes.push(scope);
        } else if let Some(edge) = node.traversal_group_edge() {
            boundaries.push(SpatialBoundary { node, edge });
        }
    }
    let mut candidates: Vec<_> = manager
        .root_scope()
        .as_focus_node()
        .descendants()
        .map(|node| SpatialCandidate {
            node,
            geometry: GeometrySnapshot::Unqueried,
        })
        .collect();
    let mut failure = FocusClosePanic::for_rejection(current.traversal_close_mode());
    let source = geometry(current, direction, &mut failure);
    let mut step = ResolvedStep::None;
    let mut selected_boundary = None;
    if let Some(source) = source {
        let search = SpatialSearch {
            manager,
            current,
            source,
            direction,
        };
        for (index, boundary) in boundaries.iter().enumerate() {
            if let Some(target) = spatial_target(
                &search,
                &boundary.node,
                &mut candidates,
                false,
                &mut failure,
            ) {
                step = ResolvedStep::Focus(target);
                selected_boundary = Some(boundary.node.id());
                break;
            }
            match boundary.edge {
                TraversalEdgeBehavior::Stop => break,
                TraversalEdgeBehavior::LeaveView => {
                    step = ResolvedStep::Unfocus;
                    break;
                }
                TraversalEdgeBehavior::ClosedLoop => {
                    if let Some(target) =
                        spatial_target(&search, &boundary.node, &mut candidates, true, &mut failure)
                    {
                        step = ResolvedStep::Focus(target);
                        selected_boundary = Some(boundary.node.id());
                    }
                    break;
                }
                TraversalEdgeBehavior::ParentScope if index + 1 == boundaries.len() => {
                    if let Some(target) =
                        spatial_target(&search, &boundary.node, &mut candidates, true, &mut failure)
                    {
                        step = ResolvedStep::Focus(target);
                        selected_boundary = Some(boundary.node.id());
                    }
                }
                TraversalEdgeBehavior::ParentScope => {}
            }
        }
    }
    for candidate in candidates {
        failure.retire(candidate.node);
    }
    for boundary in boundaries {
        failure.retire(boundary.node);
    }
    for scope in scopes {
        failure.retire(scope);
    }
    let step = failure.finish_with(step);
    if !manager
        .primary_focus()
        .is_some_and(|focused| Rc::ptr_eq(&focused, current))
    {
        return ResolvedStep::None;
    }
    if let ResolvedStep::Focus(target) = &step
        && (!eligible(manager, target)
            || !selected_boundary
                .is_some_and(|id| target.ancestors().any(|parent| parent.id() == id)))
    {
        return ResolvedStep::None;
    }
    step
}
