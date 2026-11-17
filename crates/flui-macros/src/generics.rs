//! Whether a field's type depends on the deriving type's generic parameters.
//!
//! A derive that adds a `where` bound per generic field, or that refuses a
//! field whose layout depends on a parameter, asks the same question of the
//! field's syntax; neither compares type spellings.

use syn::visit::Visit;
use syn::{Ident, Type};

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
            && path
                .segments
                .first()
                .is_some_and(|first| self.params.iter().any(|param| first.ident == **param))
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
        let cases: [(Type, bool); 9] = [
            (parse_quote!(T), true),
            (parse_quote!(Vec<T>), true),
            (parse_quote!(T::Item), true),
            (parse_quote!(Option<&'static [T]>), true),
            (parse_quote!([f64; N]), true),
            (parse_quote!(fn(T) -> u8), true),
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
