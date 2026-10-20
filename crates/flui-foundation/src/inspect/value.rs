//! The closed set of values a description carries, and how a field type maps to one.

use core::fmt;
use core::time::Duration;

use crate::geometry::{Axis, EdgeInsets, Matrix4, Offset, Rect, Size};
use crate::id::{ElementId, RenderId, SemanticsId};

use super::tree::{Link, SceneLayer};

/// One field's value, borrowed from the object that describes itself.
///
/// The set is closed: a consumer that handles every variant handles every
/// description, which is what lets the owned snapshot be a wire contract.
/// `Copy` and never owning heap data, so describing a node allocates nothing.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub enum Value<'a> {
    /// A flag.
    Bool(bool),
    /// A signed integer.
    Int(i64),
    /// An unsigned integer.
    Uint(u64),
    /// A float, compared exactly; [`Meta::unit`](super::Meta::unit) sets the diff's tolerance.
    Float(f64),
    /// Text.
    Str(&'a str),
    /// A fieldless enum variant: `Axis::Horizontal` is `{ ty: "Axis", name: "Horizontal" }`.
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
    Matrix(&'a Matrix4),
    /// A span of time.
    Duration(Duration),
    /// A typed reference to a node of another tree.
    Link(Link),
    /// A type that only has `Debug`. Snapshots count these fields, so the number of untyped
    /// fields is visible and can be driven down.
    Debug(&'a dyn fmt::Debug),
    /// No value: an absent `Option`.
    None,
}

impl Value<'_> {
    /// The variant's name, as a field error names what it found.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Uint(_) => "uint",
            Self::Float(_) => "float",
            Self::Str(_) => "str",
            Self::Variant { .. } => "variant",
            Self::Size(_) => "size",
            Self::Offset(_) => "offset",
            Self::Rect(_) => "rect",
            Self::Color(_) => "color",
            Self::EdgeInsets(_) => "edge_insets",
            Self::Matrix(_) => "matrix",
            Self::Duration(_) => "duration",
            Self::Link(_) => "link",
            Self::Debug(_) => "debug",
            Self::None => "none",
        }
    }
}

/// Values compare by value. A [`Value::Debug`] equals nothing, itself included: its type is
/// unknown, so two of them cannot be shown equal.
impl PartialEq for Value<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Uint(a), Self::Uint(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Variant { ty: a, name: x }, Self::Variant { ty: b, name: y }) => {
                a == b && x == y
            }
            (Self::Size(a), Self::Size(b)) => a == b,
            (Self::Offset(a), Self::Offset(b)) => a == b,
            (Self::Rect(a), Self::Rect(b)) => a == b,
            (Self::Color(a), Self::Color(b)) => a == b,
            (Self::EdgeInsets(a), Self::EdgeInsets(b)) => a == b,
            (Self::Matrix(a), Self::Matrix(b)) => a == b,
            (Self::Duration(a), Self::Duration(b)) => a == b,
            (Self::Link(a), Self::Link(b)) => a == b,
            (Self::None, Self::None) => true,
            _ => false,
        }
    }
}

impl fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({self})", self.kind_name())
    }
}

/// The compact, pinned text form every human output uses. Numbers print in Rust's shortest
/// round-trip form, so the text parses back to the same value:
///
/// | Value | Text |
/// |---|---|
/// | `Str` | `"Save"` (quoted, escaped) |
/// | `Variant` | `Horizontal` |
/// | `Size` | `360x48` |
/// | `Offset` | `@8,8` |
/// | `Rect` | `8,8 344x32` |
/// | `Color` | `#RRGGBB`, or `#RRGGBBAA` when not opaque |
/// | `EdgeInsets` | `8`, or `top,right,bottom,left` |
/// | `Matrix` | `identity`, or the 16 entries in column order in brackets |
/// | `Duration` | `16ms`, `250us` or `40ns`, in the largest exact unit |
/// | `Link` | the linked node's id, `r12` |
/// | `None` | `none` |
impl fmt::Display for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Uint(value) => write!(f, "{value}"),
            Self::Float(value) => write!(f, "{value}"),
            Self::Str(text) => write!(f, "{text:?}"),
            Self::Variant { name, .. } => f.write_str(name),
            Self::Size(size) => write!(f, "{size}"),
            Self::Offset(offset) => write!(f, "{offset}"),
            Self::Rect(rect) => write!(f, "{rect}"),
            Self::Color(color) => write!(f, "{color}"),
            Self::EdgeInsets(insets) => write!(f, "{insets}"),
            Self::Matrix(matrix) => write_matrix(f, matrix),
            Self::Duration(duration) => write_duration(f, *duration),
            Self::Link(link) => write!(f, "{link}"),
            Self::Debug(value) => write!(f, "{value:?}"),
            Self::None => f.write_str("none"),
        }
    }
}

pub(super) fn write_matrix(f: &mut fmt::Formatter<'_>, matrix: &Matrix4) -> fmt::Result {
    if matrix.is_identity() {
        return f.write_str("identity");
    }
    f.write_str("[")?;
    for (index, entry) in matrix.m.iter().enumerate() {
        if index > 0 {
            f.write_str(",")?;
        }
        write!(f, "{entry}")?;
    }
    f.write_str("]")
}

