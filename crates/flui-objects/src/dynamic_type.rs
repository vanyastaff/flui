//! Comparing the concrete types behind two trait objects.

use std::any::Any;

/// Whether `a` and `b` are values of one concrete type.
///
/// The parameters are `&dyn Any`, so a caller passes `&*rc` where `rc` is an
/// `Rc<dyn Trait>`: the upcast reads the delegate's type, where `&rc` would
/// read `Rc`'s.
pub(crate) fn same_concrete_type(a: &dyn Any, b: &dyn Any) -> bool {
    a.type_id() == b.type_id()
}
