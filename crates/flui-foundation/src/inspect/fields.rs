//! What an object says about itself: the [`Inspect`] trait, its fields and their metadata.

use super::value::{ToValue, Value};

/// One node's own description.
///
/// A tree walker calls it; it never describes children, because structure belongs to the tree
/// that owns the node ([`InspectTree`](super::InspectTree)). There is no `Debug` supertrait.
///
/// Usually derived (`#[derive(Inspect)]`); a hand-written body is a list of
/// [`Fields::field`] calls:
///
/// ```
/// use flui_foundation::geometry::Size;
/// use flui_foundation::inspect::{Fields, Inspect, Unit};
///
/// struct Spacer {
///     size: Size,
///     flex: u32,
/// }
///
/// impl Inspect for Spacer {
///     fn kind(&self) -> &'static str {
///         "Spacer"
///     }
///
///     fn inspect(&self, out: &mut Fields<'_>) {
///         out.field("size", &self.size).unit(Unit::LogicalPx);
///         out.field("flex", &self.flex).default(&1_u32);
///     }
/// }
/// ```
pub trait Inspect {
    /// The node's kind, the type's own name: `"RenderPadding"`.
    fn kind(&self) -> &'static str;

    /// The type arguments of a generic kind, in order, as [`core::any::type_name`] spells them.
    /// The derive reports one per type parameter; a non-generic type reports none.
    fn kind_args(&self, args: &mut dyn FnMut(&'static str)) {
        let _ = args;
    }

    /// Writes this node's own fields.
    fn inspect(&self, out: &mut Fields<'_>);
}

impl<T: Inspect + ?Sized> Inspect for &T {
    fn kind(&self) -> &'static str {
        (**self).kind()
    }

    fn kind_args(&self, args: &mut dyn FnMut(&'static str)) {
        (**self).kind_args(args);
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        (**self).inspect(out);
    }
}

impl<T: Inspect + ?Sized> Inspect for Box<T> {
    fn kind(&self) -> &'static str {
        (**self).kind()
    }

    fn kind_args(&self, args: &mut dyn FnMut(&'static str)) {
        (**self).kind_args(args);
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        (**self).inspect(out);
    }
}

/// How much detail a field is: a summary shows only [`Level::Summary`] fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Shown everywhere.
    #[default]
    Summary,
    /// Shown when detail is asked for.
    Detail,
    /// Shown only when debugging the framework itself.
    Debug,
}

impl Level {
    /// The level's name: `summary`, `detail` or `debug`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Detail => "detail",
            Self::Debug => "debug",
        }
    }
}

/// What a number measures. It sets how close two values must be for the structural diff to
/// call them equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unit {
    /// Logical pixels.
    LogicalPx,
    /// Device pixels.
    DevicePx,
    /// Seconds.
    Seconds,
    /// Degrees.
    Degrees,
    /// Radians.
    Radians,
    /// A fraction, `0.0..=1.0`: an opacity, a progress, an alignment factor.
    Fraction,
}

impl Unit {
    /// The unit's name: `logical_px`, `device_px`, `seconds`, `degrees`, `radians` or `fraction`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LogicalPx => "logical_px",
            Self::DevicePx => "device_px",
            Self::Seconds => "seconds",
            Self::Degrees => "degrees",
            Self::Radians => "radians",
            Self::Fraction => "fraction",
        }
    }
}

/// A field's metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meta<'a> {
    /// How much detail the field is.
    pub level: Level,
    /// What the field measures, when it is a number or geometry.
    pub unit: Option<Unit>,
    /// The value at which the field says nothing notable. A summary hides a field equal to it,
    /// compared by value.
    pub default: Option<Value<'a>>,
}

impl Meta<'_> {
    /// Whether `value` equals this field's default.
    #[must_use]
    pub fn is_default(&self, value: &Value<'_>) -> bool {
        self.default.is_some_and(|default| default == *value)
    }
}

/// One described field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field<'a> {
    /// The field's name.
    pub name: &'static str,
    /// Its value.
    pub value: Value<'a>,
    /// Its metadata.
    pub meta: Meta<'a>,
}

