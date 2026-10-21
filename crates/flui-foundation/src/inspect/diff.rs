//! The structural diff of two snapshots.

use crate::geometry::{EdgeInsets, Offset, Rect, Size};

use super::fields::Unit;
use super::snapshot::{OwnedField, OwnedValue, SnapshotNode};
use super::tree::InspectId;

/// How close two numbers must be to count as equal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiffOptions {
    /// For pixels: geometry values, and fields in [`Unit::LogicalPx`] or [`Unit::DevicePx`].
    pub pixels: f64,
    /// For [`Unit::Seconds`].
    pub seconds: f64,
    /// For [`Unit::Degrees`] and [`Unit::Radians`] (in the field's own unit).
    pub angle: f64,
    /// For [`Unit::Fraction`].
    pub fraction: f64,
}

impl Default for DiffOptions {
    /// A hundredth of a pixel, a microsecond, a thousandth of a degree or radian, and a
    /// millionth of a fraction.
    fn default() -> Self {
        Self {
            pixels: 0.01,
            seconds: 1e-6,
            angle: 1e-3,
            fraction: 1e-6,
        }
    }
}

impl DiffOptions {
    /// Exact comparison everywhere.
    #[must_use]
    pub const fn exact() -> Self {
        Self {
            pixels: 0.0,
            seconds: 0.0,
            angle: 0.0,
            fraction: 0.0,
        }
    }

    fn tolerance(&self, unit: Option<Unit>) -> f64 {
        match unit {
            Some(Unit::LogicalPx | Unit::DevicePx) => self.pixels,
            Some(Unit::Seconds) => self.seconds,
            Some(Unit::Degrees | Unit::Radians) => self.angle,
            Some(Unit::Fraction) => self.fraction,
            None => 0.0,
        }
    }
}

/// Where a change is: the node's id in the newer snapshot (the older one for a removal), and
/// its path of child indices from the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodePath {
    /// The node's id.
    pub id: InspectId,
    /// The node's kind.
    pub kind: &'static str,
    /// Child indices from the root to the node.
    pub path: Vec<usize>,
}

/// One difference between two snapshots.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A node only the newer snapshot has.
    Added(NodePath),
    /// A node only the older snapshot has.
    Removed(NodePath),
    /// A node whose origin moved; its size may have changed too.
    Moved {
        /// The node.
        node: NodePath,
        /// Its bounds before.
        from: Rect,
        /// Its bounds after.
        to: Rect,
    },
    /// A node whose size changed in place.
    Resized {
        /// The node.
        node: NodePath,
        /// Its size before.
        from: Size,
        /// Its size after.
        to: Size,
    },
    /// A node whose bounds appeared or disappeared: one snapshot has them and the other does
    /// not, as a picture's do when it changes between an unbounded fill and a bounded shape.
    Bounds {
        /// The node.
        node: NodePath,
        /// Its bounds before, `None` when it had none.
        from: Option<Rect>,
        /// Its bounds after, `None` when it has none.
        to: Option<Rect>,
    },
    /// An object's own field whose value changed, appeared or disappeared.
    Field {
        /// The node.
        node: NodePath,
        /// The field.
        name: &'static str,
        /// Its value before, `None` when it is new.
        from: Option<OwnedValue>,
        /// Its value after, `None` when it is gone.
        to: Option<OwnedValue>,
    },
    /// A fact of the tree's whose value changed, appeared or disappeared. It is compared only
    /// with the facts, so a field of the same name never stands for it.
    Fact {
        /// The node.
        node: NodePath,
        /// The field.
        name: &'static str,
        /// Its value before, `None` when it is new.
        from: Option<OwnedValue>,
        /// Its value after, `None` when it is gone.
        to: Option<OwnedValue>,
    },
}

impl Change {
    /// How far a moved node's origin went.
    #[must_use]
    pub fn delta(&self) -> Option<Offset> {
        match self {
            Self::Moved { from, to, .. } => {
                Some(Offset::new(to.left() - from.left(), to.top() - from.top()))
            }
            _ => None,
        }
    }
}

/// The changes from `before` to `after`.
///
/// Children are matched by id where both snapshots have it, then the rest by position and kind;
/// what stays unmatched was added or removed. Numbers compare within the tolerance their unit
/// sets ([`DiffOptions`]); geometry compares as pixels.
#[must_use]
pub fn diff(before: &SnapshotNode, after: &SnapshotNode, options: DiffOptions) -> Vec<Change> {
    let mut changes = Vec::new();
    if same_kind(before, after) {
        compare(before, after, &options, &mut changes);
    } else {
        changes.push(Change::Removed(path_of(before, &[])));
        changes.push(Change::Added(path_of(after, &[])));
    }
    changes
}

