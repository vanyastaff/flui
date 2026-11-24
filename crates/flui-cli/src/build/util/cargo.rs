//! Cargo owns package discovery and artifact locations; this module consumes its protocol.
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use cargo_metadata::{CrateType, Message, Metadata, PackageId, TargetKind};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::build::error::{BuildError, BuildResult};
use crate::build::framework::FrameworkDependency;
use crate::build::platform::{BuildUnit, BuiltPackage};

pub(crate) struct CargoTarget {
    package: PackageId,
    package_name: String,
    name: String,
    kind: TargetKind,
    crate_type: CrateType,
    version: String,
    framework: FrameworkDependency,
}

impl CargoTarget {
    /// The dependency through which the package reaches FLUI's build
    /// switches.
    pub(crate) fn framework(&self) -> &FrameworkDependency {
        &self.framework
    }

    /// The workspace member the build compiles, and whether its target
    /// links the member's dev-dependencies (an example's does).
    pub(crate) fn built_package(&self) -> BuiltPackage {
        BuiltPackage {
            name: self.package_name.clone(),
            version: self.version.clone(),
            links_dev_dependencies: self.kind == TargetKind::Example,
        }
    }
    /// The package id of the workspace member the build compiles.
    pub(crate) fn package_id(&self) -> &PackageId {
        &self.package
    }

    /// The arguments that select this target in a `cargo` command. The
    /// package is named by its id, which a dependency that shares its name
    /// cannot make ambiguous ("multiple packages match").
    pub(crate) fn cargo_args(&self) -> Vec<String> {
        let mut args = vec![
            "--package".into(),
            self.package.repr.clone(),
            if self.crate_type == CrateType::StaticLib {
                "--lib"
            } else if self.kind == TargetKind::Example {
                "--example"
            } else {
                "--bin"
            }
            .into(),
        ];
        if self.crate_type != CrateType::StaticLib {
            args.push(self.name.clone());
        }
        args
    }
}

fn invalid(reason: impl Into<String>) -> BuildError {
    BuildError::invalid_config("cargo target", reason.into())
}

fn command_error(command: &str, error: impl std::fmt::Display) -> BuildError {
    BuildError::CommandFailed {
        command: command.into(),
        exit_code: -1,
        stderr: error.to_string(),
    }
}

/// What `cargo metadata` says about the project at `dir`.
pub(crate) struct CargoProject {
    /// The directory cargo builds the project into: whichever of
    /// `CARGO_TARGET_DIR`, `build.target-dir` and the enclosing workspace's
    /// `target/` cargo itself would use.
    pub(crate) target_dir: PathBuf,
    /// The package whose manifest is in `dir`, not a package of an
    /// enclosing workspace; `None` for a virtual workspace.
    pub(crate) package: Option<String>,
    /// The file stem of that package's library target, as an Android build
    /// lands it in `lib<stem>.so` (`-` as `_`), when it has one. The crate
    /// types the manifest declares do not matter: that build asks for the
    /// `cdylib` itself.
    pub(crate) library: Option<String>,
    /// The dependency through which that package reaches FLUI's build
    /// switches; [`FrameworkDependency::Neither`] for a virtual workspace.
    pub(crate) framework: FrameworkDependency,
}

/// Ask `cargo metadata` (without dependencies) about the project at `dir`.
///
/// # Errors
///
/// Returns the failure of `cargo metadata`, such as a missing or invalid
/// manifest.
pub(crate) fn cargo_project(dir: &Path) -> BuildResult<CargoProject> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(dir)
        .no_deps()
        .exec()
        .map_err(|error| command_error("cargo metadata", error))?;
    let here = std::fs::canonicalize(dir).ok();
    let package = metadata.packages.iter().find(|package| {
        here.is_some()
            && package
                .manifest_path
                .parent()
                .and_then(|dir| std::fs::canonicalize(dir).ok())
                == here
    });
    let library = package.and_then(|package| {
        package
            .targets
            .iter()
            // The kind, not the crate types: an `[[example]]` may declare
            // `crate-type = ["lib"]`, and its kind stays `example`.
            .find(|target| {
                target.kind.iter().any(|kind| {
                    matches!(
                        kind,
                        TargetKind::Lib
                            | TargetKind::RLib
                            | TargetKind::DyLib
                            | TargetKind::CDyLib
                            | TargetKind::StaticLib
                    )
                })
            })
            .map(|target| target.name.replace('-', "_"))
    });
    Ok(CargoProject {
        target_dir: metadata.target_directory.into_std_path_buf(),
        package: package.map(|package| package.name.to_string()),
        library,
        framework: package.map_or(FrameworkDependency::Neither, |package| {
            FrameworkDependency::of(&package.dependencies)
        }),
    })
}