/// The items of a sequence field, handed to a [`Sink`] one by one.
pub trait Seq {
    /// Calls `item` with each element, in order.
    fn each(&self, item: &mut dyn FnMut(Value<'_>));
}

fn each_of<T: ToValue>(items: &[T], item: &mut dyn FnMut(Value<'_>)) {
    for element in items {
        item(element.to_value());
    }
}

impl<T: ToValue> Seq for Vec<T> {
    fn each(&self, item: &mut dyn FnMut(Value<'_>)) {
        each_of(self, item);
    }
}

impl<T: ToValue, const N: usize> Seq for [T; N] {
    fn each(&self, item: &mut dyn FnMut(Value<'_>)) {
        each_of(self, item);
    }
}

impl<T: ToValue> Seq for &[T] {
    fn each(&self, item: &mut dyn FnMut(Value<'_>)) {
        each_of(self, item);
    }
}

/// Receives a description. The one `dyn` boundary of the model: whether describing a node
/// allocates is the sink's decision alone.
pub trait Sink {
    /// One field.
    fn field(&mut self, field: Field<'_>);

    /// One sequence field.
    fn seq(&mut self, name: &'static str, meta: Meta<'_>, items: &dyn Seq);
}

/// What a field holds: one value, or a sequence of them.
#[derive(Clone, Copy)]
pub enum Payload<'a> {
    /// One value.
    One(Value<'a>),
    /// A sequence.
    Seq(&'a dyn Seq),
}

impl core::fmt::Debug for Payload<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::One(value) => f.debug_tuple("One").field(value).finish(),
            Self::Seq(_) => f.write_str("Seq(..)"),
        }
    }
}

/// A field type: a [`ToValue`] type is one value; a `Vec`, an array or a slice of them is a
/// sequence.
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no inspect value",
    label = "no `ToValue` mapping for this field's type",
    note = "mark the field `#[inspect(skip)]`, map it with `#[inspect(with = path)]`, describe it by `Debug` with `#[inspect(debug)]`, or implement `ToValue` for the type"
)]
pub trait FieldValue {
    /// This field's payload, borrowed.
    fn payload(&self) -> Payload<'_>;
}

impl<T: ToValue + ?Sized> FieldValue for T {
    fn payload(&self) -> Payload<'_> {
        Payload::One(self.to_value())
    }
}

impl<T: ToValue> FieldValue for Vec<T> {
    fn payload(&self) -> Payload<'_> {
        Payload::Seq(self)
    }
}

impl<T: ToValue, const N: usize> FieldValue for [T; N] {
    fn payload(&self) -> Payload<'_> {
        Payload::Seq(self)
    }
}

impl<T: ToValue> FieldValue for &[T] {
    fn payload(&self) -> Payload<'_> {
        Payload::Seq(self)
    }
}

/// Where an [`Inspect`] body writes its fields.
pub struct Fields<'s> {
    sink: &'s mut dyn Sink,
}

impl core::fmt::Debug for Fields<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Fields(..)")
    }
}

impl<'s> Fields<'s> {
    /// Fields written into `sink`.
    pub fn new(sink: &'s mut dyn Sink) -> Self {
        Self { sink }
    }

    /// Writes the field `name`. Metadata is set on the returned builder, and the field reaches
    /// the sink when the builder drops, at the end of the statement.
    pub fn field<'f, 'v, T: FieldValue + ?Sized>(
        &'f mut self,
        name: &'static str,
        value: &'v T,
    ) -> FieldMeta<'f, 'v> {
        FieldMeta {
            sink: &mut *self.sink,
            name,
            payload: value.payload(),
            meta: Meta::default(),
        }
    }

    /// Writes the field `name` with a value computed in place.
    pub fn value<'f, 'v>(&'f mut self, name: &'static str, value: Value<'v>) -> FieldMeta<'f, 'v> {
        FieldMeta {
            sink: &mut *self.sink,
            name,
            payload: Payload::One(value),
            meta: Meta::default(),
        }
    }
}

/// A field on its way to the sink. Setting metadata is optional; dropping it writes the field.
pub struct FieldMeta<'f, 'v> {
    sink: &'f mut dyn Sink,
    name: &'static str,
    payload: Payload<'v>,
    meta: Meta<'v>,
}

impl core::fmt::Debug for FieldMeta<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FieldMeta")
            .field("name", &self.name)
            .field("payload", &self.payload)
            .field("meta", &self.meta)
            .finish_non_exhaustive()
    }
}

impl<'v> FieldMeta<'_, 'v> {
    /// Sets how much detail the field is.
    pub fn level(&mut self, level: Level) -> &mut Self {
        self.meta.level = level;
        self
    }

    /// Sets what the field measures.
    pub fn unit(&mut self, unit: Unit) -> &mut Self {
        self.meta.unit = Some(unit);
        self
    }

    /// Sets the value at which the field says nothing notable.
    pub fn default<D: ToValue + ?Sized>(&mut self, default: &'v D) -> &mut Self {
        self.meta.default = Some(default.to_value());
        self
    }

    /// Sets the default to a value computed in place.
    pub fn default_value(&mut self, default: Value<'v>) -> &mut Self {
        self.meta.default = Some(default);
        self
    }
}

impl Drop for FieldMeta<'_, '_> {
    fn drop(&mut self) {
        match self.payload {
            // A number the protocol cannot carry (infinity, NaN) is an absent value, whatever the
            // object holds: an unbounded bound is "no bound", never a published infinity.
            Payload::One(value) => self.sink.field(Field {
                name: self.name,
                value: if value.is_finite() {
                    value
                } else {
                    Value::None
                },
                meta: self.meta,
            }),
            Payload::Seq(items) => self.sink.seq(self.name, self.meta, items),
        }
    }
}
