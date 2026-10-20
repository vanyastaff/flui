//! The owned snapshot of a tree walk, and typed access to it.

use core::fmt;
use core::time::Duration;

use crate::geometry::{EdgeInsets, Matrix4, Offset, Rect, Size};

use super::fields::{Field, Fields, Inspect, Level, Meta, Seq, Sink, Unit};
use super::tree::{InspectId, InspectTree, Link, Links};
use super::value::{Rgba, Value};

/// A [`Value`] that owns its data, as a snapshot keeps it.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum OwnedValue {
    /// A flag.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    Uint(u64),
    /// A float.
    Float(f64),
    /// Text.
    Str(String),
    /// A fieldless enum variant.
    Variant {
        /// The enum's name.
        ty: &'static str,
        /// The variant's name.
        name: &'static str,
    },
    /// A logical size.
    Size(Size),
    /// A logical offset.
    Offset(Offset),
    /// A logical rectangle.
    Rect(Rect),
    /// A colour.
    Color(Rgba),
    /// Insets on the four edges.
    EdgeInsets(EdgeInsets),
    /// A transform.
    Matrix(Box<Matrix4>),
    /// A span of time.
    Duration(Duration),
    /// A typed reference to a node of another tree.
    Link(Link),
    /// A type that only has `Debug`, as its `Debug` text.
    Debug(String),
    /// No value.
    None,
    /// A sequence field's items.
    Seq(Vec<OwnedValue>),
}

impl OwnedValue {
    /// An owned copy of `value`.
    #[must_use]
    pub fn from_value(value: Value<'_>) -> Self {
        match value {
            Value::Bool(value) => Self::Bool(value),
            Value::Int(value) => Self::Int(value),
            Value::Uint(value) => Self::Uint(value),
            Value::Float(value) => Self::Float(value),
            Value::Str(text) => Self::Str(text.to_owned()),
            Value::Variant { ty, name } => Self::Variant { ty, name },
            Value::Size(size) => Self::Size(size),
            Value::Offset(offset) => Self::Offset(offset),
            Value::Rect(rect) => Self::Rect(rect),
            Value::Color(color) => Self::Color(color),
            Value::EdgeInsets(insets) => Self::EdgeInsets(insets),
            Value::Matrix(matrix) => Self::Matrix(Box::new(*matrix)),
            Value::Duration(duration) => Self::Duration(duration),
            Value::Link(link) => Self::Link(link),
            Value::Debug(value) => Self::Debug(format!("{value:?}")),
            Value::None => Self::None,
        }
    }

    /// The value, borrowed. A sequence has no single value and answers `None`.
    #[must_use]
    pub fn as_value(&self) -> Option<Value<'_>> {
        Some(match self {
            Self::Bool(value) => Value::Bool(*value),
            Self::Int(value) => Value::Int(*value),
            Self::Uint(value) => Value::Uint(*value),
            Self::Float(value) => Value::Float(*value),
            Self::Str(text) => Value::Str(text),
            Self::Variant { ty, name } => Value::Variant { ty, name },
            Self::Size(size) => Value::Size(*size),
            Self::Offset(offset) => Value::Offset(*offset),
            Self::Rect(rect) => Value::Rect(*rect),
            Self::Color(color) => Value::Color(*color),
            Self::EdgeInsets(insets) => Value::EdgeInsets(*insets),
            Self::Matrix(matrix) => Value::Matrix(matrix),
            Self::Duration(duration) => Value::Duration(*duration),
            Self::Link(link) => Value::Link(*link),
            // The text, so it prints as written rather than quoted.
            Self::Debug(text) => Value::Debug(text),
            Self::None => Value::None,
            Self::Seq(_) => return None,
        })
    }

    /// The variant's name, as a field error names what it found.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        self.as_value().map_or("seq", |value| value.kind_name())
    }
}

/// Prints as [`Value`] does; a sequence prints its items in brackets.
impl fmt::Display for OwnedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Seq(items) = self {
            f.write_str("[")?;
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{item}")?;
            }
            return f.write_str("]");
        }
        match self.as_value() {
            Some(Value::Debug(_)) => match self {
                Self::Debug(text) => f.write_str(text),
                _ => unreachable!("BUG: only an owned Debug value borrows as Value::Debug"),
            },
            Some(value) => write!(f, "{value}"),
            None => Ok(()),
        }
    }
}

