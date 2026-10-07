//! Source-derived native type-check scope. Scan the module trees of every
//! Cargo target, including test modules and feature-gated roots. Selection is
//! conservative: unrelated feature predicates do not exclude a native arm.
//! Macro tokens are scanned without expanding them; string literals and
//! comments do not create cfg predicates. Generated include! output remains
//! outside source discovery, so its target needs an explicit manifest gate.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, ensure};
use cargo_metadata::{Metadata, TargetKind};
use proc_macro2::{TokenStream, TokenTree};
use syn::parse::Parser as _;
use syn::punctuated::Punctuated;
use syn::{Expr, Lit, Meta, Token};

use super::PLATFORM_TARGETS;
use super::exec::{Cmd, Host, Step};
use crate::file_length::modules;

/// Names selected for each native target, sorted for reproducible plans.
pub(super) type Packages = BTreeMap<&'static str, BTreeSet<String>>;

/// Required Cargo target features, or every feature of the selected packages.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FeatureSet {
    RequiredTargets,
    All,
}

/// A predicate's native targets. `None` means it has no platform predicate;
/// negating an unrelated feature must not admit every native platform.
fn targets(meta: &Meta) -> Option<BTreeSet<&'static str>> {
    let all = || PLATFORM_TARGETS.into_iter().collect::<BTreeSet<_>>();
    let named = |value: &str| {
        PLATFORM_TARGETS
            .into_iter()
            .filter(|target| match value {
                "windows" | "msvc" | "x86_64" => target.contains("windows"),
                "macos" => target.contains("darwin"),
                "android" => target.contains("android"),
                "ios" => target.ends_with("ios"),
                "unix" | "aarch64" => !target.contains("windows"),
                _ => false,
            })
            .collect::<BTreeSet<_>>()
    };
    match meta {
        Meta::Path(path) if path.is_ident("windows") => Some(named("windows")),
        Meta::Path(path) if path.is_ident("unix") => Some(named("unix")),
        Meta::NameValue(value)
            if ["target_os", "target_arch", "target_env", "target_family"]
                .iter()
                .any(|name| value.path.is_ident(name)) =>
        {
            let Expr::Lit(expr) = &value.value else {
                return Some(all());
            };
            let Lit::Str(literal) = &expr.lit else {
                return Some(all());
            };
            Some(named(&literal.value()))
        }
        Meta::List(list) => {
            let args = Punctuated::<Meta, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .ok()?;
            let selected: Vec<_> = args.iter().filter_map(targets).collect();
            if selected.is_empty() {
                return None;
            }
            if list.path.is_ident("not") && args.len() == 1 {
                // Feature predicates can make a negated conjunction true on
                // the named target too. Keep every native target rather than
                // excluding a possible configuration.
                return Some(all());
            }
            // Both conjunction and disjunction retain every referenced native
            // arm. Feature resolution and surrounding cfgs can only narrow it.
            Some(selected.into_iter().flatten().collect())
        }
        _ => None,
    }
}

fn source_targets(tokens: TokenStream, out: &mut BTreeSet<&'static str>) {
    let trees: Vec<_> = tokens.into_iter().collect();
    for pair in trees.windows(2) {
        let [TokenTree::Ident(name), TokenTree::Group(group)] = pair else {
            continue;
        };
        if name == "cfg" {
            if let Ok(meta) = syn::parse2::<Meta>(group.stream()) {
                out.extend(targets(&meta).into_iter().flatten());
            }
        } else if name == "cfg_attr"
            && let Ok(args) = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(group.stream())
            && let Some(predicate) = args.first()
        {
            out.extend(targets(predicate).into_iter().flatten());
        }
    }
    for triple in trees.windows(3) {
        if let [
            TokenTree::Ident(name),
            TokenTree::Punct(bang),
            TokenTree::Group(group),
        ] = triple
            && name == "cfg"
            && bang.as_char() == '!'
            && let Ok(meta) = syn::parse2::<Meta>(group.stream())
        {
            out.extend(targets(&meta).into_iter().flatten());
        }
    }
    for tree in trees {
        if let TokenTree::Group(group) = tree {
            source_targets(group.stream(), out);
        }
    }
}