/// The directory cargo builds the project at `dir` into: the
/// [`CargoProject::target_dir`] of [`cargo_project`].
///
/// # Errors
///
/// Returns the failure of `cargo metadata`, such as a missing or invalid
/// manifest.
pub(crate) fn target_directory(dir: &Path) -> BuildResult<PathBuf> {
    cargo_project(dir).map(|project| project.target_dir)
}

/// The directory inside a target-dir that cargo names after `profile`: `dev`
/// and `test` build into `debug`, `bench` into `release`, and any other
/// profile, `release` included, into its own name.
#[must_use]
pub(crate) fn profile_dir(profile: &str) -> &str {
    match profile {
        "dev" | "test" => "debug",
        "bench" => "release",
        other => other,
    }
}

async fn cargo_output(dir: &Path, args: &[&str]) -> BuildResult<Vec<u8>> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| command_error("cargo metadata", error))?;
    if !output.status.success() {
        return Err(BuildError::CommandFailed {
            command: format!("cargo {}", args.join(" ")),
            exit_code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy_owned(output.stderr),
        });
    }
    Ok(output.stdout)
}

pub(crate) async fn select_target(dir: &Path, unit: &BuildUnit) -> BuildResult<CargoTarget> {
    if matches!(unit, BuildUnit::Library { .. }) {
        return Err(invalid(
            "library selection is supported only for iOS library delivery",
        ));
    }
    select_target_for(dir, unit, false).await
}

pub(crate) async fn select_static_library(
    dir: &Path,
    unit: &BuildUnit,
) -> BuildResult<CargoTarget> {
    select_target_for(dir, unit, true).await
}

