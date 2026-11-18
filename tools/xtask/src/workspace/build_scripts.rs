//! No build script runs a platform compiler or packager (ADR-0154 decision 5,
//! ADR-0255 decision 21). Java and Kotlin sources, the dex, and Swift and
//! Objective-C sources are built by `flui build` from the sources a crate
//! declares (ADR-0307); a `build.rs` that spawns `javac`, `d8` or `swiftc`
//! puts a JDK, the Android build tools or Xcode on every consumer's machine
//! and makes the build depend on their versions.
//!
//! The rule reads every member's build script, the `custom-build` target Cargo
//! reports and each file it mounts with `mod`, as Rust syntax, so a comment or
//! a `cargo:` line that names a tool is not a finding. A tool counts as
//! spawned when a string literal names it in the program of a
//! `Command::new(…)`, or in an operand of `.arg(…)` or `.args(…)` (`xcrun
//! swiftc`, `sh -c "javac …"`, `java -jar d8.jar`), written there or reached
//! through a `const`, `static`, `let` binding or function of the same file. A
//! literal names a tool when one of its whitespace-separated words, with `{…}`
//! format placeholders removed, ends in a path component that is one of
//! [`TOOLS`], alone or with `.exe`, `.bat`, `.cmd` or `.jar`. A crate that
//! wraps the tools (`android-build`, `swift-rs`) is banned in `deny.toml`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use cargo_metadata::Metadata;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};

use super::{Identity, SelfTest, relative};

/// The platform compilers and packagers no build script runs, by the name of
/// their executable.
const TOOLS: [&str; 18] = [
    // Java and Kotlin
    "javac",
    "kotlinc",
    "kotlinc-jvm",
    "jar",
    // the dex
    "d8",
    "r8",
    "dx",
    // Swift and Objective-C, through Xcode's drivers
    "swiftc",
    "swift",
    "xcrun",
    "xcodebuild",
    // Android packaging
    "aapt",
    "aapt2",
    "apksigner",
    "zipalign",
    "bundletool",
    "gradle",
    "gradlew",
];

/// The extensions an executable or a launched archive carries on the hosts
/// that build.
const EXECUTABLE_EXTENSIONS: [&str; 4] = ["exe", "bat", "cmd", "jar"];

/// A member's build script: its root file and every file it mounts.
pub(super) struct BuildScript {
    package: String,
    /// Each file, relative to the repository root, with its syntax.
    files: Vec<(String, syn::File)>,
}

/// A build script that spawns one of [`TOOLS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Finding {
    package: String,
    file: String,
    line: usize,
    tool: &'static str,
}

impl Finding {
    fn identity(&self) -> Identity {
        (
            self.package.clone(),
            self.tool.to_owned(),
            "build script runs tool",
        )
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            package,
            file,
            line,
            tool,
        } = self;
        write!(
            f,
            "{file}:{line}: {package}'s build script runs `{tool}`: no build script runs a \
             platform compiler or packager; Java, Kotlin, Swift and Objective-C sources and the \
             dex are built by `flui build` from the sources the crate declares (ADR-0154 \
             decision 5, ADR-0255 decision 21)"
        )
    }
}

/// Every workspace member's build script, with the files it mounts.
pub(super) fn load(root: &Path, metadata: &Metadata) -> anyhow::Result<Vec<BuildScript>> {
    let mut scripts = Vec::new();
    for package in metadata.workspace_packages() {
        for target in package
            .targets
            .iter()
            .filter(|target| target.is_custom_build())
        {
            scripts.push(BuildScript {
                package: package.name.to_string(),
                files: script_files(root, target.src_path.as_std_path())?,
            });
        }
    }
    Ok(scripts)
}

/// The root file of a build script and every file its `mod` items mount,
/// resolved as rustc resolves a crate root's modules.
fn script_files(root: &Path, entry: &Path) -> anyhow::Result<Vec<(String, syn::File)>> {
    let mut files = Vec::new();
    // Each file to read, with the directory its own `mod` items resolve in.
    let mut pending = vec![(
        entry.to_path_buf(),
        entry
            .parent()
            .expect("BUG: a build script path has a parent directory")
            .to_path_buf(),
    )];
    while let Some((path, child_dir)) = pending.pop() {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading build script file {}", path.display()))?;
        let syntax = syn::parse_file(&text)
            .with_context(|| format!("parsing build script file {}", path.display()))?;
        let file_dir = path
            .parent()
            .expect("BUG: a file path has a parent directory");
        mounted(&syntax.items, file_dir, &child_dir, &mut pending)?;
        files.push((relative(root, &path)?, syntax));
    }
    Ok(files)
}

