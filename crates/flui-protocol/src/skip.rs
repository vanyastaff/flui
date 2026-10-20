//! What serde leaves out of a reply: a field at its default costs the reader nothing.

/// Whether `value` is its type's default, for `skip_serializing_if`.
pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}