/// One field of a snapshot node.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnedField {
    /// The field's name.
    pub name: &'static str,
    /// Its value.
    pub value: OwnedValue,
    /// How much detail it is.
    pub level: Level,
    /// What it measures.
    pub unit: Option<Unit>,
    /// Its default, when it has one.
    pub default: Option<OwnedValue>,
}

impl OwnedField {
    /// Whether the value equals the field's default.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.default.as_ref().is_some_and(|default| {
            match (default.as_value(), self.value.as_value()) {
                (Some(default), Some(value)) => default == value,
                _ => false,
            }
        })
    }
}

/// How much of a tree a snapshot takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotQuery {
    /// Where to start; `None` for the tree's root.
    pub root: Option<InspectId>,
    /// How many levels below the start to descend; `None` for all.
    pub max_depth: Option<u32>,
    /// How many nodes to take at most; the rest are counted in `omitted_children`.
    pub max_nodes: Option<u32>,
    /// The most detailed fields to keep.
    pub level: Level,
}

impl Default for SnapshotQuery {
    fn default() -> Self {
        Self {
            root: None,
            max_depth: None,
            max_nodes: None,
            level: Level::Debug,
        }
    }
}

/// One node of an owned snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotNode {
    /// The node's id in its tree.
    pub id: InspectId,
    /// Its kind.
    pub kind: &'static str,
    /// Its kind's type arguments, for a generic kind.
    pub kind_args: Vec<&'static str>,
    /// The tree's facts, then the object's own fields, in the order written.
    pub fields: Vec<OwnedField>,
    /// Its committed bounds in its parent's logical coordinates.
    pub rect: Option<Rect>,
    /// Its counterparts in the other trees.
    pub links: Links,
    /// Its children that were taken.
    pub children: Vec<SnapshotNode>,
    /// Its children left out by the depth or node limit.
    pub omitted_children: u32,
    /// How many of its fields are [`Value::Debug`] (untyped).
    pub debug_fields: u32,
}

/// Why a snapshot has no node to start from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    /// The tree has no root.
    #[error("the tree is empty")]
    Empty,
    /// The start node is not in the tree: it was removed, or its slot holds a newer node.
    #[error("{0} is gone")]
    Gone(InspectId),
}

/// Takes an owned snapshot of `tree` from the query's start node.
///
/// # Errors
///
/// [`SnapshotError::Empty`] for a tree with no root and no start node asked for, and
/// [`SnapshotError::Gone`] when the start node asked for is not in the tree.
pub fn snapshot<T: InspectTree + ?Sized>(
    tree: &T,
    query: SnapshotQuery,
) -> Result<SnapshotNode, SnapshotError> {
    let start = query
        .root
        .or_else(|| tree.root())
        .ok_or(SnapshotError::Empty)?;
    let mut budget = query.max_nodes.unwrap_or(u32::MAX);
    take(tree, start, 0, &query, &mut budget).ok_or(SnapshotError::Gone(start))
}

fn take<T: InspectTree + ?Sized>(
    tree: &T,
    id: InspectId,
    depth: u32,
    query: &SnapshotQuery,
    budget: &mut u32,
) -> Option<SnapshotNode> {
    let view = tree.node(id)?;
    *budget = budget.saturating_sub(1);
    let mut kind_args = Vec::new();
    view.object.kind_args(&mut |arg| kind_args.push(arg));
    let mut sink = OwnedSink {
        level: query.level,
        fields: Vec::new(),
        debug_fields: 0,
    };
    tree.facts(id, &mut Fields::new(&mut sink));
    view.object.inspect(&mut Fields::new(&mut sink));
    let mut node = SnapshotNode {
        id,
        kind: view.object.kind(),
        kind_args,
        fields: sink.fields,
        rect: view.rect,
        links: view.links,
        children: Vec::new(),
        omitted_children: 0,
        debug_fields: sink.debug_fields,
    };
    let descend = query.max_depth.is_none_or(|max| depth < max);
    for child in tree.children(id) {
        if descend
            && *budget > 0
            && let Some(child) = take(tree, child, depth + 1, query, budget)
        {
            node.children.push(child);
            continue;
        }
        node.omitted_children = node.omitted_children.saturating_add(1);
    }
    Some(node)
}

