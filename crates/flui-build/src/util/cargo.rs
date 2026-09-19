//! Cargo owns package discovery and artifact locations; this module consumes its protocol.
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use cargo_metadata::{CrateType, Message, Metadata, PackageId, TargetKind};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::error::{BuildError, BuildResult};
use crate::platform::BuildUnit;

pub(crate) struct ExecutableTarget {
    package: PackageId,
    package_name: String,
    name: String,
    kind: TargetKind,
}

impl ExecutableTarget {
    pub(crate) fn cargo_args(&self) -> Vec<String> {
        vec![
            "--package".into(),
            self.package_name.clone(),
            if self.kind == TargetKind::Example {
                "--example"
            } else {
                "--bin"
            }
            .into(),
            self.name.clone(),
        ]
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
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(output.stdout)
}

pub(crate) async fn select_target(dir: &Path, unit: &BuildUnit) -> BuildResult<ExecutableTarget> {
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
        BuildUnit::Package(name) => metadata
            .packages
            .iter()
            .filter(|package| {
                metadata.workspace_members.contains(&package.id) && package.name.as_str() == name
            })
            .collect(),
        _ => {
            if let Some(package) = metadata.packages.iter().find(|package| {
                package
                    .manifest_path
                    .as_std_path()
                    .canonicalize()
                    .is_ok_and(|path| path == manifest)
            }) {
                vec![package]
            } else {
                metadata
                    .packages
                    .iter()
                    .filter(|package| metadata.workspace_default_members.contains(&package.id))
                    .collect()
            }
        }
    };
    if packages.is_empty() {
        return Err(invalid(format!("no package matches {unit:?}")));
    }
    let mut candidates = Vec::new();
    for package in packages {
        let kind = if matches!(unit, BuildUnit::Example(_)) {
            TargetKind::Example
        } else {
            TargetKind::Bin
        };
        let targets: Vec<_> = package
            .targets
            .iter()
            .filter(|target| {
                target.kind.contains(&kind) && target.crate_types.contains(&CrateType::Bin)
            })
            .filter(|target| match unit {
                BuildUnit::Example(name) => target.name == *name,
                _ => package
                    .default_run
                    .as_ref()
                    .is_none_or(|name| target.name == *name),
            })
            .collect();
        for target in targets {
            candidates.push(ExecutableTarget {
                package: package.id.clone(),
                package_name: package.name.to_string(),
                name: target.name.clone(),
                kind: kind.clone(),
            });
        }
    }
    if candidates.len() != 1 {
        let names = candidates
            .iter()
            .map(|target| format!("{}::{}", target.package_name, target.name))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(invalid(format!(
            "expected one executable for {unit:?}, found {} ({names}); select a package/example or set package.default-run",
            candidates.len()
        )));
    }
    candidates
        .pop()
        .ok_or_else(|| invalid("Cargo target selection produced no executable"))
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

async fn collect_artifacts(child: &mut Child, target: &ExecutableTarget) -> BuildResult<PathBuf> {
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
                    && !artifact.profile.test =>
            {
                if let Some(path) = artifact.executable {
                    let path = path.into_std_path_buf();
                    if executable
                        .as_ref()
                        .is_some_and(|previous| previous != &path)
                    {
                        return Err(invalid("Cargo reported conflicting executable paths"));
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
            "Cargo produced no executable for {}::{}",
            target.package_name, target.name
        ))
    })?;
    if !path.is_file() {
        return Err(BuildError::path_not_found(
            path,
            "Cargo's reported executable is absent",
        ));
    }
    Ok(path)
}

pub(crate) async fn build_executable(
    dir: &Path,
    args: &[String],
    target: &ExecutableTarget,
) -> BuildResult<PathBuf> {
    let mut child = Command::new("cargo")
        .args(args)
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

async fn finish_child(child: &mut Child, target: &ExecutableTarget) -> BuildResult<PathBuf> {
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
    use super::*;

    #[test]
    fn protocol_accepts_extensions_but_rejects_broken_known_messages() {
        assert!(
            parse_message("ordinary tool output")
                .expect("plain output")
                .is_none()
        );
        assert!(
            parse_message(r#"{"reason":"future-cargo-message","new":true}"#)
                .expect("forward compatibility")
                .is_none()
        );
        assert!(parse_message(r#"{"reason":"compiler-artifact"}"#).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn invalid_protocol_and_read_errors_kill_and_reap_the_child() {
        let target = ExecutableTarget {
            package: serde_json::from_str(r#""fixture-id""#).expect("opaque package id"),
            package_name: "fixture".into(),
            name: "app".into(),
            kind: TargetKind::Bin,
        };
        for script in [
            "printf '%s\n' '{\"reason\":\"compiler-artifact\"}'; exec sleep 60",
            "printf '\\377\\n'; exec sleep 60",
        ] {
            let mut child = Command::new("sh")
                .args(["-c", script])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .spawn()
                .expect("fixture child");
            let error = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                finish_child(&mut child, &target),
            )
            .await
            .expect("cleanup must not await the child's sleep")
            .expect_err("invalid protocol");
            assert!(error.to_string().contains("Cargo"), "{error}");
            assert!(
                child.try_wait().expect("wait status").is_some(),
                "child must be reaped on explicit error"
            );
        }
    }
}
