//! `#[flui::main]`: the function an application starts from, on every
//! platform.

use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemFn;
use syn::spanned::Spanned;

use crate::runtime_path::Runtime;

/// Expand `#[main]` on `item`, with `args` the attribute's own tokens.
pub(crate) fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new(
            args.span(),
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
    let name = &function.sig.ident;
    Ok(quote! {
        #function

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

/// The entry function takes nothing and is a plain, synchronous `fn`.
fn check_signature(function: &ItemFn) -> syn::Result<()> {
    let sig = &function.sig;
    let wrong = |span: proc_macro2::Span, what: &str| {
        Err(syn::Error::new(
            span,
            format!(
                "`#[flui::main]` function {what}: write `pub fn run() -> Result<(), flui::LaunchError>`"
            ),
        ))
    };
    if let Some(asyncness) = &sig.asyncness {
        return wrong(
            asyncness.span(),
            "cannot be `async`; the run drives its own loop",
        );
    }
    if let syn::Safety::Unsafe(unsafety) = &sig.safety {
        return wrong(unsafety.span(), "cannot be `unsafe`");
    }
    if let Some(abi) = &sig.abi {
        return wrong(abi.span(), "cannot name an ABI");
    }
    if !sig.generics.params.is_empty() || sig.generics.where_clause.is_some() {
        return wrong(sig.generics.span(), "cannot be generic");
    }
    if !sig.inputs.is_empty() {
        return wrong(sig.inputs.span(), "takes no arguments");
    }
    if let Some(variadic) = &sig.variadic {
        return wrong(variadic.span(), "takes no arguments");
    }
    Ok(())
}
