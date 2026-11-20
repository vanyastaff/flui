//! FLUI's W3C key tables, generated from the `keyboard-types` the workspace pins (ADR-0089 §4).
//!
//! `flui_platform::input::keyboard::{NamedKey, Code}` are FLUI's own enums, so no `keyboard-types`
//! type reaches a Stable signature, but their variants are the W3C `key` and `code` value lists,
//! which `keyboard-types` already transcribes from the specifications. This module reads the
//! pinned `keyboard-types` sources (`named_key.rs` and `code.rs`) and generates both enums with
//! their documentation, an `ALL` table and the W3C spelling of every variant.
//!
//! The test regenerates the files in memory and asserts each equals the committed file, so a
//! `keyboard-types` bump that adds, removes or renames a key fails until the tables are
//! regenerated with `FLUI_BLESS=1`. A variant upstream marks `#[deprecated]` is not generated:
//! the specification lists it as legacy, and the backend conversion maps its spelling to the
//! current key. Any other upstream attribute on a variant fails the run instead of being dropped
//! silently. The fixture tests run the generator over an embedded upstream-shaped file.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

/// A generator failure, worded for the person reading the test output.
type Failure = String;

/// One generated table: the upstream file it is read from and the file it is written to.
#[derive(Debug, Clone, Copy)]
struct Table {
    /// The enum's name, upstream and here.
    name: &'static str,
    /// The upstream file, relative to the `keyboard-types` package root.
    upstream: &'static str,
    /// The generated file, relative to the `flui-platform` package root.
    output: &'static str,
    /// The enum's own documentation.
    summary: &'static str,
    /// A variant whose W3C spelling is its name, for the generated example.
    example: &'static str,
    /// Upstream variants FLUI carries as a `Key` variant of its own, with a payload the W3C
    /// value has no room for; each must exist upstream, so a rename fails the run.
    carried_by_key: &'static [&'static str],
}

const TABLES: [Table; 2] = [
    Table {
        name: "NamedKey",
        upstream: "src/named_key.rs",
        output: "src/input/keyboard/named_key.rs",
        summary: "A key the W3C names: the `key` value of a key that produces no character.\n\n\
                  The variants are the [UI Events `key` values](https://w3c.github.io/uievents-key/).\n\
                  A value the specification adds before FLUI regenerates this table arrives as\n\
                  [`NamedKey::Unidentified`]. A dead key is not one of them: it is\n\
                  [`Key::Dead`](super::Key::Dead), which carries its accent.",
        example: "Enter",
        // `Key::Dead` carries the accent the dead key leaves pending.
        carried_by_key: &["Dead"],
    },
    Table {
        name: "Code",
        upstream: "src/code.rs",
        output: "src/input/keyboard/code.rs",
        summary: "The physical key, independent of the keyboard layout: the W3C `code` value.\n\n\
                  The variants are the [UI Events `code` values](https://w3c.github.io/uievents-code/).\n\
                  A value the specification adds before FLUI regenerates this table arrives as\n\
                  [`Code::Unidentified`].",
        example: "KeyA",
        carried_by_key: &[],
    },
];

/// The command that rewrites the generated files, named in their header and in a failure.
const BLESS_COMMAND: &str = "FLUI_BLESS=1 cargo nextest run -p flui-platform --test platform_api_it -E 'test(key_vocabulary)'";

/// Regenerates the tables in memory and compares them with the committed files; with
/// `FLUI_BLESS=1` it writes them instead.
#[test]
fn key_tables_match_the_pinned_keyboard_types() {
    let package_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let (upstream_root, version) = keyboard_types_root(&package_root);
    let bless = std::env::var("FLUI_BLESS").is_ok_and(|value| value == "1");
    let mut stale = Vec::new();
    for table in TABLES {
        let upstream = upstream_root.join(table.upstream);
        let source = std::fs::read_to_string(&upstream)
            .unwrap_or_else(|error| panic!("reading {}: {error}", upstream.display()));
        let generated = generate(table, &source, &version).unwrap_or_else(|failure| {
            panic!(
                "generating {} from {}: {failure}",
                table.name, table.upstream
            )
        });
        let output = package_root.join(table.output);
        if bless {
            std::fs::write(&output, &generated)
                .unwrap_or_else(|error| panic!("writing {}: {error}", output.display()));
        } else if std::fs::read_to_string(&output).ok().as_deref() != Some(generated.as_str()) {
            stale.push(table.output);
        }
    }
    assert!(
        stale.is_empty(),
        "{} not what keyboard-types {version} generates; run `{BLESS_COMMAND}`",
        stale.join(", ")
    );
}