/// Copies fields at or under its level.
struct OwnedSink {
    level: Level,
    fields: Vec<OwnedField>,
    debug_fields: u32,
}

impl OwnedSink {
    fn push(&mut self, name: &'static str, value: OwnedValue, meta: Meta<'_>) {
        if matches!(value, OwnedValue::Debug(_)) {
            self.debug_fields = self.debug_fields.saturating_add(1);
        }
        self.fields.push(OwnedField {
            name,
            value,
            level: meta.level,
            unit: meta.unit,
            default: meta.default.map(OwnedValue::from_value),
        });
    }
}

impl Sink for OwnedSink {
    fn field(&mut self, field: Field<'_>) {
        if field.meta.level <= self.level {
            self.push(field.name, OwnedValue::from_value(field.value), field.meta);
        }
    }

    fn seq(&mut self, name: &'static str, meta: Meta<'_>, items: &dyn Seq) {
        if meta.level <= self.level {
            let mut values = Vec::new();
            items.each(&mut |item| values.push(OwnedValue::from_value(item)));
            self.push(name, OwnedValue::Seq(values), meta);
        }
    }
}

/// Why a typed field read failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FieldError {
    /// The node has no such field.
    #[error("{kind} {id} has no field `{name}`")]
    Missing {
        /// The node's kind.
        kind: &'static str,
        /// The node's id.
        id: InspectId,
        /// The field asked for.
        name: String,
    },
    /// The field holds another kind of value.
    #[error("{kind} {id}'s field `{name}` is {found}, not {expected}")]
    WrongKind {
        /// The node's kind.
        kind: &'static str,
        /// The node's id.
        id: InspectId,
        /// The field asked for.
        name: String,
        /// The kind of value asked for.
        expected: &'static str,
        /// The kind of value the field holds.
        found: &'static str,
    },
}

/// A type a snapshot field can be read as.
pub trait FromField<'a>: Sized {
    /// The value kind this type reads, as an error names it.
    const EXPECTED: &'static str;

    /// The field's value as `Self`, or `None` when it holds another kind.
    fn from_field(value: &'a OwnedValue) -> Option<Self>;
}

macro_rules! from_field {
    ($($ty:ty => $expected:literal, $pattern:pat => $out:expr;)*) => {$(
        impl<'a> FromField<'a> for $ty {
            const EXPECTED: &'static str = $expected;

            fn from_field(value: &'a OwnedValue) -> Option<Self> {
                match value {
                    $pattern => Some($out),
                    _ => None,
                }
            }
        }
    )*};
}

from_field! {
    bool => "bool", OwnedValue::Bool(value) => *value;
    i64 => "int", OwnedValue::Int(value) => *value;
    u64 => "uint", OwnedValue::Uint(value) => *value;
    f64 => "float", OwnedValue::Float(value) => *value;
    &'a str => "str", OwnedValue::Str(text) => text.as_str();
    Size => "size", OwnedValue::Size(size) => *size;
    Offset => "offset", OwnedValue::Offset(offset) => *offset;
    Rect => "rect", OwnedValue::Rect(rect) => *rect;
    Rgba => "color", OwnedValue::Color(color) => *color;
    EdgeInsets => "edge_insets", OwnedValue::EdgeInsets(insets) => *insets;
    &'a Matrix4 => "matrix", OwnedValue::Matrix(matrix) => matrix.as_ref();
    Duration => "duration", OwnedValue::Duration(duration) => *duration;
    Link => "link", OwnedValue::Link(link) => *link;
    &'a [OwnedValue] => "seq", OwnedValue::Seq(items) => items.as_slice();
}

/// A variant field read by its variant name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variant<'a> {
    /// The enum's name.
    pub ty: &'a str,
    /// The variant's name.
    pub name: &'a str,
}

