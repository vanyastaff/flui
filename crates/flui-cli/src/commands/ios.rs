//! Native simulator selection and one-shot application launch.
use crate::build::{AppBundle, BuildUnit, BuilderContextBuilder, IosBuilder, Platform, Profile};
use crate::error::{CliError, CliResult, ResultExt};
use crate::proc;
use std::{io, path::Path, process::Command, time::Duration};

fn invalid(message: impl Into<String>) -> CliError {
    CliError::Missing(message.into())
}
/// Run a simulator tool to completion within `limit`, returning its stdout.
///
/// The deadline covers the pipe drain too: a grandchild that inherited the
/// pipe (CoreSimulator services do) cannot hold the CLI past `limit`.
fn tool(program: &str, args: &[&str], limit: Duration) -> CliResult<Vec<u8>> {
    let output = match proc::output_with_timeout(Command::new(program).args(args), limit) {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::TimedOut => {
            return Err(invalid(format!("{program} {} timed out", args.join(" "))));
        }
        Err(error) => return Err(error).context("start simulator tool"),
    };
    if !output.status.success() {
        return Err(invalid(format!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(output.stdout)
}
fn simctl(args: &[&str], limit: Duration) -> CliResult<Vec<u8>> {
    let mut all = vec!["simctl"];
    all.extend_from_slice(args);
    tool("xcrun", &all, limit)
}
#[derive(Debug)]
pub(super) struct Simulator {
    pub(super) udid: String,
    pub(super) triple: String,
    version: String,
}
fn selection(json: &serde_json::Value, udid: &str) -> CliResult<(String, bool)> {
    let runtimes = json["runtimes"]
        .as_array()
        .ok_or_else(|| invalid("simctl returned no runtimes"))?;
    let devices = json["devices"]
        .as_object()
        .ok_or_else(|| invalid("simctl returned no devices"))?;
    let mut matches = Vec::new();
    for (runtime_id, entries) in devices {
        if !runtime_id.starts_with("com.apple.CoreSimulator.SimRuntime.iOS-") {
            continue;
        }
        let Some(runtime) = runtimes
            .iter()
            .find(|runtime| runtime["identifier"] == *runtime_id && runtime["isAvailable"] == true)
        else {
            continue;
        };
        for device in entries
            .as_array()
            .ok_or_else(|| invalid("invalid simulator list"))?
        {
            if device["udid"] == udid && device["isAvailable"] == true {
                let version = runtime["version"]
                    .as_str()
                    .ok_or_else(|| invalid("simulator runtime has no version"))?;
                numeric_version(version)?;
                matches.push((version.to_owned(), device["state"] == "Booted"));
            }
        }
    }
    if matches.len() != 1 {
        return Err(invalid(format!(
            "select an exact available iOS simulator UDID; {udid} matched {} devices",
            matches.len()
        )));
    }
    matches
        .pop()
        .ok_or_else(|| invalid("missing selected simulator"))
}
pub(super) fn resolve_simulator(udid: &str) -> CliResult<Simulator> {
    if !cfg!(target_os = "macos") {
        return Err(invalid("iOS simulator commands require macOS and Xcode"));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&simctl(&["list", "--json"], Duration::from_secs(30))?)
            .context("decode simulator inventory")?;
    let (version, booted) = selection(&json, udid)?;
    if !booted {
        simctl(&["boot", udid], Duration::from_secs(30))?;
    }
    simctl(&["bootstatus", udid, "-b"], Duration::from_secs(180))?;
    let architecture = simctl(
        &["getenv", udid, "SIMULATOR_ARCHS"],
        Duration::from_secs(30),
    )?;
    let runtime = json["runtimes"]
        .as_array()
        .and_then(|runtimes| {
            runtimes.iter().find(|runtime| {
                json["devices"][runtime["identifier"].as_str().unwrap_or("")]
                    .as_array()
                    .is_some_and(|devices| devices.iter().any(|device| device["udid"] == udid))
            })
        })
        .ok_or_else(|| invalid("selected runtime disappeared from inventory"))?;
    let triple = architecture_triple(&architecture, runtime.get("supportedArchitectures"))?;
    Ok(Simulator {
        udid: udid.into(),
        triple: triple.into(),
        version,
    })
}
fn architecture_triple(
    bytes: &[u8],
    supported: Option<&serde_json::Value>,
) -> CliResult<&'static str> {
    let architecture = std::str::from_utf8(bytes)
        .map_err(|_| invalid("SIMULATOR_ARCHS is not UTF-8"))?
        .trim();
    let triple = match architecture {
        "arm64" => "aarch64-apple-ios-sim",
        "x86_64" => "x86_64-apple-ios",
        _ => {
            return Err(invalid(
                "SIMULATOR_ARCHS must contain exactly one supported architecture (arm64 or x86_64)",
            ));
        }
    };
    if let Some(supported) = supported {
        let supported = supported
            .as_array()
            .filter(|values| !values.is_empty() && values.iter().all(serde_json::Value::is_string))
            .ok_or_else(|| invalid("runtime supportedArchitectures metadata is malformed"))?;
        if !supported.iter().any(|value| value == architecture) {
            return Err(invalid(
                "selected device architecture conflicts with its runtime supportedArchitectures",
            ));
        }
    }
    Ok(triple)
}
pub(super) fn configured_bundle(root: &Path) -> CliResult<Option<AppBundle>> {
    let path = root.join("flui.toml");
    match std::fs::symlink_metadata(&path) {
        Ok(_) => {
            let config = crate::config::FluiConfig::load_from(&path)?;
            let mut bundle = AppBundle::new(&config.app.name, &config.app.organization);
            bundle.version = config.app.version;
            Ok(Some(bundle))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(CliError::context(error, "read application config")),
    }
}
fn plist(app: &Path) -> CliResult<serde_json::Value> {
    let file = app.join("Info.plist");
    let file = file
        .to_str()
        .ok_or_else(|| invalid("bundle plist path must be UTF-8"))?;
    serde_json::from_slice(&tool(
        "plutil",
        &["-convert", "json", "-o", "-", file],
        Duration::from_secs(30),
    )?)
    .context("decode application plist")
}
fn numeric_version(value: &str) -> CliResult<[u32; 3]> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 {
        return Err(invalid("invalid numeric OS version"));
    }
    let mut parsed = [0; 3];
    for (index, part) in parts.iter().enumerate() {
        parsed[index] = part
            .parse()
            .map_err(|_| invalid("invalid numeric OS version"))?;
    }
    Ok(parsed)
}
pub(super) fn check_runtime(simulator: &Simulator, app: &Path) -> CliResult<()> {
    let info = plist(app)?;
    let minimum = info["MinimumOSVersion"]
        .as_str()
        .ok_or_else(|| invalid("application is missing deployment minimum"))?;
    if numeric_version(&simulator.version)? < numeric_version(minimum)? {
        return Err(invalid(format!(
            "simulator runtime {} is older than application minimum {minimum}",
            simulator.version
        )));
    }
    Ok(())
}
pub(super) fn run(udid: &str, release: bool, profile: Option<&str>) -> CliResult<()> {
    if profile.is_some_and(|profile| profile != "debug" && profile != "release") {
        return Err(invalid("iOS run supports debug/release profiles only"));
    }
    let simulator = resolve_simulator(udid)?;
    let root = std::env::current_dir()?;
    let bundle = configured_bundle(&root)?;
    let mut context = BuilderContextBuilder::new(root)
        .with_platform(Platform::Ios {
            targets: vec![simulator.triple.clone()],
        })
        .with_target(BuildUnit::DefaultBinary)
        .with_profile(if release || profile == Some("release") {
            Profile::Release
        } else {
            Profile::Debug
        });
    if let Some(bundle) = bundle {
        context = context.with_bundle(bundle);
    }
    let context = context.build();
    let runtime = tokio::runtime::Runtime::new().context("start build runtime")?;
    let builder = IosBuilder::new();
    let delivered = runtime
        .block_on(async {
            let artifacts = builder.build_rust(&context).await?;
            builder.build_platform(&context, &artifacts).await
        })
        .context("build iOS application")?;
    check_runtime(&simulator, &delivered.app_binary)?;
    let info = plist(&delivered.app_binary)?;
    let identifier = info["CFBundleIdentifier"]
        .as_str()
        .ok_or_else(|| invalid("missing bundle identifier"))?;
    let output = install_and_launch(
        &simulator.udid,
        &delivered.app_binary,
        identifier,
        simctl,
        plist,
    )?;
    crate::ui::success(format!(
        "Launched {identifier} on {}: {}",
        simulator.udid,
        String::from_utf8_lossy(&output).trim()
    ))?;
    crate::ui::outro(
        "iOS application launched; simulator runs are one-shot (no desktop hot-reload watcher)",
    )?;
    Ok(())
}

fn install_and_launch(
    udid: &str,
    app: &Path,
    identifier: &str,
    mut command: impl FnMut(&[&str], Duration) -> CliResult<Vec<u8>>,
    inspect: impl Fn(&Path) -> CliResult<serde_json::Value>,
) -> CliResult<Vec<u8>> {
    let app = app
        .to_str()
        .ok_or_else(|| invalid("application path must be UTF-8"))?;
    command(&["install", udid, app], Duration::from_secs(120))?;
    let installed = command(
        &["get_app_container", udid, identifier, "app"],
        Duration::from_secs(30),
    )?;
    let installed = std::str::from_utf8(&installed)
        .map_err(|_| invalid("installed application path is not UTF-8"))?
        .trim();
    if installed.is_empty() || inspect(Path::new(installed))?["CFBundleIdentifier"] != identifier {
        return Err(invalid(
            "installed application identity does not match the staged bundle",
        ));
    }
    command(&["launch", udid, identifier], Duration::from_secs(30))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installation_identity_precedes_exact_device_launch() {
        for matches in [true, false] {
            let mut calls = Vec::new();
            let result = install_and_launch(
                "exact-udid",
                Path::new("/staged.app"),
                "dev.test.app",
                |args, _| {
                    calls.push(args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>());
                    Ok(if args[0] == "get_app_container" {
                        b"/installed.app\n".to_vec()
                    } else {
                        Vec::new()
                    })
                },
                |path| {
                    assert_eq!(path, Path::new("/installed.app"));
                    Ok(
                        serde_json::json!({"CFBundleIdentifier": if matches { "dev.test.app" } else { "wrong.id" }}),
                    )
                },
            );
            assert_eq!(result.is_ok(), matches);
            assert_eq!(calls[0], ["install", "exact-udid", "/staged.app"]);
            assert_eq!(
                calls[1],
                ["get_app_container", "exact-udid", "dev.test.app", "app"]
            );
            assert_eq!(calls.len(), if matches { 3 } else { 2 });
            if matches {
                assert_eq!(calls[2], ["launch", "exact-udid", "dev.test.app"]);
            }
        }
    }
    #[cfg(unix)]
    #[test]
    fn command_deadline_includes_inherited_output_pipes() {
        let start = std::time::Instant::now();
        let output = tool("sh", &["-c", "sleep 2 & exit 0"], Duration::from_millis(50))
            .expect("the tool itself exited 0");
        assert!(output.is_empty());
        assert!(start.elapsed() < Duration::from_secs(1));
        let error = tool("sh", &["-c", "sleep 2"], Duration::from_millis(50))
            .expect_err("a tool that never exits times out");
        assert!(error.to_string().contains("timed out"));
    }
    #[test]
    fn device_architecture_is_exact_and_runtime_consistent() {
        assert_eq!(
            architecture_triple(b"arm64\n", Some(&serde_json::json!(["arm64"])))
                .expect("architecture"),
            "aarch64-apple-ios-sim"
        );
        assert_eq!(
            architecture_triple(b"x86_64", None).expect("architecture"),
            "x86_64-apple-ios"
        );
        for bytes in [b"".as_slice(), b"arm64 x86_64", b"unknown", &[255]] {
            assert!(architecture_triple(bytes, None).is_err());
        }
        for metadata in [
            serde_json::json!(null),
            serde_json::json!([]),
            serde_json::json!([1]),
            serde_json::json!(["x86_64"]),
        ] {
            assert!(architecture_triple(b"arm64", Some(&metadata)).is_err());
        }
    }
    #[test]
    fn selection_requires_available_exact_ios_identity_and_runtime() {
        let json = serde_json::json!({"runtimes":[{"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-26-2","version":"26.2","isAvailable":true}], "devices":{"com.apple.CoreSimulator.SimRuntime.iOS-26-2":[{"udid":"chosen","state":"Booted","isAvailable":true},{"udid":"unavailable","isAvailable":false}], "com.apple.CoreSimulator.SimRuntime.tvOS-26-2":[{"udid":"tv","isAvailable":true}]}});
        assert_eq!(
            selection(&json, "chosen").expect("selected"),
            ("26.2".into(), true)
        );
        for id in ["", "booted", "unavailable", "tv", "other"] {
            assert!(selection(&json, id).is_err());
        }
        assert!(
            numeric_version("26.2").expect("runtime") > numeric_version("14.0").expect("minimum")
        );
    }
}
