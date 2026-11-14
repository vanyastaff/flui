//! What the crate's `[package.metadata.flui.declares]` says the application
//! opens, read as `#[flui::main]` compiles: the lists the run admits opened
//! URLs by.
//!
//! `flui build` validates the same table strictly (ADR-0307) and registers
//! it with the operating system; the attribute only carries the lists, so the
//! URLs a run opens are the ones the manifest registers.

use std::path::{Path, PathBuf};

use proc_macro2::{Span, TokenStream};
use quote::quote;
use toml_edit::{Document, TableLike};

/// One list the manifest declares: the crate's own entries, then each
/// feature's, which count when the crate is compiled with that feature.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Declared {
    pub(super) always: Vec<String>,
    pub(super) by_feature: Vec<(String, Vec<String>)>,
}

impl Declared {
    fn is_empty(&self) -> bool {
        self.always.is_empty() && self.by_feature.iter().all(|(_, list)| list.is_empty())
    }

    /// A constant expression of type `&[&[&str]]`: the crate's list, then
    /// each feature's under `cfg!`.
    fn parts(&self) -> TokenStream {
        let always = &self.always;
        let features = self
            .by_feature
            .iter()
            .filter(|(_, list)| !list.is_empty())
            .map(|(feature, list)| {
                quote! {
                    {
                        const PART: &[&str] = &[#(#list),*];
                        if cfg!(feature = #feature) { PART } else { &[] }
                    }
                }
            });
        quote! { &[&[#(#always),*], #(#features),*] }
    }
}

/// The URL schemes and web domains a manifest declares.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Opens {
    pub(super) url_schemes: Declared,
    pub(super) associated_domains: Declared,
}

impl Opens {
    /// What the manifest text `manifest` declares.
    pub(super) fn parse(manifest: &str) -> Result<Self, String> {
        let document = Document::parse(manifest).map_err(|error| error.to_string())?;
        let Some(declares) = table_at(
            document.as_table(),
            "package.metadata.flui.declares",
            &["package", "metadata", "flui", "declares"],
        )?
        else {
            return Ok(Self::default());
        };
        let mut opens = Self {
            url_schemes: Declared {
                always: strings(declares, "declares", "url-schemes")?,
                by_feature: Vec::new(),
            },
            associated_domains: Declared {
                always: strings(declares, "declares", "associated-domains")?,
                by_feature: Vec::new(),
            },
        };
        if let Some(features) = table_at(declares, "declares.features", &["features"])? {
            for (feature, part) in features.iter() {
                let name = format!("declares.features.{feature}");
                let part = part
                    .as_table_like()
                    .ok_or_else(|| format!("`{name}` is not a table"))?;
                let schemes = strings(part, &name, "url-schemes")?;
                let domains = strings(part, &name, "associated-domains")?;
                opens
                    .url_schemes
                    .by_feature
                    .push((feature.to_owned(), schemes));
                opens
                    .associated_domains
                    .by_feature
                    .push((feature.to_owned(), domains));
            }
        }
        Ok(opens)
    }

    /// The statement the entry function starts with: the declarations
    /// recorded while it runs. Nothing when the manifest declares nothing,
    /// so an application that gives `Application::declares` itself keeps it
    /// the only source.
    pub(super) fn statement(&self, app: &syn::Path) -> syn::Result<Option<syn::Stmt>> {
        if self.url_schemes.is_empty() && self.associated_domains.is_empty() {
            return Ok(None);
        }
        let schemes = self.url_schemes.parts();
        let domains = self.associated_domains.parts();
        let declared = syn::Ident::new("__flui_declared", Span::mixed_site());
        syn::parse2(quote! {
            let #declared = {
                const URL_SCHEME_PARTS: &[&[&str]] = #schemes;
                const URL_SCHEMES: &[&str] = &#app::__entry::join::<
                    { #app::__entry::joined_len(URL_SCHEME_PARTS) },
                >(URL_SCHEME_PARTS);
                const DOMAIN_PARTS: &[&[&str]] = #domains;
                const DOMAINS: &[&str] = &#app::__entry::join::<
                    { #app::__entry::joined_len(DOMAIN_PARTS) },
                >(DOMAIN_PARTS);
                #app::__entry::declare(
                    #app::__entry::LinkDeclarations::new()
                        .url_schemes(URL_SCHEMES)
                        .associated_domains(DOMAINS),
                )
            };
        })
        .map(Some)
    }
}

/// The manifest of the crate being compiled.
pub(super) fn manifest_path() -> Result<PathBuf, String> {
    let dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| "CARGO_MANIFEST_DIR is not set; build with Cargo".to_owned())?;
    Ok(Path::new(&dir).join("Cargo.toml"))
}

/// The table at `path` under `table`, named `name`, if there is one.
fn table_at<'a>(
    table: &'a dyn TableLike,
    name: &str,
    path: &[&str],
) -> Result<Option<&'a dyn TableLike>, String> {
    let mut at = table;
    for key in path {
        match at.get(key) {
            None => return Ok(None),
            Some(item) => {
                at = item
                    .as_table_like()
                    .ok_or_else(|| format!("`{name}` is not a table"))?;
            }
        }
    }
    Ok(Some(at))
}

/// The strings `<name>.<key>` lists under `table`.
fn strings(table: &dyn TableLike, name: &str, key: &str) -> Result<Vec<String>, String> {
    let Some(item) = table.get(key) else {
        return Ok(Vec::new());
    };
    let array = item
        .as_array()
        .ok_or_else(|| format!("`{name}.{key}` is not an array of strings"))?;
    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("`{name}.{key}` holds a value that is not a string"))
        })
        .collect::<Result<Vec<String>, String>>()
}

