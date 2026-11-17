//! The derives, one module each.
//!
//! Each reads its input into a typed description, refusing what it could not
//! act on, before it looks up the runtime crate its expansion names. How a
//! helper attribute is recognised, and refused where no derive reads it, is
//! here ([`attribute`]); the record of several refusals of one input
//! (`crate::refusals`) and the questions asked of an item's generics
//! (`crate::generics`) are shared with the entry attribute.

mod attribute;

pub(crate) mod inherited_data;
pub(crate) mod inspect;
pub(crate) mod routable;
pub(crate) mod two_way_converter;
pub(crate) mod view;
