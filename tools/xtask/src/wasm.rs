//! wasm helpers: the import allowlist, the crates whose tests run on wasm, locked tool versions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, ensure};

use crate::util::{read, repo_root};

/// The reviewed set of host modules a linked cdylib may import from, one per line.
const ALLOWLIST: &str = "tools/xtask/fixtures/wasm/import-allowlist.txt";

/// Arguments for `cargo xtask wasm-imports`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmImportsArgs {
    /// Linked wasm modules to check.
    #[arg(required = true, num_args = 1..)]
    wasm: Vec<PathBuf>,
}

/// `cargo xtask wasm-imports`: check a linked wasm module's imports against the allowlist.
///
/// An undefined symbol on wasm32 does not fail the link — rust-lld turns it
/// into an import — so "it linked" proves nothing about the import surface.
/// This check makes the import list an intentional, reviewed artifact: a new
/// module (most notably `env`, the catch-all for accidental native externs)
/// fails CI until it is added to the committed allowlist.
pub(crate) fn wasm_imports(args: &WasmImportsArgs) -> anyhow::Result<ExitCode> {
    let allowlist_path = repo_root().join(ALLOWLIST);
    let Ok(allowlist) = std::fs::read_to_string(&allowlist_path) else {
        eprintln!("error: allowlist not found at {ALLOWLIST}");
        return Ok(ExitCode::from(2));
    };
    let allowed: BTreeSet<&str> = allowlist
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    let mut status = ExitCode::SUCCESS;
    for wasm in &args.wasm {
        let shown = wasm.display();
        let Ok(bytes) = std::fs::read(wasm) else {
            eprintln!("error: {shown} does not exist (was the cdylib built?)");
            status = ExitCode::FAILURE;
            continue;
        };
        let modules = import_modules(&bytes).with_context(|| format!("parsing {shown}"))?;
        let unexpected: Vec<&String> = modules
            .iter()
            .filter(|module| !allowed.contains(module.as_str()))
            .collect();
        if unexpected.is_empty() {
            println!("ok: {shown} imports only from allowed modules:");
            for module in &modules {
                println!("  {module}");
            }
        } else {
            eprintln!("error: {shown} imports from modules not in {ALLOWLIST}:");
            for module in unexpected {
                eprintln!("  {module}");
            }
            eprintln!("If intentional, add the module to the allowlist with a comment in the PR.");
            status = ExitCode::FAILURE;
        }
    }
    Ok(status)
}

/// The distinct module names of a core wasm module's imports, sorted.
///
/// Use wasmparser's maintained binary grammar, including compact imports and
/// bounded u32 lengths. Iterate import readers fully to detect malformed entries
/// and trailing bytes. This inspects the import surface, not full module semantics;
/// compilation and the browser runner remain responsible for module validation.
fn import_modules(bytes: &[u8]) -> anyhow::Result<BTreeSet<String>> {
    ensure!(
        wasmparser::Parser::is_core_wasm(bytes),
        "not a core wasm module (bad magic, version or a component)"
    );
    let mut modules = BTreeSet::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::ImportSection(section) = payload? {
            for import in section.into_imports() {
                modules.insert(import?.module.to_owned());
            }
        }
    }
    Ok(modules)
}

/// The dev-dependency whose wasm32-conditional presence opts a crate in.
const WASM_TEST_DEP: &str = "wasm-bindgen-test";

/// Arguments for `cargo xtask wasm-test-crates`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmTestCratesArgs {
    /// Directory whose `*/Cargo.toml` are scanned (default: crates).
    root: Option<PathBuf>,
}