impl<'a> FromField<'a> for Variant<'a> {
    const EXPECTED: &'static str = "variant";

    fn from_field(value: &'a OwnedValue) -> Option<Self> {
        match value {
            OwnedValue::Variant { ty, name } => Some(Self { ty, name }),
            _ => None,
        }
    }
}

/// Why a lookup by kind did not find exactly one node.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MatchError {
    /// No node matched.
    #[error("no {kind} in the tree")]
    None {
        /// The kind looked for.
        kind: String,
    },
    /// Several nodes matched.
    #[error("{count} nodes are {kind}: {ids:?}; name one by its id or link")]
    Ambiguous {
        /// The kind looked for.
        kind: String,
        /// How many matched.
        count: usize,
        /// The ids of the matches.
        ids: Vec<InspectId>,
    },
}

/// The nodes of one kind, in tree order.
#[derive(Clone, Debug)]
pub struct Matches<'a> {
    kind: String,
    nodes: Vec<&'a SnapshotNode>,
}

impl<'a> Matches<'a> {
    /// The one match.
    ///
    /// # Errors
    ///
    /// [`MatchError::None`] when nothing matched, [`MatchError::Ambiguous`] with the ids when
    /// several did.
    pub fn unique(self) -> Result<&'a SnapshotNode, MatchError> {
        match self.nodes.as_slice() {
            [node] => Ok(node),
            [] => Err(MatchError::None { kind: self.kind }),
            nodes => Err(MatchError::Ambiguous {
                count: nodes.len(),
                ids: nodes.iter().map(|node| node.id).collect(),
                kind: self.kind,
            }),
        }
    }

    /// The matches, in tree order.
    #[must_use]
    pub fn as_slice(&self) -> &[&'a SnapshotNode] {
        &self.nodes
    }

    /// How many nodes matched.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether nothing matched.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

impl<'a> IntoIterator for Matches<'a> {
    type Item = &'a SnapshotNode;
    type IntoIter = std::vec::IntoIter<&'a SnapshotNode>;

    fn into_iter(self) -> Self::IntoIter {
        self.nodes.into_iter()
    }
}

impl SnapshotNode {
    /// The field `name`, untyped.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&OwnedField> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// The field `name` read as `T`: `node.get::<f64>("flex")`.
    ///
    /// # Errors
    ///
    /// [`FieldError::Missing`] when the node has no such field, [`FieldError::WrongKind`] when
    /// it holds another kind of value.
    pub fn get<'a, T: FromField<'a>>(&'a self, name: &str) -> Result<T, FieldError> {
        let field = self.field(name).ok_or_else(|| FieldError::Missing {
            kind: self.kind,
            id: self.id,
            name: name.to_owned(),
        })?;
        T::from_field(&field.value).ok_or_else(|| FieldError::WrongKind {
            kind: self.kind,
            id: self.id,
            name: name.to_owned(),
            expected: T::EXPECTED,
            found: field.value.kind_name(),
        })
    }

    /// This node and its descendants, depth first, parents before children.
    pub fn iter(&self) -> impl Iterator<Item = &SnapshotNode> {
        let mut stack = vec![self];
        core::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(node.children.iter().rev());
            Some(node)
        })
    }

    /// The node whose id is `link`, or that links to it.
    #[must_use]
    pub fn find_by_link(&self, link: Link) -> Option<&SnapshotNode> {
        self.iter()
            .find(|node| node.id.link() == link)
            .or_else(|| self.iter().find(|node| node.links.contains(link)))
    }

    /// The nodes of `kind`, in tree order.
    #[must_use]
    pub fn find_by_kind(&self, kind: &str) -> Matches<'_> {
        Matches {
            kind: kind.to_owned(),
            nodes: self.iter().filter(|node| node.kind == kind).collect(),
        }
    }

    /// How many fields of the whole subtree are [`Value::Debug`].
    #[must_use]
    pub fn total_debug_fields(&self) -> u64 {
        self.iter().map(|node| u64::from(node.debug_fields)).sum()
    }
}
