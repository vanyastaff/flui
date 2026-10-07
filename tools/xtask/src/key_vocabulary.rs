//! FLUI's W3C key tables, generated from the `keyboard-types` the workspace pins (ADR-0089 §4).
//!
//! `flui_platform_api::keyboard::{NamedKey, Code}` are FLUI's own enums, so no `keyboard-types`
//! type reaches a Stable signature, but their variants are the W3C `key` and `code` value lists,
//! which `keyboard-types` already transcribes from the specifications. This command reads the
//! `keyboard-types` sources that `ui-events` resolves to (`named_key.rs` and `code.rs`) and writes
//! both enums with their documentation, an `ALL` table and the W3C spelling of every variant.
//!
//! Without arguments it checks that the checked-in files are what the pinned `keyboard-types`
//! generates, so a `keyboard-types` bump that adds, removes or renames a key fails `cargo xtask
//! checks` until the tables are regenerated with `--write`. A variant upstream marks
//! `#[deprecated]` is not generated: the specification lists it as legacy, and the backend
//! conversion maps its spelling to the current key. Any other upstream attribute on a variant
//! fails the run instead of being dropped silently. `--self-test` runs the generator over an
//! embedded fixture and fails unless the fixture's output and its stale-file finding come back.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, anyhow, bail};
use syn::visit::Visit;

use crate::util::repo_root;

/// One generated table: the upstream file it is read from and the file it is written to.
#[derive(Debug, Clone, Copy)]
struct Table {
    /// The enum's name, upstream and here.
    name: &'static str,
    /// The upstream file, relative to the `keyboard-types` package root.
    upstream: &'static str,
    /// The generated file, relative to the repository root.
    output: &'static str,
    /// The enum's own documentation.
    summary: &'static str,
    /// A variant whose W3C spelling is its name, for the generated example.
    example: &'static str,
}

const TABLES: [Table; 2] = [
    Table {
        name: "NamedKey",
        upstream: "src/named_key.rs",
        output: "crates/flui-platform-api/src/keyboard/named_key.rs",
        summary: "A key the W3C names: the `key` value of a key that produces no character.\n\n\
                  The variants are the [UI Events `key` values](https://w3c.github.io/uievents-key/).\n\
                  A value the specification adds before FLUI regenerates this table arrives as\n\
                  [`NamedKey::Unidentified`].",
        example: "Enter",
    },
    Table {
        name: "Code",
        upstream: "src/code.rs",
        output: "crates/flui-platform-api/src/keyboard/code.rs",
        summary: "The physical key, independent of the keyboard layout: the W3C `code` value.\n\n\
                  The variants are the [UI Events `code` values](https://w3c.github.io/uievents-code/).\n\
                  A value the specification adds before FLUI regenerates this table arrives as\n\
                  [`Code::Unidentified`].",
        example: "KeyA",
    },
];

/// Arguments for `cargo xtask key-vocabulary`.
#[derive(Debug, clap::Args)]
pub(crate) struct KeyVocabularyArgs {
    /// Regenerate the tables instead of checking them.
    #[arg(long, conflicts_with = "self_test")]
    write: bool,
    /// Run the generator over the embedded fixture instead of `keyboard-types`.
    #[arg(long)]
    self_test: bool,
}

/// `cargo xtask key-vocabulary`: check (or `--write`) the generated W3C key tables.
pub(crate) fn key_vocabulary(args: &KeyVocabularyArgs) -> anyhow::Result<ExitCode> {
    if args.self_test {
        return self_test();
    }
    let root = repo_root();
    let (upstream_root, version) = keyboard_types_root(&root)?;
    let mut stale = Vec::new();
    for table in TABLES {
        let source = read(&upstream_root.join(table.upstream))?;
        let generated = generate(table, &source, &version)
            .with_context(|| format!("generating {} from {}", table.name, table.upstream))?;
        let output = root.join(table.output);
        if args.write {
            std::fs::write(&output, &generated)
                .with_context(|| format!("writing {}", output.display()))?;
            println!("key-vocabulary: wrote {}", table.output);
        } else if std::fs::read_to_string(&output).ok().as_deref() != Some(generated.as_str()) {
            stale.push(table.output);
        }
    }
    if stale.is_empty() {
        if !args.write {
            println!("key-vocabulary: the key tables match keyboard-types {version}");
        }
        return Ok(ExitCode::SUCCESS);
    }
    for path in stale {
        eprintln!(
            "key-vocabulary: {path} is not what keyboard-types {version} generates; \
             run `cargo xtask key-vocabulary --write`"
        );
    }
    Ok(ExitCode::FAILURE)
}