fn path_of(node: &SnapshotNode, path: &[usize]) -> NodePath {
    NodePath {
        id: node.id,
        kind: node.kind,
        path: path.to_vec(),
    }
}

/// What the diff walk does next.
enum Step<'a> {
    /// Compare a matched pair, the `index`th child of the node at the current path.
    Enter {
        before: &'a SnapshotNode,
        after: &'a SnapshotNode,
        index: usize,
    },
    /// Report a child left unmatched, whose path was taken when its parent was compared.
    Report(Change),
    /// Leave the pair entered last.
    Leave,
}

/// Compares `before` with `after` and every matched pair below them, with its own stack so a
/// deep snapshot costs no Rust stack. Changes come in the order a recursive walk gives: a
/// node's own, then its matched children's subtrees, then its removed and its added children.
fn compare(
    before: &SnapshotNode,
    after: &SnapshotNode,
    options: &DiffOptions,
    changes: &mut Vec<Change>,
) {
    let mut path = Vec::new();
    compare_node(before, after, &path, options, changes);
    let mut steps = children_steps(before, after, &path);
    while let Some(step) = steps.pop() {
        match step {
            Step::Enter {
                before,
                after,
                index,
            } => {
                path.push(index);
                compare_node(before, after, &path, options, changes);
                steps.push(Step::Leave);
                let mut below = children_steps(before, after, &path);
                steps.append(&mut below);
            }
            Step::Report(change) => changes.push(change),
            Step::Leave => {
                path.pop();
            }
        }
    }
}

/// The steps for the children of the pair at `path`, last to run first.
fn children_steps<'a>(
    before: &'a SnapshotNode,
    after: &'a SnapshotNode,
    path: &[usize],
) -> Vec<Step<'a>> {
    let pairs = pair_children(before, after);
    let mut matched_before = vec![false; before.children.len()];
    let mut matched_after = vec![false; after.children.len()];
    for &(old, new) in &pairs {
        matched_before[old] = true;
        matched_after[new] = true;
    }
    let child_path = |index: usize| [path, &[index]].concat();
    let mut steps = Vec::new();
    for (index, child) in after.children.iter().enumerate().rev() {
        if !matched_after[index] {
            steps.push(Step::Report(Change::Added(path_of(
                child,
                &child_path(index),
            ))));
        }
    }
    for (index, child) in before.children.iter().enumerate().rev() {
        if !matched_before[index] {
            steps.push(Step::Report(Change::Removed(path_of(
                child,
                &child_path(index),
            ))));
        }
    }
    for &(old, new) in pairs.iter().rev() {
        steps.push(Step::Enter {
            before: &before.children[old],
            after: &after.children[new],
            index: new,
        });
    }
    steps
}

/// The changes to the pair at `path` itself: its bounds, facts and fields.
fn compare_node(
    before: &SnapshotNode,
    after: &SnapshotNode,
    path: &[usize],
    options: &DiffOptions,
    changes: &mut Vec<Change>,
) {
    match (before.rect, after.rect) {
        (Some(from), Some(to)) => {
            let moved = !close(from.left(), to.left(), options.pixels)
                || !close(from.top(), to.top(), options.pixels);
            if moved {
                changes.push(Change::Moved {
                    node: path_of(after, path),
                    from,
                    to,
                });
            } else if !size_close(from.size(), to.size(), options.pixels) {
                changes.push(Change::Resized {
                    node: path_of(after, path),
                    from: from.size(),
                    to: to.size(),
                });
            }
        }
        (None, None) => {}
        (from, to) => changes.push(Change::Bounds {
            node: path_of(after, path),
            from,
            to,
        }),
    }

    let node = || path_of(after, path);
    for (part, from, to) in [
        (Part::Fact, &before.facts, &after.facts),
        (Part::Field, &before.fields, &after.fields),
    ] {
        compare_part(part, from, to, &node, options, changes);
    }
}

/// Which of a node's lists a value is in.
#[derive(Clone, Copy)]
enum Part {
    Fact,
    Field,
}

