//! A derive's helper attributes, as their author may spell them.
//!
//! rustc resolves `#[r#view(...)]` to the helper `view` and accepts `#[view()]`
//! as one, so a derive that compared the identifier as written, or read a list
//! without counting its entries, would pass both by every check and act on
//! neither.

use syn::ext::IdentExt;
use syn::meta::ParseNestedMeta;
use syn::{Attribute, Path};

/// Whether `path` is the single name `name`, written with or without `r#`.
pub(crate) fn is_named(path: &Path, name: &str) -> bool {
    path.get_ident().is_some_and(|ident| ident.unraw() == name)
}

/// The attributes of `attrs` that are the helper `name`.
pub(crate) fn helpers<'a>(
    attrs: &'a [Attribute],
    name: &'a str,
) -> impl Iterator<Item = &'a Attribute> {
    attrs.iter().filter(move |attr| is_named(attr.path(), name))
}

/// Read each entry of the list `attr` carries with `entry`.
///
/// A list without an entry names nothing the derive could act on: it is
/// refused with `expected`, the entries the list takes.
pub(crate) fn parse_entries(
    attr: &Attribute,
    expected: &str,
    mut entry: impl FnMut(ParseNestedMeta<'_>) -> syn::Result<()>,
) -> syn::Result<()> {
    let mut any = false;
    attr.parse_nested_meta(|meta| {
        any = true;
        entry(meta)
    })?;
    if any {
        return Ok(());
    }
    Err(syn::Error::new_spanned(
        attr,
        format!("the list is empty; expected {expected}"),
    ))
}