fn read(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// The package root and version of the `keyboard-types` that `ui-events` resolves to.
fn keyboard_types_root(root: &Path) -> anyhow::Result<(PathBuf, String)> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .context("running `cargo metadata --locked`")?;
    let resolve = metadata
        .resolve
        .as_ref()
        .context("`cargo metadata` returned no dependency graph")?;
    let ui_events: Vec<_> = metadata
        .packages
        .iter()
        .filter(|package| package.name.as_str() == "ui-events")
        .collect();
    let [ui_events] = ui_events.as_slice() else {
        bail!(
            "expected one ui-events package in the lock file, found {}",
            ui_events.len()
        );
    };
    let node = resolve
        .nodes
        .iter()
        .find(|node| node.id == ui_events.id)
        .context("ui-events is not in the dependency graph")?;
    let dependency = node
        .deps
        .iter()
        .find(|dep| dep.name == "keyboard_types")
        .context("ui-events does not depend on keyboard-types")?;
    let package = &metadata[&dependency.pkg];
    let manifest_dir = package
        .manifest_path
        .parent()
        .context("keyboard-types' manifest path has no parent")?;
    Ok((
        manifest_dir.as_std_path().to_owned(),
        package.version.to_string(),
    ))
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
fn generate(table: Table, source: &str, version: &str) -> anyhow::Result<String> {
    let file = syn::parse_file(source).context("parsing the upstream file")?;
    let variants = variants(&file, table.name)?;
    let mut out = String::new();
    write_file(&mut out, table, version, &variants)?;
    Ok(out)
}

fn variants(file: &syn::File, name: &str) -> anyhow::Result<Vec<Variant>> {
    let item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(item) if item.ident == name => Some(item),
            _ => None,
        })
        .with_context(|| format!("no `enum {name}` upstream"))?;
    let mut spellings = Spellings::default();
    for item in &file.items {
        if let syn::Item::Impl(item) = item
            && implements_display_for(item, name)
        {
            spellings.visit_item_impl(item);
        }
    }
    if spellings.arms.is_empty() {
        bail!("no `impl Display for {name}` upstream");
    }
    let mut variants = Vec::new();
    'variants: for variant in &item.variants {
        if !matches!(variant.fields, syn::Fields::Unit) {
            bail!("{name}::{} carries data", variant.ident);
        }
        let mut doc = Vec::new();
        let mut aliases = Vec::new();
        for attr in &variant.attrs {
            if attr.path().is_ident("deprecated") {
                continue 'variants;
            }
            if !attr.path().is_ident("doc") {
                bail!(
                    "{name}::{} has an attribute the generator does not know: {}",
                    variant.ident,
                    attr.path()
                        .get_ident()
                        .map_or_else(|| "a path".to_owned(), ToString::to_string)
                );
            }
            match &attr.meta {
                syn::Meta::NameValue(meta) => doc.push(string_literal(&meta.value)?),
                syn::Meta::List(_) => attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("alias") {
                        let alias: syn::LitStr = meta.value()?.parse()?;
                        aliases.push(alias.value());
                        Ok(())
                    } else {
                        Err(meta.error("only `doc(alias)` is generated"))
                    }
                })?,
                syn::Meta::Path(_) => bail!("{name}::{} has a bare `#[doc]`", variant.ident),
            }
        }
        let ident = variant.ident.to_string();
        let spelling = spellings
            .arms
            .iter()
            .find(|(arm, _)| *arm == ident)
            .map(|(_, spelling)| spelling.clone())
            .with_context(|| format!("{name}::{ident} has no Display spelling upstream"))?;
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
        bail!("{name} has no `Unidentified` variant, the fallback for an unknown value");
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

fn string_literal(expr: &syn::Expr) -> anyhow::Result<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(literal),
            ..
        }) => Ok(literal.value()),
        _ => Err(anyhow!("a `#[doc = ...]` that is not a string literal")),
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
        "// @generated by `cargo xtask key-vocabulary --write` from keyboard-types {version}"
    )?;
    writeln!(
        out,
        "// ({}, MIT OR Apache-2.0). Edit the generator, not this file.",
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
        format!("use flui_platform_api::keyboard::{name};"),
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
                doc_line(out, "    ", line.strip_prefix(' ').unwrap_or(line))?;
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
const SELF_TEST_FIXTURE: &str = r#"
pub enum NamedKey {
    /// Unknown key.
    Unidentified,
    /// The Enter key.
    #[doc(alias = "Return")]
    Enter,
    #[deprecated = "legacy"]
    Hyper,
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
            Spelled => f.write_str("W3cSpelling"),
        }
    }
}
"#;

fn self_test() -> anyhow::Result<ExitCode> {
    let table = TABLES[0];
    let generated = generate(table, SELF_TEST_FIXTURE, "0.0.0")?;
    let mut missed: Vec<&str> = [
        "    /// The Enter key.\n    #[doc(alias = \"Return\")]\n    Enter,\n",
        "            Self::Spelled => \"W3cSpelling\",\n",
        "        Self::Unidentified,\n        Self::Enter,\n        Self::Spelled,\n    ];\n",
    ]
    .into_iter()
    .filter(|expected| !generated.contains(expected))
    .collect();
    if generated.contains("Hyper") {
        missed.push("the deprecated variant is generated");
    }
    let unknown_attribute =
        SELF_TEST_FIXTURE.replace("#[doc(alias = \"Return\")]", "#[cfg(feature = \"x\")]");
    if generate(table, &unknown_attribute, "0.0.0").is_ok() {
        missed.push("an unknown variant attribute is accepted");
    }
    let renamed = SELF_TEST_FIXTURE.replace("\"W3cSpelling\"", "\"Renamed\"");
    if generate(table, &renamed, "0.0.0")? == generated {
        missed.push("a renamed spelling generates the same file");
    }
    for finding in &missed {
        println!("self-test: MISSED {finding}");
    }
    if missed.is_empty() {
        println!("key-vocabulary self-test: the fixture generates as expected");
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}