pub(super) fn discover(metadata: &Metadata, scope: &str) -> anyhow::Result<Packages> {
    let scope: BTreeSet<_> = scope.split_whitespace().collect();
    let mut packages = Packages::new();
    for package in metadata.workspace_packages() {
        if !scope.is_empty() && !scope.contains(package.name.as_str()) {
            continue;
        }
        let roots = package
            .targets
            .iter()
            .filter(|target| !target.kind.contains(&TargetKind::CustomBuild))
            .map(|target| target.src_path.clone().into_std_path_buf());
        let tree = modules::walk_all(roots);
        ensure!(
            tree.problems.is_empty(),
            "native source discovery for {}: {}",
            package.name,
            tree.problems
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
        let mut selected = BTreeSet::new();
        for path in tree.modules.keys().chain(&tree.test_files) {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading native cfgs in {}", path.display()))?;
            let tokens = text.parse::<TokenStream>().map_err(|error| {
                anyhow::anyhow!("tokenizing native cfgs in {}: {error}", path.display())
            })?;
            source_targets(tokens, &mut selected);
            proc_macro2::extra::invalidate_current_thread_spans();
        }
        for dependency in &package.dependencies {
            if let Some(platform) = &dependency.target {
                let text = platform.to_string();
                if PLATFORM_TARGETS.contains(&text.as_str()) {
                    selected.extend(
                        PLATFORM_TARGETS
                            .into_iter()
                            .filter(|target| *target == text),
                    );
                } else {
                    let tokens = text
                        .parse::<TokenStream>()
                        .map_err(|error| anyhow::anyhow!("native manifest cfg {text}: {error}"))?;
                    source_targets(tokens, &mut selected);
                }
            }
        }
        for target in selected {
            packages
                .entry(target)
                .or_default()
                .insert(package.name.to_string());
        }
    }
    Ok(packages)
}

/// One feature resolution per native target. All Cargo target kinds are
/// checked directly, rather than relying on incidental dependency builds.
pub(super) fn plans(
    root: &Path,
    scope: &str,
    host: Host,
    feature_set: FeatureSet,
) -> anyhow::Result<BTreeMap<&'static str, Step>> {
    let metadata = crate::util::metadata(root)?;
    let selected = discover(&metadata, scope)?;
    let mut steps = BTreeMap::new();
    for (target, names) in selected {
        let cross_macos = target == super::MACOS_TARGET && host != Host::MacOs;
        let mut cmd = if target == super::WINDOWS_TARGET && host != Host::Windows {
            // stacker's Windows C shim needs SDK headers, even without linking.
            Cmd::cargo(["xwin", "clippy"])
        } else if cross_macos {
            // Zig supplies Darwin libc headers for C dependencies such as
            // AWS-LC. Bare clang can assemble psm but cannot build that graph.
            Cmd::new("cargo-zigbuild").args(["clippy"])
        } else {
            Cmd::cargo(["clippy"])
        };
        let mut features = BTreeSet::new();
        for name in &names {
            cmd = cmd.args(["-p", name]);
            let package = metadata
                .workspace_packages()
                .into_iter()
                .find(|package| package.name.as_str() == name)
                .expect("BUG: discovered package belongs to workspace metadata");
            for feature in package
                .targets
                .iter()
                .flat_map(|target| &target.required_features)
            {
                features.insert(format!("{name}/{feature}"));
            }
            for feature in ["testing", "a11y"] {
                if package.features.contains_key(feature) {
                    features.insert(format!("{name}/{feature}"));
                }
            }
        }
        cmd = cmd.args(["--all-targets", "--locked", "--target", target]);
        if feature_set == FeatureSet::All {
            cmd = cmd.args(["--all-features"]);
        } else if !features.is_empty() {
            cmd = cmd.args([
                "--features",
                &features.into_iter().collect::<Vec<_>>().join(","),
            ]);
        }
        if target != super::WINDOWS_TARGET && host != Host::MacOs && !cross_macos {
            // Preserve cc-rs target/global overrides. Android callers also
            // supply NDK headers; iOS callers need an Apple SDK, not just clang.
            let suffix = target.replace('-', "_");
            for (prefix, fallback) in [("CC", "clang"), ("AR", "llvm-ar")] {
                let key = format!("{prefix}_{suffix}");
                if std::env::var_os(&key).is_none()
                    && std::env::var_os(format!("{prefix}_{target}")).is_none()
                    && std::env::var_os(format!("TARGET_{prefix}")).is_none()
                    && std::env::var_os(prefix).is_none()
                {
                    cmd = cmd.env(key, fallback);
                }
            }
        }
        steps.insert(target, super::deny_warnings(cmd).into());
    }
    Ok(steps)
}