/// The package root and version of `flui-platform`'s `keyboard-types` dev-dependency.
fn keyboard_types_root(package_root: &Path) -> (PathBuf, String) {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(package_root)
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .expect("`cargo metadata --locked` runs");
    let resolve = metadata
        .resolve
        .as_ref()
        .expect("`cargo metadata` returns a dependency graph");
    let node = resolve
        .nodes
        .iter()
        .find(|node| {
            metadata[&node.id].name.as_str() == "flui-platform"
                && metadata[&node.id].manifest_path.as_std_path() == package_root.join("Cargo.toml")
        })
        .expect("flui-platform is in the dependency graph");
    let dependency = node
        .deps
        .iter()
        .find(|dep| dep.name == "keyboard_types")
        .expect("flui-platform has its keyboard-types generator dev-dependency");
    let package = &metadata[&dependency.pkg];
    let manifest_dir = package
        .manifest_path
        .parent()
        .expect("keyboard-types' manifest path has a parent");
    (
        manifest_dir.as_std_path().to_owned(),
        package.version.to_string(),
    )
}

/// One upstream variant: its name, documentation lines, search aliases and W3C spelling.
#[derive(Debug)]
struct Variant {
    name: String,
    doc: Vec<String>,
    aliases: Vec<String>,
    spelling: String,
}

/// The generated file for `table`, from the upstream source text.
fn generate(table: Table, source: &str, version: &str) -> Result<String, Failure> {
    let file =
        syn::parse_file(source).map_err(|error| format!("parsing the upstream file: {error}"))?;
    let variants = variants(&file, table.name, table.carried_by_key)?;
    let mut out = String::new();
    write_file(&mut out, table, version, &variants).expect("writing to a String cannot fail");
    Ok(out)
}

fn variants(
    file: &syn::File,
    name: &str,
    carried_by_key: &[&str],
) -> Result<Vec<Variant>, Failure> {
    let item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(item) if item.ident == name => Some(item),
            _ => None,
        })
        .ok_or_else(|| format!("no `enum {name}` upstream"))?;
    let mut spellings = Spellings::default();
    for item in &file.items {
        if let syn::Item::Impl(item) = item
            && implements_display_for(item, name)
        {
            spellings.visit_item_impl(item);
        }
    }
    if spellings.arms.is_empty() {
        return Err(format!("no `impl Display for {name}` upstream"));
    }
    for carried in carried_by_key {
        if !item.variants.iter().any(|variant| variant.ident == carried) {
            return Err(format!(
                "{name}::{carried}, which `Key` carries, is gone upstream"
            ));
        }
    }
    let mut variants = Vec::new();
    'variants: for variant in &item.variants {
        if carried_by_key
            .iter()
            .any(|carried| variant.ident == carried)
        {
            continue;
        }
        if !matches!(variant.fields, syn::Fields::Unit) {
            return Err(format!("{name}::{} carries data", variant.ident));
        }
        let mut doc = Vec::new();
        let mut aliases = Vec::new();
        for attr in &variant.attrs {
            if attr.path().is_ident("deprecated") {
                continue 'variants;
            }
            if !attr.path().is_ident("doc") {
                return Err(format!(
                    "{name}::{} has an attribute the generator does not know: {}",
                    variant.ident,
                    attr.path()
                        .get_ident()
                        .map_or_else(|| "a path".to_owned(), ToString::to_string)
                ));
            }
            match &attr.meta {
                syn::Meta::NameValue(meta) => doc.push(string_literal(&meta.value)?),
                syn::Meta::List(_) => attr
                    .parse_nested_meta(|meta| {
                        if meta.path.is_ident("alias") {
                            let alias: syn::LitStr = meta.value()?.parse()?;
                            aliases.push(alias.value());
                            Ok(())
                        } else {
                            Err(meta.error("only `doc(alias)` is generated"))
                        }
                    })
                    .map_err(|error| format!("{name}::{}: {error}", variant.ident))?,
                syn::Meta::Path(_) => {
                    return Err(format!("{name}::{} has a bare `#[doc]`", variant.ident));
                }
            }
        }
        let ident = variant.ident.to_string();
        let spelling = spellings
            .arms
            .iter()
            .find(|(arm, _)| *arm == ident)
            .map(|(_, spelling)| spelling.clone())
            .ok_or_else(|| format!("{name}::{ident} has no Display spelling upstream"))?;
        variants.push(Variant {
            name: ident,
            doc,
            aliases,
            spelling,
        });
    }
    if !variants
        .iter()
        .any(|variant| variant.name == "Unidentified")
    {
        return Err(format!(
            "{name} has no `Unidentified` variant, the fallback for an unknown value"
        ));
    }
    Ok(variants)
}

