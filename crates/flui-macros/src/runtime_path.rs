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
    /// The path of the runtime module, looked up in the consumer's manifest in
    /// this order: `flui-sdk` (a package), the owning crate (a framework
    /// crate), then the `flui` facade (an application).
    ///
    /// The SDK comes first because a package may also carry the facade or an
    /// internal crate as a dev-dependency, and `proc-macro-crate` cannot tell
    /// a dev-dependency from a normal one: an owner-first lookup would expand
    /// to a crate the library build does not have. The converse case, an
    /// owner as a normal dependency beside `flui-sdk` as a dev-dependency
    /// only, is the one this order cannot serve; no crate has that shape.
    pub(crate) fn resolve(&self, span: Span) -> syn::Result<Path> {
        let (package, module) = match self {
            Self::View => ("flui-view", "view"),
            Self::Foundation => ("flui-foundation", "foundation"),
            Self::Animation => ("flui-animation", "animation"),
        };
        if let Ok(found) = crate_name("flui-sdk") {
            return module_path(found, "flui-sdk", module, span);
        }
        if let Ok(found) = crate_name(package) {
            return absolute_path(found, package, span);
        }
        if let Ok(found) = crate_name("flui") {
            return module_path(found, "flui", module, span);
        }
        Err(syn::Error::new(
            span,
            format!(
                "FLUI derive requires a runtime dependency; add `flui` to Cargo.toml (a package adds `flui-sdk`), or use a direct `{package}` dependency (renamed dependencies are supported)"
            ),
        ))
    }
}

/// `::<crate>::<module>` for a crate that re-exports the runtime crates as
/// modules: the facade and the SDK.
fn module_path(found: FoundCrate, package: &str, module: &str, span: Span) -> syn::Result<Path> {
    let mut path = absolute_path(found, package, span)?;
    path.segments
        .push(syn::PathSegment::from(syn::Ident::new(module, span)));
    Ok(path)
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
