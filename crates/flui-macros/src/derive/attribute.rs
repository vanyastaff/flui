//! A derive's helper attributes, as their author may spell and place them.
//!
//! rustc resolves `#[r#view(...)]` to the helper `view`, accepts `#[view()]`
//! as one, and accepts a helper anywhere inside the item it derives for. A
//! derive that compared the identifier as written, read a list without
//! counting its entries, or looked only where it reads would pass each of
//! those by every check and act on none.

use proc_macro2::TokenTree;
use syn::ext::IdentExt;
use syn::meta::ParseNestedMeta;
use syn::visit::Visit;
use syn::{Attribute, DeriveInput, Field, Path, Token, Variant};

use crate::refusals::Refusals;

/// Whether `path` is the single name `name`, written with or without `r#`.
pub(crate) fn is_named(path: &Path, name: &str) -> bool {
    path.get_ident().is_some_and(|ident| ident.unraw() == name)
}

/// The attributes of `attrs` that are the helper `name`.
pub(crate) fn named<'a>(
    attrs: &'a [Attribute],
    name: &'a str,
) -> impl Iterator<Item = &'a Attribute> {
    attrs.iter().filter(move |attr| is_named(attr.path(), name))
}

/// Read each entry of the list `attr` carries with `entry`, recording every
/// one it refuses.
///
/// A refused entry does not end its list: the reading goes on after the next
/// comma, so one build names each of them. That covers an entry `entry`
/// refuses and one followed by tokens it did not read (`skip = 1`). A list
/// without an entry names nothing the derive could act on and is refused
/// with `expected`, what the author writes instead. Only a list that is not
/// one ends early: `#[inspect = "x"]`, or a literal where an entry's name goes.
pub(crate) fn parse_entries(
    attr: &Attribute,
    expected: &str,
    refusals: &mut Refusals,
    mut entry: impl FnMut(ParseNestedMeta<'_>, &mut Refusals) -> syn::Result<()>,
) {
    let mut any = false;
    let read = attr.parse_nested_meta(|meta| {
        any = true;
        let rest = meta.input;
        let ends = || rest.is_empty() || rest.peek(Token![,]);
        let read = entry(meta, refusals).and_then(|()| {
            if ends() {
                Ok(())
            } else {
                Err(rest.error("expected `,`"))
            }
        });
        if let Err(refused) = read {
            refusals.push(refused);
            while !ends() {
                rest.parse::<TokenTree>()?;
            }
        }
        Ok(())
    });
    match read {
        Err(syntax) => refusals.push(syntax),
        Ok(()) if any => {}
        Ok(()) => refusals.push(syn::Error::new_spanned(
            attr,
            format!("the list is empty; expected {expected}"),
        )),
    }
}

/// Refuse each helper of `names` written where no derive reads one.
///
/// A derive reads the attributes of the type, of its variants and of their
/// fields, and refuses there what it cannot act on. This refuses the rest of
/// the item: a generic parameter (whose attributes the generated `impl`
/// repeats, outside the helper's scope), a parameter of a function pointer in
/// a field's type, an expression or an item inside a discriminant.
pub(crate) fn refuse_unread(input: &DeriveInput, names: &[&str], refusals: &mut Refusals) {
    Unread { names, refusals }.visit_derive_input(input);
}

struct Unread<'a> {
    names: &'a [&'a str],
    refusals: &'a mut Refusals,
}

impl<'ast> Visit<'ast> for Unread<'_> {
    // The three places a derive reads are stepped over: only what is under
    // them is visited.
    fn visit_derive_input(&mut self, input: &'ast DeriveInput) {
        self.visit_generics(&input.generics);
        self.visit_data(&input.data);
    }

    fn visit_variant(&mut self, variant: &'ast Variant) {
        self.visit_fields(&variant.fields);
        if let Some((_, discriminant)) = &variant.discriminant {
            self.visit_expr(discriminant);
        }
    }

    fn visit_field(&mut self, field: &'ast Field) {
        self.visit_type(&field.ty);
    }

    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        if let Some(name) = self.names.iter().find(|name| is_named(attr.path(), name)) {
            self.refusals.push(syn::Error::new_spanned(
                attr,
                format!(
                    "`#[{name}]` does nothing here: the derive reads the attributes of the \
                     type, of its variants and of their fields"
                ),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;
    use crate::refusals::assert_refused;

    #[test]
    fn a_helper_outside_the_type_its_variants_and_their_fields_is_refused() {
        let unread = |input: DeriveInput| {
            Refusals::collect(|refusals| {
                refuse_unread(&input, &["view", "inspect"], refusals);
                Ok(())
            })
        };
        // Each place a derive reads, and other attributes anywhere, pass.
        let read: DeriveInput = parse_quote! {
            #[view(inspect)]
            enum E<#[cfg(all())] T> {
                #[inspect(skip)]
                A { #[view(site)] a: fn(#[allow(unused)] T) } = { #[allow(unused)] 1 },
            }
        };
        assert!(unread(read).is_ok());

        let cases: [(&str, DeriveInput); 6] = [
            (
                "a type parameter",
                parse_quote! { struct S<#[view(site)] T>(T); },
            ),
            (
                "a lifetime parameter",
                parse_quote! { struct S<#[inspect(skip)] 'a>(&'a u8); },
            ),
            (
                "a const parameter, spelled raw",
                parse_quote! { struct S<#[r#view(site)] const N: usize>([u8; N]); },
            ),
            (
                "a parameter of a function pointer",
                parse_quote! { struct S { call: fn(#[inspect(name = "x")] u32) } },
            ),
            (
                "an item inside a discriminant",
                parse_quote! { enum E { A = { #[view(site)] const X: isize = 1; X } } },
            ),
            (
                "an expression inside a discriminant",
                parse_quote! { enum E { A = #[inspect(skip)] 1 } },
            ),
        ];
        for (case, input) in cases {
            let result = unread(input);
            let Err(refusal) = &result else {
                panic!("BUG: `{case}` passes");
            };
            assert!(
                refusal.to_string().contains("does nothing here"),
                "{case}: {refusal}"
            );
            assert_refused(result, &["does nothing here"]);
        }
    }
}