fn implements_display_for(item: &syn::ItemImpl, name: &str) -> bool {
    let is_display = item
        .trait_
        .as_ref()
        .and_then(|(path, _)| path.segments.last())
        .is_some_and(|segment| segment.ident == "Display");
    let is_target = matches!(&*item.self_ty, syn::Type::Path(path)
        if path.path.segments.last().is_some_and(|segment| segment.ident == name));
    is_display && is_target
}

fn string_literal(expr: &syn::Expr) -> Result<String, Failure> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(literal),
            ..
        }) => Ok(literal.value()),
        _ => Err("a `#[doc = ...]` that is not a string literal".to_owned()),
    }
}

/// The `Variant => f.write_str("Spelling")` arms of an upstream `Display` impl.
#[derive(Debug, Default)]
struct Spellings {
    arms: Vec<(String, String)>,
}

impl<'ast> Visit<'ast> for Spellings {
    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        let variant = match &arm.pat {
            syn::Pat::Ident(pat) => Some(pat.ident.to_string()),
            syn::Pat::Path(pat) => pat.path.segments.last().map(|s| s.ident.to_string()),
            _ => None,
        };
        let spelling = match &*arm.body {
            syn::Expr::MethodCall(call) if call.method == "write_str" => {
                call.args.first().and_then(|arg| string_literal(arg).ok())
            }
            _ => None,
        };
        if let (Some(variant), Some(spelling)) = (variant, spelling) {
            self.arms.push((variant, spelling));
        }
        syn::visit::visit_arm(self, arm);
    }
}

fn write_file(
    out: &mut String,
    table: Table,
    version: &str,
    variants: &[Variant],
) -> std::fmt::Result {
    let name = table.name;
    writeln!(
        out,
        "// @generated by `{BLESS_COMMAND}` from keyboard-types {version}"
    )?;
    writeln!(
        out,
        "// ({}, MIT OR Apache-2.0). Edit the generator in tests/key_vocabulary.rs, not this file.",
        table.upstream
    )?;
    writeln!(out)?;
    writeln!(out, "//! [`{name}`], generated from the W3C value list.")?;
    writeln!(out)?;
    writeln!(
        out,
        "#![allow(\n    clippy::doc_markdown,\n    reason = \"the variant descriptions are the specification's prose\"\n)]"
    )?;
    writeln!(out)?;
    for line in table.summary.lines() {
        doc_line(out, "", line)?;
    }
    let example = table.example;
    for line in [
        String::new(),
        "# Examples".to_owned(),
        String::new(),
        "```".to_owned(),
        format!("use flui_platform::input::keyboard::{name};"),
        String::new(),
        format!("assert_eq!({name}::{example}.as_str(), \"{example}\");"),
        format!("assert_eq!({name}::from_w3c(\"{example}\"), Some({name}::{example}));"),
        format!("assert_eq!({name}::from_w3c(\"NoSuchValue\"), None);"),
        "```".to_owned(),
    ] {
        doc_line(out, "", &line)?;
    }
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]")?;
    writeln!(out, "#[non_exhaustive]")?;
    writeln!(out, "pub enum {name} {{")?;
    for variant in variants {
        if variant.doc.is_empty() {
            // Upstream leaves some values undocumented; the W3C spelling is their description.
            doc_line(
                out,
                "    ",
                &format!("The W3C `{}` value.", variant.spelling),
            )?;
        }
        for doc in &variant.doc {
            for line in doc.lines() {
                doc_line(out, "    ", line.trim_prefix(' '))?;
            }
        }
        for alias in &variant.aliases {
            writeln!(out, "    #[doc(alias = {alias:?})]")?;
        }
        writeln!(out, "    {},", variant.name)?;
    }
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl {name} {{")?;
    writeln!(
        out,
        "    /// Every variant, in the order the specification lists them."
    )?;
    writeln!(out, "    pub const ALL: &[Self] = &[")?;
    for variant in variants {
        writeln!(out, "        Self::{},", variant.name)?;
    }
    writeln!(out, "    ];")?;
    writeln!(out)?;
    writeln!(out, "    /// The W3C spelling of this value.")?;
    writeln!(out, "    #[must_use]")?;
    writeln!(out, "    pub const fn as_str(self) -> &'static str {{")?;
    writeln!(out, "        match self {{")?;
    for variant in variants {
        writeln!(
            out,
            "            Self::{} => {:?},",
            variant.name, variant.spelling
        )?;
    }
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out)?;
    writeln!(
        out,
        "    /// The value the W3C spells `name`, or `None` for a spelling this table does not list."
    )?;
    writeln!(out, "    #[must_use]")?;
    writeln!(out, "    pub fn from_w3c(name: &str) -> Option<Self> {{")?;
    writeln!(
        out,
        "        Self::ALL.iter().copied().find(|value| value.as_str() == name)"
    )?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "impl core::fmt::Display for {name} {{")?;
    writeln!(
        out,
        "    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {{"
    )?;
    writeln!(out, "        f.write_str(self.as_str())")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")
}

