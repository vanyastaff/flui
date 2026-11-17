//! What the crate's `[package.metadata.flui.declares]` says the application
//! opens, read as `#[flui::main]` compiles: the lists the run admits opened
//! URLs by, and the file types it registers where a host registers itself.
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
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Declared<T> {
    pub(super) always: Vec<T>,
    pub(super) by_feature: Vec<(String, Vec<T>)>,
}

impl<T> Default for Declared<T> {
    fn default() -> Self {
        Self {
            always: Vec::new(),
            by_feature: Vec::new(),
        }
    }
}

impl<T> Declared<T> {
    fn is_empty(&self) -> bool {
        self.always.is_empty() && self.by_feature.iter().all(|(_, list)| list.is_empty())
    }

    /// A constant expression of type `&[&[#ty]]`: the crate's list, then
    /// each feature's under `cfg!`, each entry written by `entry`.
    fn parts(&self, ty: &TokenStream, entry: impl Fn(&T) -> TokenStream) -> TokenStream {
        let always = self.always.iter().map(&entry);
        let features = self
            .by_feature
            .iter()
            .filter(|(_, list)| !list.is_empty())
            .map(|(feature, list)| {
                let list = list.iter().map(&entry);
                quote! {
                    {
                        const PART: &[#ty] = &[#(#list),*];
                        if ::core::cfg!(feature = #feature) { PART } else { &[] }
                    }
                }
            });
        quote! { &[&[#(#always),*], #(#features),*] }
    }
}

/// One `file-types` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FileType {
    pub(super) extension: String,
    pub(super) mime: String,
    pub(super) name: String,
}

/// The URL schemes, web domains and file types a manifest declares.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Opens {
    pub(super) url_schemes: Declared<String>,
    pub(super) associated_domains: Declared<String>,
    pub(super) file_types: Declared<FileType>,
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
            file_types: Declared {
                always: file_types(declares, "declares")?,
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
                let types = file_types(part, &name)?;
                opens
                    .url_schemes
                    .by_feature
                    .push((feature.to_owned(), schemes));
                opens
                    .associated_domains
                    .by_feature
                    .push((feature.to_owned(), domains));
                opens
                    .file_types
                    .by_feature
                    .push((feature.to_owned(), types));
            }
        }
        Ok(opens)
    }

    /// The statement the entry function starts with: the declarations
    /// recorded while it runs. Nothing when the manifest declares nothing,
    /// so an application that gives `Application::declares` itself keeps it
    /// the only source.
    pub(super) fn statement(&self, app: &syn::Path) -> syn::Result<Option<syn::Stmt>> {
        if self.url_schemes.is_empty()
            && self.associated_domains.is_empty()
            && self.file_types.is_empty()
        {
            return Ok(None);
        }
        let text = quote! { &'static str };
        let string = |value: &String| quote! { #value };
        let schemes = self.url_schemes.parts(&text, string);
        let domains = self.associated_domains.parts(&text, string);
        let file_type = quote! { #app::__entry::FileTypeDeclaration };
        let types = self.file_types.parts(&file_type, |declared| {
            let FileType {
                extension,
                mime,
                name,
            } = declared;
            quote! { #app::__entry::FileTypeDeclaration::new(#extension, #mime, #name) }
        });
        let declared = syn::Ident::new("__flui_declared", Span::mixed_site());
        syn::parse2(quote! {
            let #declared = {
                const URL_SCHEME_PARTS: &[&[&str]] = #schemes;
                const URL_SCHEMES: &[&str] = &#app::__entry::join::<
                    &str,
                    { #app::__entry::joined_len(URL_SCHEME_PARTS) },
                >(URL_SCHEME_PARTS, "");
                const DOMAIN_PARTS: &[&[&str]] = #domains;
                const DOMAINS: &[&str] = &#app::__entry::join::<
                    &str,
                    { #app::__entry::joined_len(DOMAIN_PARTS) },
                >(DOMAIN_PARTS, "");
                const FILE_TYPE_PARTS: &[&[#file_type]] = #types;
                const FILE_TYPES: &[#file_type] = &#app::__entry::join::<
                    #file_type,
                    { #app::__entry::joined_len(FILE_TYPE_PARTS) },
                >(FILE_TYPE_PARTS, #app::__entry::FileTypeDeclaration::new("", "", ""));
                #app::__entry::declare(
                    #app::__entry::OpenDeclarations::new()
                        .url_schemes(URL_SCHEMES)
                        .associated_domains(DOMAINS)
                        .file_types(FILE_TYPES),
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

/// The `<name>.file-types` entries under `table`: an array of tables, each
/// with the strings `extension`, `mime` and `name`. `flui build` checks the
/// values themselves.
fn file_types(table: &dyn TableLike, name: &str) -> Result<Vec<FileType>, String> {
    let key = "file-types";
    let Some(item) = table.get(key) else {
        return Ok(Vec::new());
    };
    let not_tables = || format!("`{name}.{key}` is not an array of tables");
    let entries: Vec<&dyn TableLike> = if let Some(tables) = item.as_array_of_tables() {
        tables.iter().map(|table| table as &dyn TableLike).collect()
    } else if let Some(array) = item.as_array() {
        array
            .iter()
            .map(|value| {
                value
                    .as_inline_table()
                    .map(|table| table as &dyn TableLike)
                    .ok_or_else(not_tables)
            })
            .collect::<Result<Vec<&dyn TableLike>, String>>()?
    } else {
        return Err(not_tables());
    };
    entries
        .into_iter()
        .map(|entry| {
            let field = |field: &str| {
                entry
                    .get(field)
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
                    .ok_or_else(|| format!("each `{name}.{key}` entry needs a string `{field}`"))
            };
            Ok(FileType {
                extension: field("extension")?,
                mime: field("mime")?,
                name: field("name")?,
            })
        })
        .collect::<Result<Vec<FileType>, String>>()
}

#[cfg(test)]
mod tests {
    use super::{Declared, FileType, Opens};

    fn note() -> FileType {
        FileType {
            extension: "note".to_owned(),
            mime: "application/x-notes-note".to_owned(),
            name: "Note".to_owned(),
        }
    }

    #[test]
    fn the_entry_carries_what_the_manifest_declares_it_opens() {
        let cases: [(&str, &str, Result<Opens, &str>); 11] = [
            (
                "no table",
                "[package]\nname = \"notes\"\n",
                Ok(Opens::default()),
            ),
            (
                "the crate's lists and its features'",
                "[package.metadata.flui.declares]\nschema = 3\nurl-schemes = [\"notes\"]\n\
                 associated-domains = [\"notes.example\"]\n\
                 [[package.metadata.flui.declares.file-types]]\nextension = \"note\"\n\
                 mime = \"application/x-notes-note\"\nname = \"Note\"\n\
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
                    file_types: Declared {
                        always: vec![note()],
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
                 declares = { features = { links = { associated-domains = [\"a.example\"], \
                 file-types = [{ extension = \"note\", mime = \"application/x-notes-note\", \
                 name = \"Note\" }] } } }\n",
                Ok(Opens {
                    url_schemes: Declared {
                        always: Vec::new(),
                        by_feature: vec![("links".to_owned(), Vec::new())],
                    },
                    associated_domains: Declared {
                        always: Vec::new(),
                        by_feature: vec![("links".to_owned(), vec!["a.example".to_owned()])],
                    },
                    file_types: Declared {
                        always: Vec::new(),
                        by_feature: vec![("links".to_owned(), vec![note()])],
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
            (
                "file types that are strings",
                "[package.metadata.flui.declares]\nfile-types = [\"note\"]\n",
                Err("`declares.file-types` is not an array of tables"),
            ),
            (
                "file types that are a table",
                "[package.metadata.flui.declares.file-types]\nextension = \"note\"\n",
                Err("`declares.file-types` is not an array of tables"),
            ),
            (
                "a file type without its name",
                "[[package.metadata.flui.declares.file-types]]\nextension = \"note\"\n\
                 mime = \"application/x-notes-note\"\n",
                Err("each `declares.file-types` entry needs a string `name`"),
            ),
            (
                "a feature's file type whose extension is not a string",
                "[[package.metadata.flui.declares.features.links.file-types]]\nextension = 3\n\
                 mime = \"application/x-notes-note\"\nname = \"Note\"\n",
                Err("each `declares.features.links.file-types` entry needs a string `extension`"),
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