/// Queues the files the out-of-line `mod` items of `items` name: `#[path]`
/// relative to the declaring file's directory, otherwise `<dir>/<name>.rs` or
/// `<dir>/<name>/mod.rs`.
fn mounted(
    items: &[syn::Item],
    file_dir: &Path,
    dir: &Path,
    pending: &mut Vec<(PathBuf, PathBuf)>,
) -> anyhow::Result<()> {
    for item in items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        let name = module.ident.to_string();
        if let Some((_, content)) = &module.content {
            let inner = dir.join(&name);
            mounted(content, &inner, &inner, pending)?;
            continue;
        }
        let attribute = module
            .attrs
            .iter()
            .find(|attribute| attribute.path().is_ident("path"));
        if let Some(attribute) = attribute {
            let syn::Meta::NameValue(syn::MetaNameValue {
                value:
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(path),
                        ..
                    }),
                ..
            }) = &attribute.meta
            else {
                bail!("a `#[path]` in {} is not a string", file_dir.display());
            };
            let file = file_dir.join(path.value());
            let child_dir = file
                .parent()
                .expect("BUG: a joined path has a parent directory")
                .to_path_buf();
            pending.push((file, child_dir));
            continue;
        }
        let flat = dir.join(format!("{name}.rs"));
        let nested = dir.join(&name).join("mod.rs");
        let file = if flat.is_file() {
            flat
        } else if nested.is_file() {
            nested
        } else {
            bail!(
                "`mod {name};` in a build script under {} has neither {} nor {}",
                dir.display(),
                flat.display(),
                nested.display()
            );
        };
        pending.push((file, dir.join(&name)));
    }
    Ok(())
}

/// Every site where a build script spawns one of [`TOOLS`], once per file,
/// line and tool.
pub(super) fn check(scripts: &[BuildScript]) -> Vec<Finding> {
    let mut findings = BTreeSet::new();
    for script in scripts {
        for (file, syntax) in &script.files {
            for (line, tool) in spawned_tools(syntax) {
                findings.insert((file.clone(), line, tool, script.package.clone()));
            }
        }
    }
    findings
        .into_iter()
        .map(|(file, line, tool, package)| Finding {
            package,
            file,
            line,
            tool,
        })
        .collect()
}

/// `(line, tool)` for each literal that names a tool in a spawn operand of
/// `file`.
fn spawned_tools(file: &syn::File) -> Vec<(usize, &'static str)> {
    let mut operands = Operands::default();
    operands.visit_file(file);
    let mut bindings = Bindings::default();
    bindings.visit_file(file);

    let mut reached = Reached::default();
    for operand in &operands.0 {
        reached.visit_expr(operand);
    }
    let mut resolved = BTreeSet::new();
    while let Some(name) = reached.names.pop() {
        if !resolved.insert(name.clone()) {
            continue;
        }
        for bound in bindings.0.get(&name).into_iter().flatten() {
            match bound {
                Bound::Expr(expr) => reached.visit_expr(expr),
                Bound::Block(block) => reached.visit_block(block),
            }
        }
    }
    reached
        .strings
        .iter()
        .flat_map(|(text, line)| {
            text.split_whitespace()
                .filter_map(named_tool)
                .map(move |tool| (*line, tool))
        })
        .collect()
}

/// The tool a word of a literal names: its last path component, without
/// `{…}` placeholders, quotes or an executable extension, compared without
/// case (`C:\sdk\build-tools\d8.bat`, `{home}/bin/javac{EXE}`, `d8.jar`).
fn named_tool(word: &str) -> Option<&'static str> {
    let mut bare = String::with_capacity(word.len());
    let mut depth = 0_u32;
    for character in word.chars() {
        match character {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '\'' | '"' => {}
            _ if depth == 0 => bare.push(character),
            _ => {}
        }
    }
    let component = bare.rsplit(['/', '\\']).next()?;
    let stem = match component.rsplit_once('.') {
        Some((stem, extension))
            if EXECUTABLE_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension)) =>
        {
            stem
        }
        _ => component,
    };
    TOOLS
        .iter()
        .copied()
        .find(|tool| tool.eq_ignore_ascii_case(stem))
}