async fn select_target_for(
    dir: &Path,
    unit: &BuildUnit,
    staticlib: bool,
) -> BuildResult<CargoTarget> {
    let located: serde_json::Value = serde_json::from_slice(
        &cargo_output(dir, &["locate-project", "--message-format=json"]).await?,
    )
    .map_err(|error| invalid(format!("invalid Cargo project location: {error}")))?;
    let manifest = located
        .get("root")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid("Cargo returned no project manifest"))?;
    let manifest = Path::new(manifest).canonicalize()?;
    let metadata: Metadata = serde_json::from_slice(
        &cargo_output(dir, &["metadata", "--format-version=1", "--no-deps"]).await?,
    )
    .map_err(|error| invalid(format!("invalid Cargo metadata: {error}")))?;
    let packages: Vec<_> = match unit {
        BuildUnit::Package(name)
        | BuildUnit::Library {
            package: Some(name),
        } => metadata
            .packages
            .iter()
            .filter(|package| {
                metadata.workspace_members.contains(&package.id) && package.name.as_str() == name
            })
            .collect(),
        // What cargo builds without `--package`: at the workspace's root,
        // virtual or a package itself, its default members (the root
        // package alone unless `default-members` names others); in a
        // member, that member.
        _ => {
            let at_root = metadata
                .workspace_root
                .join("Cargo.toml")
                .as_std_path()
                .canonicalize()
                .is_ok_and(|root| root == manifest);
            match metadata.packages.iter().find(|package| {
                package
                    .manifest_path
                    .as_std_path()
                    .canonicalize()
                    .is_ok_and(|path| path == manifest)
            }) {
                Some(package) if !at_root || !metadata.workspace_default_members.is_available() => {
                    vec![package]
                }
                _ => metadata
                    .packages
                    .iter()
                    .filter(|package| metadata.workspace_default_members.contains(&package.id))
                    .collect(),
            }
        }
    };
    if packages.is_empty() {
        return Err(invalid(format!("no package matches {unit:?}")));
    }
    // `cargo run` without `--bin`: when exactly one selected package names a
    // `default-run`, the binaries of that name; otherwise every binary of
    // every selected package, each `default-run` ignored (cargo
    // `src/bin/cargo/commands/run.rs`, `CompileFilter::single_bin`).
    let default_run = match packages
        .iter()
        .filter_map(|package| package.default_run.as_deref())
        .collect::<Vec<&str>>()
        .as_slice()
    {
        [name] => Some((*name).to_owned()),
        _ => None,
    };
    let mut candidates = Vec::new();
    for package in packages {
        let kind = if staticlib {
            TargetKind::StaticLib
        } else if matches!(unit, BuildUnit::Example(_)) {
            TargetKind::Example
        } else {
            TargetKind::Bin
        };
        let targets: Vec<_> = package
            .targets
            .iter()
            .filter(|target| {
                target.kind.contains(&kind)
                    && target.crate_types.contains(if staticlib {
                        &CrateType::StaticLib
                    } else {
                        &CrateType::Bin
                    })
            })
            .filter(|target| {
                staticlib
                    || match unit {
                        BuildUnit::Example(name) => target.name == *name,
                        _ => default_run.as_ref().is_none_or(|name| target.name == *name),
                    }
            })
            .collect();
        for target in targets {
            candidates.push(CargoTarget {
                package: package.id.clone(),
                package_name: package.name.to_string(),
                version: package.version.to_string(),
                framework: FrameworkDependency::of(&package.dependencies),
                name: target.name.clone(),
                kind: kind.clone(),
                crate_type: if staticlib {
                    CrateType::StaticLib
                } else {
                    CrateType::Bin
                },
            });
        }
    }
    if candidates.len() != 1 {
        let names = candidates
            .iter()
            .map(|target| format!("{}::{}", target.package_name, target.name))
            .collect::<Vec<_>>()
            .join(", ");
        let artifact = if staticlib {
            "static library"
        } else {
            "executable"
        };
        let advice = if staticlib {
            "select one package whose [lib] crate-type includes 'staticlib'"
        } else {
            "select a package/example or set package.default-run"
        };
        return Err(invalid(format!(
            "expected one {artifact} for {unit:?}, found {} ({names}); {advice}",
            candidates.len()
        )));
    }
    candidates
        .pop()
        .ok_or_else(|| invalid("Cargo target selection produced no target"))
}

/// Unknown messages and non-JSON output are permitted by Cargo's public protocol.
fn parse_message(line: &str) -> BuildResult<Option<Message>> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        writeln!(std::io::stderr().lock(), "{line}")?;
        return Ok(None);
    };
    if !matches!(
        value.get("reason").and_then(serde_json::Value::as_str),
        Some("compiler-artifact" | "compiler-message" | "build-script-executed" | "build-finished")
    ) {
        return Ok(None);
    }
    serde_json::from_value(value)
        .map(Some)
        .map_err(|error| invalid(format!("invalid Cargo build message: {error}")))
}

async fn stop_child(child: &mut Child) -> BuildResult<()> {
    // Explicit error paths reap the process. Future cancellation instead relies on
    // Tokio's kill_on_drop, whose reaping is best-effort rather than synchronous.
    child
        .kill()
        .await
        .map_err(|error| command_error("stopping cargo", error))
}

