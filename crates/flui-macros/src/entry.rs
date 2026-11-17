//! `#[flui::main]`: the function an application starts from, on every
//! platform, carrying what its crate's manifest declares the application
//! opens.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::ItemFn;

use crate::generics;
use crate::refusals::Refusals;
use crate::runtime_path::Runtime;

mod declares;

/// Expand `#[main]` on `item`, with `args` the attribute's own tokens.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(
            &args,
            "`#[flui::main]` takes no arguments; configure the run with `Application`",
        ));
    }
    let function: ItemFn = syn::parse2(item).map_err(|error| {
        syn::Error::new(
            error.span(),
            "`#[flui::main]` marks a function: `pub fn run() -> Result<(), flui::LaunchError>`",
        )
    })?;
    check_signature(&function)?;
    let app = Runtime::App.resolve(function.sig.ident.span())?;
    let name = function.sig.ident.clone();
    let manifest =
        declares::manifest_path().map_err(|error| syn::Error::new(Span::call_site(), error))?;
    let text = std::fs::read_to_string(&manifest).map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!("`#[flui::main]` reads {}: {error}", manifest.display()),
        )
    })?;
    let opens = declares::Opens::parse(&text).map_err(|error| {
        syn::Error::new(
            Span::call_site(),
            format!(
                "`#[flui::main]` reads what the application opens from {}: {error}",
                manifest.display()
            ),
        )
    })?;
    let declared = opens.statement(&app)?;
    let manifest = manifest.to_string_lossy().into_owned();
    let mut function = function;
    if let Some(declared) = declared {
        function.block.stmts.insert(0, declared);
    }
    Ok(quote! {
        #function

        // The manifest the declarations come from: naming it makes Cargo
        // compile the crate again when it changes.
        const _: &[u8] = ::core::include_bytes!(#manifest);

        // What the function returns is one an entry may: checked on every
        // target, so a signature Android would refuse fails on the desktop too.
        const _: fn() = || #app::__entry::check(#name);

        /// The entry Android's `NativeActivity` calls once it has loaded this
        /// crate as a `cdylib`: it hands the run the system's `AndroidApp`.
        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        fn android_main(app: #app::__entry::AndroidApp) {
            #app::__entry::android(app, #name);
        }
    })
}

/// The entry function takes nothing and is a plain, synchronous `fn`. Each
/// part of the signature that says otherwise is refused, all in one build.
fn check_signature(function: &ItemFn) -> syn::Result<()> {
    let sig = &function.sig;
    let wrong = |at: &dyn ToTokens, what: &str| {
        syn::Error::new_spanned(
            at,
            format!(
                "`#[flui::main]` function {what}: write `pub fn run() -> Result<(), flui::LaunchError>`"
            ),
        )
    };
    Refusals::collect(|refusals| {
        if let Some(asyncness) = &sig.asyncness {
            refusals.push(wrong(
                asyncness,
                "cannot be `async`; the run drives its own loop",
            ));
        }
        if let syn::Safety::Unsafe(unsafety) = &sig.safety {
            refusals.push(wrong(unsafety, "cannot be `unsafe`"));
        }
        if let Some(abi) = &sig.abi {
            refusals.push(wrong(abi, "cannot name an ABI"));
        }
        if let Some(generic) = generics::declared(&sig.generics) {
            refusals.push(wrong(generic, "cannot be generic"));
        }
        if !sig.inputs.is_empty() {
            refusals.push(wrong(&sig.inputs, "takes no arguments"));
        } else if let Some(variadic) = &sig.variadic {
            refusals.push(wrong(variadic, "takes no arguments"));
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::*;
    use crate::refusals::assert_refused;

    #[test]
    fn every_refusal_of_an_entrys_signature_is_reported_in_one_build() {
        let plain: ItemFn = parse_quote! {
            pub fn run() -> Result<(), flui::LaunchError> { Ok(()) }
        };
        assert!(check_signature(&plain).is_ok());

        let function: ItemFn = parse_quote! {
            async unsafe extern "C" fn run<T>(argc: u8) where T: Copy {}
        };
        assert_refused(
            check_signature(&function),
            &[
                "cannot be `async`",
                "cannot be `unsafe`",
                "cannot name an ABI",
                "cannot be generic",
                "takes no arguments",
            ],
        );
        // A clause without a predicate still makes the function generic.
        let function: ItemFn = parse_quote! { fn run() where {} };
        assert_refused(check_signature(&function), &["cannot be generic"]);
    }
}