fn doc_line(out: &mut String, indent: &str, line: &str) -> std::fmt::Result {
    if line.is_empty() {
        writeln!(out, "{indent}///")
    } else {
        // A bare `*` inside upstream `<kbd>*</kbd>` reads as Markdown emphasis.
        let line = line.replace("<kbd>*</kbd>", r"<kbd>\*</kbd>");
        writeln!(out, "{indent}/// {line}")
    }
}

/// An upstream-shaped file: a deprecated variant, an alias and a spelling that differs from
/// the variant name.
const FIXTURE: &str = r#"
pub enum NamedKey {
    /// Unknown key.
    Unidentified,
    /// The Enter key.
    #[doc(alias = "Return")]
    Enter,
    #[deprecated = "legacy"]
    Hyper,
    /// A dead key, which `Key` carries.
    Dead,
    /// A key spelled differently.
    Spelled,
}

impl Display for NamedKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::NamedKey::*;
        match *self {
            Unidentified => f.write_str("Unidentified"),
            Enter => f.write_str("Enter"),
            Hyper => f.write_str("Hyper"),
            Dead => f.write_str("Dead"),
            Spelled => f.write_str("W3cSpelling"),
        }
    }
}
"#;

#[test]
fn the_fixture_generates_its_variants_and_spellings() {
    let generated = generate(TABLES[0], FIXTURE, "0.0.0").expect("the fixture generates");
    for expected in [
        "    /// The Enter key.\n    #[doc(alias = \"Return\")]\n    Enter,\n",
        "            Self::Spelled => \"W3cSpelling\",\n",
        "        Self::Unidentified,\n        Self::Enter,\n        Self::Spelled,\n    ];\n",
    ] {
        assert!(
            generated.contains(expected),
            "missing {expected:?} in\n{generated}"
        );
    }
    assert!(
        !generated.contains("Hyper"),
        "the deprecated variant is generated"
    );
    assert!(
        !generated.contains("    Dead,") && !generated.contains("Self::Dead"),
        "the variant `Key` carries is generated"
    );
}

#[test]
fn the_generator_refuses_what_it_would_otherwise_drop() {
    let table = TABLES[0];
    let carried_gone = FIXTURE.replace("Dead", "Deceased");
    assert!(
        generate(table, &carried_gone, "0.0.0").is_err(),
        "a carried variant gone upstream is accepted"
    );
    let unknown_attribute =
        FIXTURE.replace("#[doc(alias = \"Return\")]", "#[cfg(feature = \"x\")]");
    assert!(
        generate(table, &unknown_attribute, "0.0.0").is_err(),
        "an unknown variant attribute is accepted"
    );
}

#[test]
fn a_renamed_spelling_changes_the_generated_file() {
    let renamed = FIXTURE.replace("\"W3cSpelling\"", "\"Renamed\"");
    assert_ne!(
        generate(TABLES[0], &renamed, "0.0.0").expect("the renamed fixture generates"),
        generate(TABLES[0], FIXTURE, "0.0.0").expect("the fixture generates"),
        "a renamed spelling generates the same file"
    );
}
