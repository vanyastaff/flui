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

struct GroupSnapshot {
    id: FocusNodeId,
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

    pub(super) fn order(&self, nodes: &mut Vec<Rc<FocusNode>>, failure: &mut FocusClosePanic) {
        self.order_at(nodes, 0, failure);
    }

    fn order_at(
        &self,
        nodes: &mut Vec<Rc<FocusNode>>,
        depth: usize,
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
            if let Some(group) = self.groups.iter().find(|group| group.id == id) {
                failure.run(|| {
                    group
                        .config
                        .policy
                        .order(&mut members, group.config.direction)
                });
            }
            self.order_at(&mut members, depth + 1, failure);
            output.append(&mut members);
        }
        *nodes = output;
    }

    pub(super) fn retire(self, failure: &mut FocusClosePanic) {
        for group in self.groups {
            failure.retire(group.config.policy);
        }
    }
}

fn group_of(node: &Rc<FocusNode>) -> Option<Rc<FocusNode>> {
    node.ancestors()
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

fn group_order(group: &Rc<FocusNode>, config: GroupConfig) -> Vec<Rc<FocusNode>> {
    let mut nodes: Vec<_> = group
        .descendants()
        .filter(|node| !node.is_scope() && node.can_request_focus() && !node.skip_traversal())
        .collect();
    let snapshot = GroupOrderSnapshot::new(&nodes, group);
    let mut failure = FocusClosePanic::for_rejection(group.traversal_close_mode());
    failure.run(|| config.policy.order(&mut nodes, config.direction));
    snapshot.order(&mut nodes, &mut failure);
    snapshot.retire(&mut failure);
    failure.retire(config.policy);
    failure.finish_with(nodes)
}

pub(super) fn linear_step(
    manager: &FocusManager,
    current: Option<&Rc<FocusNode>>,
    scope: &Rc<FocusScopeNode>,
    direction: TraversalDirection,
) -> ResolvedStep {
    if let Some(current) = current {
        if let Some(target) = current.traversal_override_target(direction)
            && !Rc::ptr_eq(current, &target)
            && eligible(manager, &target)
            && same_boundary(current, &target)
        {
            return ResolvedStep::Focus(target);
        }
        let mut boundary = group_of(current);
        while let Some(group) = boundary {
            let Some(config) = group.traversal_group_snapshot() else {
                break;
            };
            let edge = config.edge;
            let nodes = group_order(&group, config);
            let index = nodes.iter().position(|node| Rc::ptr_eq(node, current));
            let next = match direction {
                TraversalDirection::Forward => index.and_then(|index| nodes.get(index + 1)),
                TraversalDirection::Backward => {
                    index.and_then(|index| index.checked_sub(1).and_then(|index| nodes.get(index)))
                }
            };
            let step = if let Some(next) = next {
                ResolvedStep::Focus(Rc::clone(next))
            } else {
                match edge {
                    TraversalEdgeBehavior::ClosedLoop => match direction {
                        TraversalDirection::Forward => nodes.first(),
                        TraversalDirection::Backward => nodes.last(),
                    }
                    .map_or(ResolvedStep::None, |node| {
                        ResolvedStep::Focus(Rc::clone(node))
                    }),
                    TraversalEdgeBehavior::Stop => ResolvedStep::None,
                    TraversalEdgeBehavior::LeaveView => ResolvedStep::Unfocus,
                    TraversalEdgeBehavior::ParentScope => ResolvedStep::RetryInParent,
                }
            };
            let mut failure = FocusClosePanic::for_rejection(group.traversal_close_mode());
            for node in nodes {
                failure.retire(node);
            }
            failure.finish();
            if !matches!(step, ResolvedStep::RetryInParent) {
                return step;
            }
            boundary = group_of(&group);
        }
    }
    scope.step(current, direction)
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
        let span = max - min;
        if span.is_finite() {
            min + span * 0.5
        } else {
            min * 0.5 + max * 0.5
        }
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
        let rect = failure.invoke(|| provider()).flatten().unwrap_or(fallback);
        failure.retire(provider);
        rect
    } else {
        fallback
    };
    Geometry::from_rect(rect, direction)
}

fn spatial_target(
    manager: &FocusManager,
    current: &Rc<FocusNode>,
    boundary: &Rc<FocusNode>,
    direction: FocusDirection,
    wrap: bool,
) -> Option<Rc<FocusNode>> {
    let candidates: Vec<_> = boundary
        .descendants()
        .filter(|node| eligible(manager, node) && !Rc::ptr_eq(node, current))
        .collect();
    let mut failure = FocusClosePanic::for_rejection(boundary.traversal_close_mode());
    let source = geometry(current, direction, &mut failure);
    let mut best: Option<(Rc<FocusNode>, Geometry)> = None;
    if let Some(source) = source {
        for node in &candidates {
            let Some(rect) = geometry(node, direction, &mut failure) else {
                continue;
            };
            if !wrap && rect.primary_center() <= source.primary_center() {
                continue;
            }
            let better = best.as_ref().is_none_or(|(_, previous)| {
                let beam = previous.in_beam(source).cmp(&rect.in_beam(source));
                let primary = if wrap {
                    rect.primary_center().total_cmp(&previous.primary_center())
                } else {
                    Distance::between(rect.primary_min.max(source.primary_max), source.primary_max)
                        .cmp(Distance::between(
                            previous.primary_min.max(source.primary_max),
                            source.primary_max,
                        ))
                };
                let secondary =
                    Distance::between(rect.secondary_center(), source.secondary_center()).cmp(
                        Distance::between(previous.secondary_center(), source.secondary_center()),
                    );
                beam.then(primary).then(secondary) == Ordering::Less
            });
            if better {
                best = Some((Rc::clone(node), rect));
            }
        }
    }
    for node in candidates {
        failure.retire(node);
    }
    failure.finish_with(best.map(|(node, _)| node))
}

pub(super) fn directional_step(
    manager: &FocusManager,
    current: &Rc<FocusNode>,
    direction: FocusDirection,
) -> ResolvedStep {
    let mut group = group_of(current);
    let mut scope = current.enclosing_scope();
    loop {
        let (boundary, edge) = if let Some(group) = &group {
            let Some(config) = group.traversal_group_snapshot() else {
                return ResolvedStep::None;
            };
            let edge = config.edge;
            let mut failure = FocusClosePanic::for_rejection(group.traversal_close_mode());
            failure.retire(config.policy);
            failure.finish();
            (Rc::clone(group), edge)
        } else if let Some(scope) = &scope {
            (
                Rc::clone(scope.as_focus_node()),
                scope.traversal_edge_behavior(),
            )
        } else {
            return ResolvedStep::None;
        };
        if let Some(target) = spatial_target(manager, current, &boundary, direction, false) {
            return ResolvedStep::Focus(target);
        }
        match edge {
            TraversalEdgeBehavior::Stop => return ResolvedStep::None,
            TraversalEdgeBehavior::LeaveView => return ResolvedStep::Unfocus,
            TraversalEdgeBehavior::ClosedLoop => {
                return spatial_target(manager, current, &boundary, direction, true)
                    .map_or(ResolvedStep::None, ResolvedStep::Focus);
            }
            TraversalEdgeBehavior::ParentScope => {
                if let Some(previous) = group.take() {
                    group = group_of(&previous);
                } else if let Some(previous) = scope.take() {
                    scope = previous.as_focus_node().enclosing_scope();
                    if scope.is_none() {
                        return spatial_target(manager, current, &boundary, direction, true)
                            .map_or(ResolvedStep::None, ResolvedStep::Focus);
                    }
                }
            }
        }
    }
}
