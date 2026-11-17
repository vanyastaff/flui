//! Resolve generated runtime paths from the consuming Cargo manifest.

use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use syn::Path;

/// The runtime module that owns what a macro's expansion names.
#[derive(Clone, Copy)]
pub(crate) enum Runtime {
    View,
    Foundation,
    Animation,
    Widgets,
    App,
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
    pub(crate) fn resolve(self, span: Span) -> syn::Result<Path> {
        let (package, module) = match self {
            Self::View => ("flui-view", "view"),
            Self::Foundation => ("flui-foundation", "foundation"),
            Self::Animation => ("flui-animation", "animation"),
            Self::Widgets => ("flui-widgets", "widgets"),
            Self::App => ("flui-app", "app"),
        };
        // A package builds on `flui-sdk` and has no entry function, so the
        // application runtime is never looked up there.
        if !matches!(self, Self::App)
            && let Ok(found) = crate_name("flui-sdk")
        {
            return module_path(found, "flui-sdk", module, span);
        }
        if let Ok(found) = crate_name(package) {
            return absolute_path(found, package, span);
        }
        if let Ok(found) = crate_name("flui") {
            return module_path(found, "flui", module, span);
        }
        // The application runtime is not in the SDK, so a crate that already
        // has `flui-sdk` is not told to add it.
        let facade = if matches!(self, Self::App) {
            "add `flui` to Cargo.toml (`flui-sdk` has no application entry)"
        } else {
            "add `flui` to Cargo.toml (a package adds `flui-sdk`)"
        };
        Err(syn::Error::new(
            span,
            format!(
                "FLUI macros expand to paths in a runtime crate; {facade}, or use a direct `{package}` dependency (renamed dependencies are supported)"
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

#[cfg(test)]
mod tests {
    use super::*;

    /// This crate's own manifest names no runtime crate, so every lookup
    /// from its unit tests ends in the error a consumer without one sees.
    #[test]
    fn a_crate_without_the_runtime_is_told_which_dependency_to_add() {
        let advice = |runtime: Runtime| match runtime.resolve(Span::call_site()) {
            Ok(_) => panic!("BUG: flui-macros resolves a runtime crate"),
            Err(error) => error.to_string(),
        };
        let view = advice(Runtime::View);
        assert!(view.contains("a package adds `flui-sdk`"), "{view}");
        assert!(view.contains("a direct `flui-view` dependency"), "{view}");

        let app = advice(Runtime::App);
        assert!(app.contains("`flui-sdk` has no application entry"), "{app}");
        assert!(!app.contains("a package adds `flui-sdk`"), "{app}");
        assert!(app.contains("a direct `flui-app` dependency"), "{app}");
    }
}