/// `cargo xtask wasm-test-crates`: print the crates whose tests run on wasm32.
///
/// The opt-in signal is a `wasm-bindgen-test` entry under a wasm32-conditional
/// `dev-dependencies` table. That is where a contributor already declares
/// intent, so it cannot drift from the code the way a hardcoded list in
/// xtask and the CI workflow would. Parsed rather than grepped: a substring
/// search also matches a comment, a normal dependency, or a non-wasm32 target
/// table — none of which mean "this crate has wasm tests" — and that mismatch
/// would be silent.
pub(crate) fn wasm_test_crates(args: &WasmTestCratesArgs) -> anyhow::Result<ExitCode> {
    let root = args
        .root
        .clone()
        .unwrap_or_else(|| repo_root().join("crates"));
    let mut crates: Vec<(String, PathBuf)> = std::fs::read_dir(&root)
        .with_context(|| format!("reading {}", root.display()))?
        .filter_map(Result::ok)
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path().join("Cargo.toml"),
            )
        })
        .filter(|(_, manifest)| manifest.is_file())
        .collect();
    crates.sort();
    for (name, manifest) in crates {
        match std::fs::read_to_string(&manifest)
            .map_err(anyhow::Error::from)
            .and_then(|text| opts_in(&text))
        {
            Ok(true) => println!("{name}"),
            Ok(false) => {}
            Err(error) => {
                eprintln!("{}: {error:#}", manifest.display());
                return Ok(ExitCode::from(2));
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Whether a manifest declares `wasm-bindgen-test` under a wasm32 target's dev-dependencies.
fn opts_in(manifest: &str) -> anyhow::Result<bool> {
    let parsed: toml::Table = manifest.parse()?;
    let Some(targets) = parsed.get("target").and_then(toml::Value::as_table) else {
        return Ok(false);
    };
    Ok(targets
        .iter()
        .filter(|(key, _)| key.contains("wasm32"))
        .filter_map(|(_, table)| table.get("dev-dependencies")?.as_table())
        .any(|deps| deps.contains_key(WASM_TEST_DEP)))
}

/// Arguments for `cargo xtask locked-version`.
#[derive(Debug, clap::Args)]
pub(crate) struct LockedVersionArgs {
    /// Package name as it appears in Cargo.lock.
    #[arg(value_parser = clap::builder::NonEmptyStringValueParser::new())]
    package: String,
}

/// `cargo xtask locked-version`: print a package's version from Cargo.lock.
///
/// The single source of truth for "what version of a tool does the lockfile
/// pin" — the wasm-bindgen CLI must match the locked `wasm-bindgen` exactly,
/// or `wasm-bindgen-test-runner` refuses to start.
pub(crate) fn locked_version(args: &LockedVersionArgs) -> anyhow::Result<ExitCode> {
    if let Some(version) = locked_package_version(&read("Cargo.lock")?, &args.package)? {
        println!("{version}");
        Ok(ExitCode::SUCCESS)
    } else {
        eprintln!("error: package '{}' not found in Cargo.lock", args.package);
        Ok(ExitCode::FAILURE)
    }
}

/// The version of the first `[[package]]` named `name` in a Cargo.lock.
pub(crate) fn locked_package_version(lock: &str, name: &str) -> anyhow::Result<Option<String>> {
    let parsed: toml::Table = lock.parse().context("parsing Cargo.lock")?;
    let packages = parsed
        .get("package")
        .and_then(toml::Value::as_array)
        .context("Cargo.lock has no [[package]] array")?;
    Ok(packages
        .iter()
        .find(|pkg| pkg.get("name").and_then(toml::Value::as_str) == Some(name))
        .and_then(|pkg| pkg.get("version")?.as_str())
        .map(str::to_owned))
}

/// The locked version of `name` in this repository's Cargo.lock.
pub(crate) fn repo_locked_version(name: &str) -> anyhow::Result<Option<String>> {
    locked_package_version(&read(Path::new("Cargo.lock"))?, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leb(mut value: usize, out: &mut Vec<u8>) {
        loop {
            let byte = u8::try_from(value & 0x7f).expect("BUG: masked to 7 bits");
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn name(text: &str, out: &mut Vec<u8>) {
        leb(text.len(), out);
        out.extend_from_slice(text.as_bytes());
    }

    fn section(id: u8, payload: &[u8], out: &mut Vec<u8>) {
        out.push(id);
        leb(payload.len(), out);
        out.extend_from_slice(payload);
    }

    /// A module with one import of every kind, a type section before and a
    /// custom section after, so the walk has to skip sections on both sides.
    fn module() -> Vec<u8> {
        let mut imports = Vec::new();
        leb(6, &mut imports);
        for (module, field, desc) in [
            (
                "__wbindgen_placeholder__",
                "__wbindgen_describe",
                &[0x00, 0x00][..],
            ),
            ("env", "memory", &[0x02, 0x03, 0x11, 0x80, 0x80, 0x04][..]),
            (
                "__wbindgen_externref_xform__",
                "table",
                &[0x01, 0x6f, 0x00, 0x04][..],
            ),
            ("env", "gref", &[0x03, 0x64, 0x70, 0x01][..]),
            ("wasi", "tag", &[0x04, 0x00, 0x00][..]),
            (
                "__wbindgen_placeholder__",
                "__wbg_log",
                &[0x00, 0x81, 0x01][..],
            ),
        ] {
            name(module, &mut imports);
            name(field, &mut imports);
            imports.extend_from_slice(desc);
        }
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        section(1, &[0x01, 0x60, 0x00, 0x00], &mut bytes);
        section(2, &imports, &mut bytes);
        let mut custom = Vec::new();
        name("producers", &mut custom);
        custom.extend_from_slice(b"\x00");
        section(0, &custom, &mut bytes);
        bytes
    }

    fn import_modules_are_read_from_the_binary() {
        let modules = import_modules(&module()).expect("valid module");
        assert_eq!(
            modules.into_iter().collect::<Vec<_>>(),
            [
                "__wbindgen_externref_xform__",
                "__wbindgen_placeholder__",
                "env",
                "wasi"
            ]
        );
    }

    fn malformed_binaries_are_errors_not_passes() {
        let mut truncated = module();
        truncated.truncate(truncated.len() - 20);
        assert!(import_modules(&truncated).is_err());
        assert!(import_modules(b"\0asm\x0d\0\x01\0").is_err(), "component");
        assert!(import_modules(b"MZ\x90\0\x03\0\0\0").is_err(), "bad magic");
    }

    fn import_counts_refuse_overwide_u32_encodings() {
        let mut legal = b"\0asm\x01\0\0\0".to_vec();
        section(2, &[0x80, 0x80, 0x80, 0x80, 0], &mut legal);
        assert!(
            import_modules(&legal)
                .expect("legal padded u32 zero")
                .is_empty()
        );
        for width in 6..=10 {
            let mut bytes = b"\0asm\x01\0\0\0".to_vec();
            let mut count = vec![0x80; width - 1];
            count.push(0);
            section(2, &count, &mut bytes);
            assert!(
                import_modules(&bytes).is_err(),
                "u32 count used {width} bytes"
            );
        }
    }

    fn an_import_check_requires_at_least_one_module() {
        use clap::{Args, FromArgMatches};
        let parse = |argv: &[&str]| -> Result<WasmImportsArgs, clap::Error> {
            let matches = WasmImportsArgs::augment_args(clap::Command::new("xtask"))
                .try_get_matches_from(argv)?;
            WasmImportsArgs::from_arg_matches(&matches)
        };
        assert!(parse(&["xtask"]).is_err());
        let args = parse(&["xtask", "module.wasm"]).expect("a module is required");
        assert_eq!(args.wasm, [PathBuf::from("module.wasm")]);
    }

    fn compact_import_groups_reach_the_same_module_allowlist() {
        // wasmparser 0.261's core/imports reader: an empty field name followed
        // by 0x7f shares a module; 0x7e shares both module and type.
        let mut imports = vec![2];
        name("env", &mut imports);
        name("", &mut imports);
        imports.extend_from_slice(&[0x7f, 2]);
        name("function", &mut imports);
        imports.extend_from_slice(&[0, 0]);
        name("memory", &mut imports);
        imports.extend_from_slice(&[2, 0, 1]);
        name("wasi", &mut imports);
        name("", &mut imports);
        imports.extend_from_slice(&[0x7e, 0, 0, 2]);
        name("first", &mut imports);
        name("second", &mut imports);
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        section(2, &imports, &mut bytes);
        assert_eq!(
            import_modules(&bytes)
                .expect("compact groups")
                .into_iter()
                .collect::<Vec<_>>(),
            ["env", "wasi"]
        );
    }

    fn committed_allowlist_admits_wasm_bindgen_and_refuses_env() {
        let allowlist = read(ALLOWLIST).expect("allowlist is committed");
        let allowed: BTreeSet<&str> = allowlist.lines().map(str::trim).collect();
        assert!(allowed.contains("__wbindgen_placeholder__"));
        assert!(allowed.contains("__wbindgen_externref_xform__"));
        assert!(!allowed.contains("env"));
    }

    fn opt_in_needs_a_wasm32_dev_dependency() {
        let yes = "[target.'cfg(target_arch = \"wasm32\")'.dev-dependencies]\nwasm-bindgen-test = \"0.3\"\n";
        assert!(opts_in(yes).expect("parses"));
        for no in [
            "# wasm-bindgen-test = \"0.3\"\n[package]\nname = \"a\"\n",
            "[dependencies]\nwasm-bindgen-test = \"0.3\"\n",
            "[dev-dependencies]\nwasm-bindgen-test = \"0.3\"\n",
            "[target.'cfg(windows)'.dev-dependencies]\nwasm-bindgen-test = \"0.3\"\n",
            "[target.'cfg(target_arch = \"wasm32\")'.dependencies]\nwasm-bindgen-test = \"0.3\"\n",
        ] {
            assert!(!opts_in(no).expect("parses"), "{no}");
        }
        assert!(opts_in("[target\n").is_err());
    }

    #[test]
    fn wasm_gate_contract() {
        crate::table_test::run_table(
            "wasm_gate_contract",
            &[
                (
                    "an_import_check_requires_at_least_one_module",
                    an_import_check_requires_at_least_one_module as fn(),
                ),
                (
                    "import_counts_refuse_overwide_u32_encodings",
                    import_counts_refuse_overwide_u32_encodings as fn(),
                ),
                (
                    "compact_import_groups_reach_the_same_module_allowlist",
                    compact_import_groups_reach_the_same_module_allowlist as fn(),
                ),
                (
                    "import_modules_are_read_from_the_binary",
                    import_modules_are_read_from_the_binary as fn(),
                ),
                (
                    "malformed_binaries_are_errors_not_passes",
                    malformed_binaries_are_errors_not_passes as fn(),
                ),
                (
                    "committed_allowlist_admits_wasm_bindgen_and_refuses_env",
                    committed_allowlist_admits_wasm_bindgen_and_refuses_env as fn(),
                ),
                (
                    "opt_in_needs_a_wasm32_dev_dependency",
                    opt_in_needs_a_wasm32_dev_dependency as fn(),
                ),
            ],
        );
    }
}