/// The expressions that name a process or its arguments: the program of a
/// `Command::new(…)` and each operand of `.arg(…)` and `.args(…)`, macro
/// bodies included.
#[derive(Default)]
struct Operands(Vec<syn::Expr>);

impl<'ast> Visit<'ast> for Operands {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = &*call.func
            && names_command_new(&function.path)
            && let Some(program) = call.args.first()
        {
            self.0.push(program.clone());
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "arg" || call.method == "args" {
            self.0.extend(call.args.iter().cloned());
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if let Some(body) = expression_body(mac) {
            for expr in &body {
                self.visit_expr(expr);
            }
        }
    }
}

/// `Command::new`, under any module path (`std::process`, `tokio::process`).
fn names_command_new(path: &syn::Path) -> bool {
    let mut segments = path.segments.iter().rev();
    matches!(
        (segments.next(), segments.next()),
        (Some(new), Some(command)) if new.ident == "new" && command.ident == "Command"
    )
}

/// A macro body read as comma-separated expressions (`format!`, `assert!`,
/// `concat!`), when it is one.
fn expression_body(mac: &syn::Macro) -> Option<Punctuated<syn::Expr, syn::Token![,]>> {
    mac.parse_body_with(Punctuated::parse_terminated).ok()
}

/// What a name of the file is bound to.
enum Bound<'ast> {
    /// A `const`, a `static` or a `let` initializer.
    Expr(&'ast syn::Expr),
    /// A function body.
    Block(&'ast syn::Block),
}

/// Each name the file binds, with every binding of that name: a script is
/// small, so a name bound twice reaches both.
#[derive(Default)]
struct Bindings<'ast>(BTreeMap<String, Vec<Bound<'ast>>>);

impl<'ast> Bindings<'ast> {
    fn bind(&mut self, name: &syn::Ident, bound: Bound<'ast>) {
        self.0.entry(name.to_string()).or_default().push(bound);
    }
}

impl<'ast> Visit<'ast> for Bindings<'ast> {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        let mut pattern = &local.pat;
        if let syn::Pat::Type(typed) = pattern {
            pattern = &typed.pat;
        }
        if let (syn::Pat::Ident(binding), Some(init)) = (pattern, &local.init) {
            self.bind(&binding.ident, Bound::Expr(&init.expr));
        }
        visit::visit_local(self, local);
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.bind(&item.ident, Bound::Expr(&item.expr));
        visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.bind(&item.ident, Bound::Expr(&item.expr));
        visit::visit_item_static(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.bind(&item.sig.ident, Bound::Block(&item.block));
        visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.bind(&item.sig.ident, Bound::Block(&item.block));
        visit::visit_impl_item_fn(self, item);
    }
}

/// The string literals an operand reaches, each with its line, and the
/// single-segment names it uses, which [`spawned_tools`] resolves.
#[derive(Default)]
struct Reached {
    strings: Vec<(String, usize)>,
    names: Vec<String>,
}