pub(super) fn plan(
    root: &Path,
    scope: &str,
    host: Host,
    only: Option<&str>,
    feature_set: FeatureSet,
) -> anyhow::Result<Vec<Step>> {
    Ok(plans(root, scope, host, feature_set)?
        .into_iter()
        .filter(|(target, _)| only.is_none_or(|only| only == *target))
        .map(|(_, step)| step)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_predicates_include_macro_arms_without_reading_literals() {
        crate::table_test::run_table(
            "native_predicates",
            &[
                (
                    "native_atoms_and_negation",
                    native_atoms_and_negation as fn(),
                ),
                (
                    "macro_attributes_and_strings",
                    macro_attributes_and_strings as fn(),
                ),
                (
                    "cargo_target_module_discovery",
                    cargo_target_module_discovery as fn(),
                ),
            ],
        );
    }

    fn native_atoms_and_negation() {
        for (source, expected) in [
            ("cfg(windows)", vec![super::super::WINDOWS_TARGET]),
            (
                "cfg(all(target_os = \"macos\", feature = \"testing\"))",
                vec![super::super::MACOS_TARGET],
            ),
            ("cfg(not(feature = \"testing\"))", vec![]),
            ("cfg(not(target_os = \"linux\"))", PLATFORM_TARGETS.to_vec()),
        ] {
            let mut found = BTreeSet::new();
            source_targets(source.parse().expect("fixture tokens"), &mut found);
            assert_eq!(found, expected.into_iter().collect(), "{source}");
        }
    }

    fn macro_attributes_and_strings() {
        let source = r##"
            const TEMPLATE: &str = "#[cfg(windows)]";
            // cfg(target_os = "android")
            macro_rules! native { () => { #[cfg(windows)] fn native() {} }; }
            #[cfg_attr(target_os = "macos", allow(dead_code))] fn other() {}
            fn platform() { let _ = cfg!(target_os = "ios"); }
        "##;
        let mut found = BTreeSet::new();
        source_targets(source.parse().expect("fixture tokens"), &mut found);
        assert_eq!(
            found,
            [
                super::super::WINDOWS_TARGET,
                super::super::MACOS_TARGET,
                super::super::IOS_TARGET
            ]
            .into_iter()
            .collect()
        );
    }

    fn cargo_target_module_discovery() {
        let dir = tempfile::tempdir().expect("fixture workspace");
        for (path, text) in [
            (
                "Cargo.toml",
                "[package]\nname = \"native-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[workspace]\n",
            ),
            ("src/lib.rs", "mod child;\n#[cfg(test)] mod test_only;\n"),
            (
                "src/child.rs",
                "#[cfg(target_os = \"macos\")] pub fn apple() {}",
            ),
            ("src/test_only.rs", "#[cfg(windows)] fn windows_test() {}"),
            (
                "examples/mobile.rs",
                "#[cfg(target_os = \"android\")] fn android() {}\nfn main() {}",
            ),
            (
                "benches/apple.rs",
                "#[cfg(target_os = \"ios\")] fn ios() {}\nfn main() {}",
            ),
            ("src/orphan.rs", "compile_error!(\"not a module\");"),
        ] {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().expect("fixture parent"))
                .expect("create fixture directory");
            std::fs::write(path, text).expect("write fixture source");
        }
        let metadata = crate::util::metadata(dir.path()).expect("fixture Cargo targets");
        let found = discover(&metadata, "native-fixture").expect("discover module trees");
        assert_eq!(
            found,
            PLATFORM_TARGETS
                .into_iter()
                .map(|target| (target, BTreeSet::from(["native-fixture".to_owned()])))
                .collect()
        );
        assert!(
            discover(&metadata, "another-package")
                .expect("narrow scope")
                .is_empty()
        );
        std::fs::write(dir.path().join("src/test_only.rs"), "fn broken(")
            .expect("invalid test module fixture");
        assert!(
            discover(&metadata, "native-fixture").is_err(),
            "unreadable test modules must not silently disappear from coverage"
        );
    }
}
