//! What a macro asks of an item's generics.
//!
//! A derive that adds a `where` bound per generic field, or that refuses a
//! field whose layout depends on a parameter, asks whether the field's type
//! names a parameter; a macro that takes only a concrete item asks where the
//! item says it is generic. Both read the syntax, with a parameter spelled
//! `T` or `r#T`; neither compares type spellings.

use quote::ToTokens;
use syn::ext::IdentExt;
use syn::visit::Visit;
use syn::{Generics, Ident, Type};

/// The tokens that make an item generic, for a refusal to underline: its
/// parameter list, or the `where` keyword of an item that has a clause and no
/// parameter. `None` for an item with neither.
///
/// The keyword stands for the clause because a clause may have no predicate
/// (`fn run() where {}`), and then prints as nothing a span could cover.
pub(crate) fn declared(generics: &Generics) -> Option<&dyn ToTokens> {
    if !generics.params.is_empty() {
        return Some(generics);
    }
    generics
        .where_clause
        .as_ref()
        .map(|clause| &clause.where_token as &dyn ToTokens)
}

/// Whether `ty` names one of `params`: a path that starts with the parameter,
/// which also covers its associated types (`T::Item`) and a const parameter
/// used as an expression (`[f64; N]`).
pub(crate) fn mentions_param(ty: &Type, params: &[&Ident]) -> bool {
    let mut visitor = MentionsParam {
        params,
        found: false,
    };
    visitor.visit_type(ty);
    visitor.found
}

struct MentionsParam<'a> {
    params: &'a [&'a Ident],
    found: bool,
}

impl<'ast> Visit<'ast> for MentionsParam<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        // `::T` is a crate, never a parameter.
        if path.leading_colon.is_none()
            && path.segments.first().is_some_and(|first| {
                let first = first.ident.unraw();
                self.params.iter().any(|param| first == param.unraw())
            })
        {
            self.found = true;
        }
        syn::visit::visit_path(self, path);
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;

    #[test]
    fn a_type_mentions_a_parameter_wherever_its_path_starts_with_one() {
        let t: Ident = parse_quote!(T);
        let n: Ident = parse_quote!(N);
        let params = [&t, &n];
        let cases: [(Type, bool); 11] = [
            (parse_quote!(T), true),
            (parse_quote!(Vec<T>), true),
            (parse_quote!(T::Item), true),
            (parse_quote!(Option<&'static [T]>), true),
            (parse_quote!([f64; N]), true),
            (parse_quote!(fn(T) -> u8), true),
            (parse_quote!(r#T), true),
            (parse_quote!([f64; r#N]), true),
            (parse_quote!(f64), false),
            (parse_quote!(::T::Thing), false),
            (parse_quote!(module::T), false),
        ];
        for (ty, expected) in cases {
            assert_eq!(
                mentions_param(&ty, &params),
                expected,
                "{}",
                quote::quote!(#ty)
            );
        }
    }
}
