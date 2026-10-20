//! The owned snapshot of an inspected tree, as it travels (ADR-0242 §5, ADR-0238).
//!
//! The framework describes its element, render and layer trees through
//! `flui_foundation::inspect`; a snapshot of one is an [`InspectNode`] tree here, with every
//! field typed ([`InspectValue`]) and carrying its detail [`InspectLevel`] and [`InspectUnit`].
//! Ids and links are the inspected tree's handles: a prefix for the tree (`v` element, `r`
//! render, `l` layer, `s` semantics) and the node's slot, with `.N` for a generation past the
//! first (`r12`, `v3.2`).

vocabulary! {
    /// How much detail a field is.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum InspectLevel {
        /// Shown everywhere.
        #[default]
        Summary => "summary",
        /// Shown when detail is asked for.
        Detail => "detail",
        /// Shown only when debugging the framework itself.
        Debug => "debug",
    }
}

vocabulary! {
    /// What a number measures.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum InspectUnit {
        /// Logical pixels.
        LogicalPx => "logical_px",
        /// Device pixels.
        DevicePx => "device_px",
        /// Seconds.
        Seconds => "seconds",
        /// Degrees.
        Degrees => "degrees",
        /// Radians.
        Radians => "radians",
        /// A fraction, `0.0..=1.0`.
        Fraction => "fraction",
    }
}

/// A logical rectangle: origin and size.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InspectRect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// One field's value: `{"type": "size", "value": {"width": 360, "height": 48}}`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "type", content = "value", rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum InspectValue {
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
        ty: String,
        /// The variant's name.
        name: String,
    },
    /// A logical size.
    Size {
        /// Width.
        width: f64,
        /// Height.
        height: f64,
    },
    /// A logical offset.
    Offset {
        /// Horizontal.
        dx: f64,
        /// Vertical.
        dy: f64,
    },
    /// A logical rectangle.
    Rect(InspectRect),
    /// A colour: straight-alpha sRGB channels in `0.0..=1.0`.
    Color {
        /// Red.
        red: f32,
        /// Green.
        green: f32,
        /// Blue.
        blue: f32,
        /// Alpha.
        alpha: f32,
    },
    /// Insets on the four edges.
    EdgeInsets {
        /// Top.
        top: f64,
        /// Right.
        right: f64,
        /// Bottom.
        bottom: f64,
        /// Left.
        left: f64,
    },
    /// A transform's 16 entries, column by column.
    Matrix([f64; 16]),
    /// A span of time.
    Duration {
        /// Nanoseconds.
        nanos: u64,
    },
    /// A typed reference to another tree's node, by its handle.
    Link(String),
    /// A value the framework could only describe by its `Debug` text: untyped.
    Debug(String),
    /// No value.
    None,
    /// A sequence field's items.
    Seq(Vec<InspectValue>),
}

/// One field of a node.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InspectField {
    /// The field's name.
    pub name: String,
    /// Its value.
    pub value: InspectValue,
    /// How much detail it is; left out at `summary`.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_summary"))]
    pub level: InspectLevel,
    /// What it measures.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub unit: Option<InspectUnit>,
    /// The value at which it says nothing notable.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub default: Option<InspectValue>,
}

impl InspectField {
    /// A summary field with no unit and no default.
    #[must_use]
    pub fn new(name: impl Into<String>, value: InspectValue) -> Self {
        Self {
            name: name.into(),
            value,
            level: InspectLevel::Summary,
            unit: None,
            default: None,
        }
    }
}

/// One node of an inspected tree, and the part of its subtree the snapshot took.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InspectNode {
    /// The node's handle in its tree: `r12`.
    pub id: String,
    /// Its kind: `RenderPadding`.
    pub kind: String,
    /// Its kind's type arguments, for a generic kind.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub kind_args: Vec<String>,
    /// The tree's facts, then the object's own fields.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub fields: Vec<InspectField>,
    /// Its committed bounds in its parent's logical coordinates.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub rect: Option<InspectRect>,
    /// Handles of its counterparts in the other trees.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub links: Vec<String>,
    /// The children the snapshot took.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub children: Vec<InspectNode>,
    /// Children left out by a depth or node limit.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_zero"))]
    pub omitted_children: u32,
    /// How many of its fields are [`InspectValue::Debug`].
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_zero"))]
    pub debug_fields: u32,
}

impl InspectNode {
    /// A node with no fields, bounds, links or children.
    #[must_use]
    pub fn new(id: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            kind_args: Vec::new(),
            fields: Vec::new(),
            rect: None,
            links: Vec::new(),
            children: Vec::new(),
            omitted_children: 0,
            debug_fields: 0,
        }
    }
}

#[cfg(feature = "serde")]
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
fn is_zero(count: &u32) -> bool {
    *count == 0
}

#[cfg(feature = "serde")]
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
fn is_summary(level: &InspectLevel) -> bool {
    *level == InspectLevel::Summary
}