async fn collect_artifacts(child: &mut Child, target: &CargoTarget) -> BuildResult<PathBuf> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| invalid("Cargo stdout was not piped"))?;
    let mut lines = BufReader::new(stdout).lines();
    let mut executable = None;
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| command_error("reading Cargo output", error))?
    {
        match parse_message(&line)? {
            Some(Message::CompilerArtifact(artifact))
                if artifact.package_id == target.package
                    && artifact.target.name == target.name
                    && artifact.target.kind.contains(&target.kind)
                    && artifact.target.crate_types.contains(&target.crate_type)
                    && !artifact.profile.test =>
            {
                let paths: Vec<_> = if target.crate_type == CrateType::StaticLib {
                    artifact
                        .filenames
                        .into_iter()
                        .filter(|path| matches!(path.extension(), Some("a" | "lib")))
                        .collect()
                } else {
                    artifact.executable.into_iter().collect()
                };
                for path in paths {
                    let path = path.into_std_path_buf();
                    if executable
                        .as_ref()
                        .is_some_and(|previous| previous != &path)
                    {
                        return Err(invalid("Cargo reported conflicting artifact paths"));
                    }
                    executable = Some(path);
                }
            }
            Some(Message::CompilerMessage(message)) => {
                if let Some(rendered) = message.message.rendered {
                    writeln!(std::io::stderr().lock(), "{rendered}")?;
                }
            }
            _ => {}
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|error| command_error("waiting for cargo build", error))?;
    if !status.success() {
        return Err(BuildError::CommandFailed {
            command: "cargo build".into(),
            exit_code: status.code().unwrap_or(-1),
            stderr: "Cargo compilation failed; see compiler diagnostics above".into(),
        });
    }
    let path = executable.ok_or_else(|| {
        invalid(format!(
            "Cargo produced no requested artifact for {}::{}",
            target.package_name, target.name
        ))
    })?;
    if !path.is_file() {
        return Err(BuildError::path_not_found(
            path,
            "Cargo's reported artifact is absent",
        ));
    }
    Ok(path)
}

/// Runs `cargo` with `args` and `env` in `dir` and answers the artifact of
/// `target` it reports.
pub(crate) async fn build_artifact(
    dir: &Path,
    args: &[String],
    target: &CargoTarget,
    env: &[(&str, String)],
) -> BuildResult<PathBuf> {
    let mut child = Command::new("cargo")
        .args(args)
        .envs(env.iter().map(|(name, value)| (*name, value.as_str())))
        .arg("--message-format=json-render-diagnostics")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| command_error("cargo build", error))?;
    finish_child(&mut child, target).await
}