impl Part {
    fn change(
        self,
        node: NodePath,
        name: &'static str,
        from: Option<OwnedValue>,
        to: Option<OwnedValue>,
    ) -> Change {
        match self {
            Self::Fact => Change::Fact {
                node,
                name,
                from,
                to,
            },
            Self::Field => Change::Field {
                node,
                name,
                from,
                to,
            },
        }
    }
}

/// Compares one list of a node with the same list of its counterpart, matched by name.
fn compare_part(
    part: Part,
    before: &[OwnedField],
    after: &[OwnedField],
    node: &dyn Fn() -> NodePath,
    options: &DiffOptions,
    changes: &mut Vec<Change>,
) {
    for field in before {
        match after.iter().find(|other| other.name == field.name) {
            Some(other) => {
                let tolerance = options.tolerance(field.unit.or(other.unit));
                if !value_close(&field.value, &other.value, tolerance, options.pixels) {
                    changes.push(part.change(
                        node(),
                        field.name,
                        Some(field.value.clone()),
                        Some(other.value.clone()),
                    ));
                }
            }
            None => changes.push(part.change(node(), field.name, Some(field.value.clone()), None)),
        }
    }
    for field in after {
        if before.iter().all(|other| other.name != field.name) {
            changes.push(part.change(node(), field.name, None, Some(field.value.clone())));
        }
    }
}

/// Pairs children by id first, then the rest by position and kind.
fn pair_children(before: &SnapshotNode, after: &SnapshotNode) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut taken_before = vec![false; before.children.len()];
    let mut taken_after = vec![false; after.children.len()];
    for (new, child) in after.children.iter().enumerate() {
        let found = before
            .children
            .iter()
            .position(|old| old.id == child.id && same_kind(old, child));
        if let Some(old) = found.filter(|&old| !taken_before[old]) {
            taken_before[old] = true;
            taken_after[new] = true;
            pairs.push((old, new));
        }
    }
    for (new, child) in after.children.iter().enumerate() {
        if taken_after[new] {
            continue;
        }
        if let Some(old) = before.children.get(new)
            && !taken_before[new]
            && same_kind(old, child)
        {
            taken_before[new] = true;
            taken_after[new] = true;
            pairs.push((new, new));
        }
    }
    pairs.sort_unstable_by_key(|&(_, new)| new);
    pairs
}

/// Whether two nodes are of one kind, type arguments included: `Widget<u8>` replaced by
/// `Widget<String>` is a removal and an addition.
fn same_kind(a: &SnapshotNode, b: &SnapshotNode) -> bool {
    a.kind == b.kind && a.kind_args == b.kind_args
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    // Bitwise-equal values (infinities, the same NaN) are unchanged; otherwise both must be
    // finite and within the tolerance.
    a.to_bits() == b.to_bits() || (a - b).abs() <= tolerance
}

fn size_close(a: Size, b: Size, tolerance: f64) -> bool {
    close(a.width, b.width, tolerance) && close(a.height, b.height, tolerance)
}

fn insets_close(a: EdgeInsets, b: EdgeInsets, tolerance: f64) -> bool {
    close(a.top, b.top, tolerance)
        && close(a.right, b.right, tolerance)
        && close(a.bottom, b.bottom, tolerance)
        && close(a.left, b.left, tolerance)
}

fn value_close(a: &OwnedValue, b: &OwnedValue, tolerance: f64, pixels: f64) -> bool {
    match (a, b) {
        (OwnedValue::Float(a), OwnedValue::Float(b)) => close(*a, *b, tolerance),
        (OwnedValue::Size(a), OwnedValue::Size(b)) => size_close(*a, *b, pixels),
        (OwnedValue::Offset(a), OwnedValue::Offset(b)) => {
            close(a.dx, b.dx, pixels) && close(a.dy, b.dy, pixels)
        }
        (OwnedValue::Rect(a), OwnedValue::Rect(b)) => {
            close(a.left(), b.left(), pixels)
                && close(a.top(), b.top(), pixels)
                && size_close(a.size(), b.size(), pixels)
        }
        (OwnedValue::EdgeInsets(a), OwnedValue::EdgeInsets(b)) => insets_close(*a, *b, pixels),
        (OwnedValue::Seq(a), OwnedValue::Seq(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(a, b)| value_close(a, b, tolerance, pixels))
        }
        // A `Debug` value is text with no type, so it compares as text.
        (OwnedValue::Debug(a), OwnedValue::Debug(b)) => a == b,
        _ => a == b,
    }
}
