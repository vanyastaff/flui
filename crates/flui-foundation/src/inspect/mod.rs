//! Typed self-description of the framework's trees (ADR-0242).
//!
//! An object says what it is through [`Inspect`]: a kind and a list of fields, each a
//! [`Value`] from a closed, borrowed set with [`Meta`] (detail level, unit, typed default).
//! Fields go to a [`Sink`], so describing a node allocates only if the sink does.
//!
//! Identity and structure belong to the tree, not to the object. An [`InspectTree`] names its
//! nodes with [`InspectId`]s over the generational ids, yields their children, links them to
//! the other trees ([`Link`]) and adds what only it knows ([`InspectTree::facts`]).
//!
//! From a tree walk come:
//!
//! - an owned [`SnapshotNode`] ([`snapshot()`]) that keeps the tree's facts apart from the
//!   object's own fields, and that tests read by type ([`SnapshotNode::get`],
//!   [`SnapshotNode::get_fact`], [`SnapshotNode::find_by_link`], [`SnapshotNode::find_by_kind`]);
//! - a structural [`diff()`] whose tolerances come from each field's [`Unit`];
//! - one human outline ([`render()`]), coloured by role only on a terminal without `NO_COLOR`.
//!
//! `#[derive(Inspect)]` writes the trait for a struct or an enum; see the derive's
//! documentation for its attributes.

mod diff;
mod fields;
mod render;
mod snapshot;
mod tree;
mod type_name;
mod value;

pub use diff::{Change, DiffOptions, NodePath, diff};
pub use fields::{
    Field, FieldMeta, FieldValue, Fields, Inspect, Level, Meta, Payload, Seq, Sink, Unit,
};
pub use render::{Ansi, Plain, Role, Style, render, style_for};
pub use snapshot::{
    FieldError, FromField, INSPECT_FAILED, MatchError, Matches, OwnedField, OwnedValue,
    SnapshotError, SnapshotNode, SnapshotQuery, Variant, snapshot,
};
pub use tree::{InspectId, InspectTree, Link, Links, NodeView, SceneLayer};
pub use type_name::{kind_args_of, kind_of};
pub use value::{Rgba, Shared, ToValue, Value};

/// Derives [`Inspect`] for a struct or an enum.
///
/// The kind is the type's name. Each field is read through [`FieldValue`] (one [`ToValue`]
/// value, or a slice or `Vec` of them as a sequence); a field type with no mapping is a
/// compile error that names the ways out. Tuple fields are named by position (`"0"`).
///
/// Field attributes, `#[inspect(...)]`:
///
/// - `skip`: leave the field out;
/// - `name = "text"`: the field's name, in place of the identifier;
/// - `default` or `default = EXPR`: the value at which a summary hides it, the type's
///   `Default` or `EXPR`;
/// - `level = detail` or `level = debug`;
/// - `unit = logical_px` (or `device_px`, `seconds`, `degrees`, `radians`, `fraction`);
/// - `with = path`: a `fn(&FieldType) -> Value<'_>` that maps the field;
/// - `debug`: describe the field by its `Debug`, counted as untyped.
///
/// An enum reports its variant as the field `variant`, then that variant's fields. A
/// fieldless enum also implements [`ToValue`], so it can be another type's field. A generic
/// type reports its type arguments through [`Inspect::kind_args`].
///
/// ```
/// use flui_foundation::geometry::EdgeInsets;
/// use flui_foundation::inspect::{Inspect, Level, SnapshotNode};
///
/// #[derive(Inspect)]
/// enum Fit {
///     Contain,
///     Cover,
/// }
///
/// #[derive(Inspect)]
/// struct Frame {
///     #[inspect(unit = logical_px)]
///     padding: EdgeInsets,
///     #[inspect(default = Fit::Contain)]
///     fit: Fit,
///     #[inspect(skip)]
///     cache: Vec<u8>,
/// }
/// # let _ = Frame { padding: EdgeInsets::default(), fit: Fit::Cover, cache: Vec::new() }.kind();
/// ```
pub use flui_macros::Inspect;