#[cfg(test)]
mod tests {
    use super::{Declared, Opens};

    #[test]
    fn the_entry_carries_what_the_manifest_declares_it_opens() {
        let cases: [(&str, &str, Result<Opens, &str>); 7] = [
            (
                "no table",
                "[package]\nname = \"notes\"\n",
                Ok(Opens::default()),
            ),
            (
                "the crate's lists and its features'",
                "[package.metadata.flui.declares]\nschema = 3\nurl-schemes = [\"notes\"]\n\
                 associated-domains = [\"notes.example\"]\n\
                 [package.metadata.flui.declares.features.share.android]\npermissions = []\n\
                 [package.metadata.flui.declares.features.links]\nurl-schemes = [\"notes-beta\"]\n",
                Ok(Opens {
                    url_schemes: Declared {
                        always: vec!["notes".to_owned()],
                        by_feature: vec![
                            ("share".to_owned(), Vec::new()),
                            ("links".to_owned(), vec!["notes-beta".to_owned()]),
                        ],
                    },
                    associated_domains: Declared {
                        always: vec!["notes.example".to_owned()],
                        by_feature: vec![
                            ("share".to_owned(), Vec::new()),
                            ("links".to_owned(), Vec::new()),
                        ],
                    },
                }),
            ),
            (
                "inline tables",
                "[package.metadata.flui]\n\
                 declares = { features = { links = { associated-domains = [\"a.example\"] } } }\n",
                Ok(Opens {
                    url_schemes: Declared {
                        always: Vec::new(),
                        by_feature: vec![("links".to_owned(), Vec::new())],
                    },
                    associated_domains: Declared {
                        always: Vec::new(),
                        by_feature: vec![("links".to_owned(), vec!["a.example".to_owned()])],
                    },
                }),
            ),
            (
                "a scheme that is not a string",
                "[package.metadata.flui.declares]\nurl-schemes = [3]\n",
                Err("`declares.url-schemes` holds a value that is not a string"),
            ),
            (
                "a list that is a string",
                "[package.metadata.flui.declares]\nassociated-domains = \"notes.example\"\n",
                Err("`declares.associated-domains` is not an array of strings"),
            ),
            (
                "declares that is not a table",
                "[package.metadata.flui]\ndeclares = 3\n",
                Err("`package.metadata.flui.declares` is not a table"),
            ),
            (
                "a feature's list that is not one",
                "[package.metadata.flui.declares.features.links]\nurl-schemes = \"x\"\n",
                Err("`declares.features.links.url-schemes` is not an array of strings"),
            ),
        ];
        for (case, manifest, expected) in cases {
            let parsed = Opens::parse(manifest);
            match expected {
                Ok(expected) => assert_eq!(parsed, Ok(expected), "{case}"),
                Err(message) => assert_eq!(parsed, Err(message.to_owned()), "{case}"),
            }
        }
    }
}
