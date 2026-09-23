//! wasm helpers: the import allowlist, the crates whose tests run on wasm, locked tool versions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail, ensure};

use crate::util::{read, repo_root};

/// The reviewed set of host modules a linked cdylib may import from, one per line.
const ALLOWLIST: &str = "tools/xtask/fixtures/wasm/import-allowlist.txt";

/// Arguments for `cargo xtask wasm-imports`.
#[derive(Debug, clap::Args)]
pub(crate) struct WasmImportsArgs {
    /// Linked wasm modules to check.
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

/// A cursor over a wasm binary.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn take(&mut self, len: usize) -> anyhow::Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .with_context(|| format!("truncated at byte {}", self.pos))?;
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn byte(&mut self) -> anyhow::Result<u8> {
        Ok(self.take(1)?[0])
    }

    /// An unsigned or signed LEB128 of up to 64 bits; the value only matters
    /// for lengths and counts, so the sign of an `s33` heap type is ignored.
    fn leb(&mut self) -> anyhow::Result<u64> {
        let mut value = 0_u64;
        for shift in (0..70).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift.min(63);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        bail!("LEB128 longer than 10 bytes at byte {}", self.pos)
    }

    fn len(&mut self) -> anyhow::Result<usize> {
        usize::try_from(self.leb()?).context("length does not fit in usize")
    }

    fn name(&mut self) -> anyhow::Result<&'a str> {
        let len = self.len()?;
        std::str::from_utf8(self.take(len)?).context("import name is not UTF-8")
    }

    /// A value or reference type; the GC proposal's `(ref null? ht)` carries a heap type.
    fn val_type(&mut self) -> anyhow::Result<()> {
        if matches!(self.byte()?, 0x63 | 0x64) {
            self.leb()?;
        }
        Ok(())
    }

    /// Table/memory limits: min, optional max, optional custom page size.
    fn limits(&mut self) -> anyhow::Result<()> {
        let flags = self.byte()?;
        self.leb()?;
        if flags & 0x01 != 0 {
            self.leb()?;
        }
        if flags & 0x08 != 0 {
            self.leb()?;
        }
        Ok(())
    }
}

/// The distinct module names of a core wasm module's imports, sorted.
///
/// Read straight from the binary's import section, so the check needs no
/// external disassembler.
fn import_modules(bytes: &[u8]) -> anyhow::Result<BTreeSet<String>> {
    let mut reader = Reader::new(bytes);
    ensure!(reader.take(4)? == b"\0asm", "not a wasm binary (bad magic)");
    let version = reader.take(4)?;
    ensure!(
        version == [1, 0, 0, 0],
        "not a core wasm module (version/layer bytes {version:02x?}; a component?)"
    );
    let mut modules = BTreeSet::new();
    while !reader.at_end() {
        let id = reader.byte()?;
        let len = reader.len()?;
        let payload = reader.take(len)?;
        if id != 2 {
            continue;
        }
        let mut section = Reader::new(payload);
        for _ in 0..section.leb()? {
            modules.insert(section.name()?.to_owned());
            section.name()?;
            match section.byte()? {
                0x00 => {
                    section.leb()?;
                }
                0x01 => {
                    section.val_type()?;
                    section.limits()?;
                }
                0x02 => section.limits()?,
                0x03 => {
                    section.val_type()?;
                    section.byte()?;
                }
                0x04 => {
                    section.byte()?;
                    section.leb()?;
                }
                kind => bail!("unknown import kind 0x{kind:02x}"),
            }
        }
        ensure!(section.at_end(), "import section has trailing bytes");
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

    #[test]
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

    #[test]
    fn a_module_with_no_imports_has_none() {
        assert!(
            import_modules(b"\0asm\x01\0\0\0")
                .expect("valid")
                .is_empty()
        );
    }

    #[test]
    fn malformed_binaries_are_errors_not_passes() {
        let mut truncated = module();
        truncated.truncate(truncated.len() - 20);
        assert!(import_modules(&truncated).is_err());
        assert!(import_modules(b"\0asm\x0d\0\x01\0").is_err(), "component");
        assert!(import_modules(b"MZ\x90\0\x03\0\0\0").is_err(), "bad magic");
    }

    #[test]
    fn committed_allowlist_admits_wasm_bindgen_and_refuses_env() {
        let allowlist = read(ALLOWLIST).expect("allowlist is committed");
        let allowed: BTreeSet<&str> = allowlist.lines().map(str::trim).collect();
        assert!(allowed.contains("__wbindgen_placeholder__"));
        assert!(allowed.contains("__wbindgen_externref_xform__"));
        assert!(!allowed.contains("env"));
    }

    #[test]
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
    fn locked_version_takes_the_first_matching_package() {
        let lock = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"b\"\nversion = \"0.2.1\"\n\n[[package]]\nname = \"b\"\nversion = \"0.3.0\"\n";
        assert_eq!(
            locked_package_version(lock, "b")
                .expect("parses")
                .as_deref(),
            Some("0.2.1")
        );
        assert_eq!(locked_package_version(lock, "c").expect("parses"), None);
    }
}