pub(super) fn write_duration(f: &mut fmt::Formatter<'_>, duration: Duration) -> fmt::Result {
    let nanos = duration.as_nanos();
    if nanos.is_multiple_of(1_000_000) {
        write!(f, "{}ms", nanos / 1_000_000)
    } else if nanos.is_multiple_of(1_000) {
        write!(f, "{}us", nanos / 1_000)
    } else {
        write!(f, "{nanos}ns")
    }
}

/// A colour as a description carries it: straight-alpha sRGB channels in `0.0..=1.0`.
///
/// The colour type itself lives above this crate, so a description carries its channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    /// Red.
    pub red: f32,
    /// Green.
    pub green: f32,
    /// Blue.
    pub blue: f32,
    /// Alpha, straight (not premultiplied).
    pub alpha: f32,
}

impl Rgba {
    /// A colour from its four channels.
    #[must_use]
    pub const fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    /// The channels rounded to bytes, as `#RRGGBBAA` prints them.
    #[must_use]
    pub fn to_rgba8(self) -> [u8; 4] {
        [self.red, self.green, self.blue, self.alpha].map(|channel| {
            // Clamped to 0..=255 first, so the cast neither truncates nor wraps.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped to 0..=255 before the cast"
            )]
            let byte = (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
            byte
        })
    }
}

/// `#RRGGBB` when opaque, `#RRGGBBAA` otherwise.
impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [red, green, blue, alpha] = self.to_rgba8();
        write!(f, "#{red:02X}{green:02X}{blue:02X}")?;
        if alpha != u8::MAX {
            write!(f, "{alpha:02X}")?;
        }
        Ok(())
    }
}

/// A type that describes itself as one [`Value`].
///
/// `#[derive(Inspect)]` reads every field through this trait (sequences through
/// [`FieldValue`](super::FieldValue)), so a field whose type has no mapping is a compile error
/// at the derive.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no inspect value",
    label = "no `ToValue` mapping for this field's type",
    note = "mark the field `#[inspect(skip)]`, map it with `#[inspect(with = path)]`, describe it by `Debug` with `#[inspect(debug)]`, or implement `ToValue` for the type"
)]
pub trait ToValue {
    /// This value, borrowed.
    fn to_value(&self) -> Value<'_>;
}

impl ToValue for Value<'_> {
    fn to_value(&self) -> Value<'_> {
        *self
    }
}

impl<T: ToValue + ?Sized> ToValue for &T {
    fn to_value(&self) -> Value<'_> {
        (**self).to_value()
    }
}

impl<T: ToValue + ?Sized> ToValue for Box<T> {
    fn to_value(&self) -> Value<'_> {
        (**self).to_value()
    }
}

impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value<'_> {
        self.as_ref().map_or(Value::None, ToValue::to_value)
    }
}

impl ToValue for bool {
    fn to_value(&self) -> Value<'_> {
        Value::Bool(*self)
    }
}

macro_rules! to_value_as {
    ($variant:ident($target:ty): $($ty:ty),*) => {$(
        impl ToValue for $ty {
            fn to_value(&self) -> Value<'_> {
                Value::$variant(<$target>::from(*self))
            }
        }
    )*};
}

to_value_as!(Int(i64): i8, i16, i32, i64);
to_value_as!(Uint(u64): u8, u16, u32, u64);
to_value_as!(Float(f64): f32, f64);

impl ToValue for isize {
    fn to_value(&self) -> Value<'_> {
        // `isize` is at most 64 bits on every target Rust supports.
        Value::Int(i64::try_from(*self).expect("BUG: isize fits in i64"))
    }
}

impl ToValue for usize {
    fn to_value(&self) -> Value<'_> {
        // `usize` is at most 64 bits on every target Rust supports.
        Value::Uint(u64::try_from(*self).expect("BUG: usize fits in u64"))
    }
}

impl ToValue for str {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for String {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for std::borrow::Cow<'_, str> {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for std::sync::Arc<str> {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for Size {
    fn to_value(&self) -> Value<'_> {
        Value::Size(*self)
    }
}

impl ToValue for Offset {
    fn to_value(&self) -> Value<'_> {
        Value::Offset(*self)
    }
}

impl ToValue for Rect {
    fn to_value(&self) -> Value<'_> {
        Value::Rect(*self)
    }
}

impl ToValue for EdgeInsets {
    fn to_value(&self) -> Value<'_> {
        Value::EdgeInsets(*self)
    }
}

impl ToValue for Matrix4 {
    fn to_value(&self) -> Value<'_> {
        Value::Matrix(self)
    }
}

impl ToValue for Rgba {
    fn to_value(&self) -> Value<'_> {
        Value::Color(*self)
    }
}

impl ToValue for Duration {
    fn to_value(&self) -> Value<'_> {
        Value::Duration(*self)
    }
}

impl ToValue for Axis {
    fn to_value(&self) -> Value<'_> {
        Value::Variant {
            ty: "Axis",
            name: match self {
                Self::Horizontal => "Horizontal",
                Self::Vertical => "Vertical",
            },
        }
    }
}

impl ToValue for Link {
    fn to_value(&self) -> Value<'_> {
        Value::Link(*self)
    }
}

macro_rules! to_value_link {
    ($($ty:ident => $link:ident),*) => {$(
        impl ToValue for $ty {
            fn to_value(&self) -> Value<'_> {
                Value::Link(Link::$link(*self))
            }
        }
    )*};
}

to_value_link!(
    ElementId => Element,
    RenderId => Render,
    SceneLayer => Layer,
    SemanticsId => Semantics
);
