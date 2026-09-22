//! Resolve generated runtime paths from the consuming Cargo manifest.

use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use syn::Path;

/// Runtime module that owns the trait implemented by a derive.
pub(crate) enum Runtime {
    View,
    Foundation,
    Animation,
}

impl Runtime {
    pub(crate) fn resolve(&self, span: Span) -> syn::Result<Path> {
        let (package, module) = match self {
            Self::View => ("flui-view", "view"),
            Self::Foundation => ("flui-foundation", "foundation"),
            Self::Animation => ("flui-animation", "animation"),
        };
        if let Ok(found) = crate_name(package) {
            return absolute_path(found, package, span);
        }
        if let Ok(found) = crate_name("flui") {
            let mut path = absolute_path(found, "flui", span)?;
            path.segments
                .push(syn::PathSegment::from(syn::Ident::new(module, span)));
            return Ok(path);
        }
        Err(syn::Error::new(
            span,
            format!(
                "FLUI derive requires a runtime dependency; add `flui` to Cargo.toml, or use a direct `{package}` dependency (renamed dependencies are supported)"
            ),
        ))
    }
}

fn absolute_path(found: FoundCrate, package: &str, span: Span) -> syn::Result<Path> {
    // `Itself` also occurs in a package's integration tests. An absolute crate
    // name works there and in its library via the owner's private self alias.
    let name = match found {
        FoundCrate::Itself => package.replace('-', "_"),
        FoundCrate::Name(name) => name,
    };
    syn::parse_str(&format!("::{name}"))
        .or_else(|_| syn::parse_str(&format!("::r#{name}")))
        .map_err(|_| {
            syn::Error::new(
                span,
                format!("FLUI dependency alias `{name}` is not a Rust crate identifier"),
            )
        })
}