async fn finish_child(child: &mut Child, target: &CargoTarget) -> BuildResult<PathBuf> {
    let result = collect_artifacts(child, target).await;
    if result.is_err()
        && child
            .try_wait()
            .map_err(|error| command_error("querying cargo", error))?
            .is_none()
    {
        stop_child(child).await?;
    }
    result
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::select_target;
    use crate::build::platform::BuildUnit;

    /// Write `files`, each `(path, contents)`, under `root`.
    fn write(root: &Path, files: &[(&str, &str)]) {
        for (path, contents) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent directory"))
                .expect("the directories");
            std::fs::write(path, contents).expect("a fixture file");
        }
    }

    /// The binary selected without `--package` or `--bin` at `dir`, as
    /// `<package>::<bin>`, or the refusal.
    fn selected(dir: &Path) -> Result<String, String> {
        select(dir)
            .map(|target| format!("{}::{}", target.package_name, target.name))
            .map_err(|error| error.to_string())
    }

    fn select(dir: &Path) -> crate::build::error::BuildResult<super::CargoTarget> {
        tokio::runtime::Runtime::new()
            .expect("a runtime")
            .block_on(select_target(dir, &BuildUnit::DefaultBinary))
    }

    /// The arguments that select a member name it so that a dependency of
    /// the same name does not make cargo's `--package` ambiguous.
    #[test]
    fn a_member_is_named_apart_from_a_dependency_of_its_name() {
        let fixture = tempfile::tempdir().expect("a temporary directory");
        let package = |version: &str, rest: &str| {
            format!(
                "[package]\nname = \"app\"\nversion = \"{version}\"\nedition = \"2024\"\n{rest}"
            )
        };
        write(
            fixture.path(),
            &[
                (
                    "ws/Cargo.toml",
                    &package(
                        "0.1.0",
                        "[workspace]\n[dependencies]\nold = { package = \"app\", path = \"../old\" }\n",
                    ),
                ),
                ("ws/src/main.rs", "fn main() {}\n"),
                ("old/Cargo.toml", &package("0.2.0", "")),
                ("old/src/lib.rs", ""),
            ],
        );
        let ws = fixture.path().join("ws");
        let args = select(&ws).expect("the member's binary").cargo_args();
        let [flag, spec, ..] = args.as_slice() else {
            panic!("--package and its value: {args:?}");
        };
        assert_eq!(flag, "--package");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = std::process::Command::new(cargo)
            .current_dir(&ws)
            .env("CARGO_NET_OFFLINE", "true")
            .args(["tree", "--depth", "0", "--package", spec])
            .output()
            .expect("cargo tree");
        assert!(
            output.status.success(),
            "`--package {spec}` names one package: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// The binary `cargo run` runs without `--package` or `--bin`: at a
    /// workspace root that is a package itself, of its default members, not
    /// of the root package; among several default members, the one
    /// `default-run` names when one of them names one, though another member
    /// has a binary of its own; with no `default-run`, one binary or a
    /// refusal naming them.
    #[test]
    fn a_run_without_a_package_selects_the_binary_cargo_runs() {
        let fixture = tempfile::tempdir().expect("a temporary directory");
        let package = |name: &str, rest: &str| {
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{rest}")
        };
        let root = fixture.path().join("root");
        write(
            &root,
            &[
                (
                    "Cargo.toml",
                    &package(
                        "shell",
                        "[workspace]\nmembers = [\"app\"]\ndefault-members = [\"app\"]\n",
                    ),
                ),
                ("src/main.rs", "fn main() {}\n"),
                ("app/Cargo.toml", &package("app", "")),
                ("app/src/main.rs", "fn main() {}\n"),
            ],
        );
        assert_eq!(
            selected(&root),
            Ok("app::app".to_owned()),
            "a root package's default member"
        );

        let members = fixture.path().join("members");
        write(
            &members,
            &[
                (
                    "Cargo.toml",
                    "[workspace]\nresolver = \"3\"\nmembers = [\"a\", \"c\"]\n",
                ),
                ("a/Cargo.toml", &package("a", "default-run = \"a\"\n")),
                ("a/src/main.rs", "fn main() {}\n"),
                ("a/src/bin/a2.rs", "fn main() {}\n"),
                ("c/Cargo.toml", &package("c", "")),
                ("c/src/main.rs", "fn main() {}\n"),
            ],
        );
        assert_eq!(
            selected(&members),
            Ok("a::a".to_owned()),
            "the one `default-run` among the default members"
        );
        write(&members, &[("a/Cargo.toml", &package("a", ""))]);
        let refused = selected(&members);
        assert!(
            refused
                .as_ref()
                .is_err_and(|error| error.contains("expected one executable")),
            "{refused:?}"
        );
    }

    /// The library an Android build compiles is the package's `[lib]`,
    /// whatever crate types it declares, and nothing else is taken for it.
    #[test]
    fn the_library_is_the_lib_target_whatever_its_crate_types() {
        // (case, manifest tail, whether `src/lib.rs` exists, the library)
        let cases = [
            ("no [lib] table", "", true, Some("my_app")),
            (
                "an rlib and a cdylib",
                "[lib]\ncrate-type = [\"rlib\", \"cdylib\"]\n",
                true,
                Some("my_app"),
            ),
            (
                "a staticlib",
                "[lib]\ncrate-type = [\"staticlib\"]\n",
                true,
                Some("my_app"),
            ),
            (
                "a renamed library",
                "[lib]\nname = \"core_ui\"\n",
                true,
                Some("core_ui"),
            ),
            ("a binary only", "", false, None),
            (
                "a library-typed example and no library",
                "[[example]]\nname = \"demo\"\ncrate-type = [\"lib\"]\n",
                false,
                None,
            ),
        ];
        for (case, manifest, has_library, expected) in cases {
            let fixture = tempfile::tempdir().expect("a temporary directory");
            let manifest = format!(
                "[package]\nname = \"my-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{manifest}"
            );
            let mut files = vec![
                ("Cargo.toml", manifest.as_str()),
                ("src/main.rs", "fn main() {}\n"),
                ("examples/demo.rs", ""),
            ];
            if has_library {
                files.push(("src/lib.rs", ""));
            }
            write(fixture.path(), &files);
            let project = super::cargo_project(fixture.path()).expect("cargo metadata");
            assert_eq!(project.library.as_deref(), expected, "{case}");
            assert_eq!(project.package.as_deref(), Some("my-app"), "{case}");
        }
    }
}