impl Reached {
    /// A macro body that is not a list of expressions: its literals and
    /// identifiers, token by token.
    fn scan_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Group(group) => self.scan_tokens(group.stream()),
                proc_macro2::TokenTree::Ident(ident) => self.names.push(ident.to_string()),
                proc_macro2::TokenTree::Literal(literal) => {
                    if let syn::Lit::Str(text) = syn::Lit::new(literal) {
                        self.visit_lit_str(&text);
                    }
                }
                proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Reached {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.strings
            .push((literal.value(), literal.span().start().line));
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if let Some(ident) = path.path.get_ident() {
            self.names.push(ident.to_string());
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        match expression_body(mac) {
            Some(body) => {
                for expr in &body {
                    self.visit_expr(expr);
                }
            }
            None => self.scan_tokens(mac.tokens.clone()),
        }
    }
}

/// The self-test's build scripts: one quiet script of every legal shape and
/// one planted spawn per way of naming a tool.
fn self_test_scripts() -> Vec<BuildScript> {
    let script = |package: &str, files: &[(&str, &str)]| BuildScript {
        package: package.to_owned(),
        files: files
            .iter()
            .map(|&(name, source)| {
                (
                    format!("crates/{package}/{name}"),
                    syn::parse_file(source).expect("BUG: a self-test build script parses"),
                )
            })
            .collect(),
    };
    vec![
        // silent: a compiler named in comments and `cargo:` lines, a
        // directory named like a tool outside any spawn, and a spawn of rustc
        script(
            "quiet",
            &[(
                "build.rs",
                r#"
                // javac and d8 are not run here: `flui build` compiles the Java sources.
                /// Runs `swiftc`? No.
                fn main() {
                    println!("cargo:rerun-if-changed=swift");
                    println!("cargo:warning=javac is not needed");
                    let swift = "swift";
                    let _ = swift;
                    let rustc = std::env::var("RUSTC").expect("cargo sets RUSTC");
                    let output = std::process::Command::new(&rustc).arg("--version").output();
                    assert!(output.is_ok(), "{}", "javac");
                }
                "#,
            )],
        ),
        // planted: the program is a literal
        script(
            "cap-literal",
            &[(
                "build.rs",
                r#"fn main() { Command::new("javac").status(); }"#,
            )],
        ),
        // planted: a path that ends in the tool and `.exe`
        script(
            "cap-path",
            &[(
                "build.rs",
                r#"fn main() { Command::new(home.join("bin").join("kotlinc.exe")).status(); }"#,
            )],
        ),
        // planted: through a `const`
        script(
            "cap-const",
            &[(
                "build.rs",
                r#"
                const DEXER: &str = "C:\\sdk\\build-tools\\d8.bat";
                fn main() { std::process::Command::new(DEXER).status(); }
                "#,
            )],
        ),
        // planted: through a function and a format string
        script(
            "cap-function",
            &[(
                "build.rs",
                r#"
                fn swift_compiler() -> PathBuf {
                    PathBuf::from(format!("{}/usr/bin/swiftc", toolchain()))
                }
                fn main() { Command::new(swift_compiler()).status(); }
                "#,
            )],
        ),
        // planted: a shell line passed as an argument
        script(
            "cap-shell",
            &[(
                "build.rs",
                r#"
                fn main() {
                    let mut shell = Command::new("sh");
                    shell.args(["-c", "xcodebuild -scheme App"]);
                }
                "#,
            )],
        ),
        // planted: a launched archive, inside a macro body
        script(
            "cap-archive",
            &[(
                "build.rs",
                r#"
                fn main() {
                    assert!(Command::new("java").args(["-jar", "lib/r8.jar"]).status().is_ok());
                }
                "#,
            )],
        ),
        // planted: in a file the script mounts, of a crate of any kind
        script(
            "core-mounted",
            &[
                (
                    "build.rs",
                    "mod android;\nfn main() { android::package(); }",
                ),
                (
                    "android.rs",
                    r#"pub fn package() { Command::new("./gradlew").arg("assemble"); }"#,
                ),
            ],
        ),
    ]
}

/// The finding each planted spawn must produce, and no other.
const PLANTED: [(&str, &str); 7] = [
    ("cap-literal", "javac"),
    ("cap-path", "kotlinc"),
    ("cap-const", "d8"),
    ("cap-function", "swiftc"),
    ("cap-shell", "xcodebuild"),
    ("cap-archive", "r8"),
    ("core-mounted", "gradlew"),
];

/// The rule over [`self_test_scripts`].
pub(super) fn self_test() -> SelfTest {
    let seen: BTreeSet<Identity> = check(&self_test_scripts())
        .iter()
        .map(Finding::identity)
        .collect();
    let expected: BTreeSet<Identity> = PLANTED
        .iter()
        .map(|&(package, tool)| {
            (
                package.to_owned(),
                tool.to_owned(),
                "build script runs tool",
            )
        })
        .collect();
    SelfTest {
        planted: expected.len(),
        missed: expected.difference(&seen).cloned().collect(),
        extra: seen.difference(&expected).cloned().collect(),
    }
}
