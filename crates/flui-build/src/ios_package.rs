//! Native library delivery without a consumer Xcode project.
use crate::error::{BuildError, BuildResult};
use crate::{BuildArtifacts, BuilderContext, FinalArtifacts, Platform};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

const RUNNING: u8 = 0;
const CANCELLED: u8 = 1;
const COMMITTING: u8 = 2;
const TOOL_TIMEOUT: Duration = Duration::from_secs(300);

fn invalid(reason: impl Into<String>) -> BuildError {
    BuildError::invalid_config("iOS delivery", reason.into())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Variant {
    Device,
    Simulator,
}
impl Variant {
    fn name(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Simulator => "simulator",
        }
    }
}
struct Group {
    variant: Variant,
    inputs: Vec<(String, PathBuf)>,
}
fn plan(targets: &[String], paths: &[PathBuf]) -> BuildResult<Vec<Group>> {
    if targets.is_empty() || targets.len() != paths.len() {
        return Err(invalid(
            "requested triples must have exactly one archive each",
        ));
    }
    let mut groups: BTreeMap<Variant, Vec<(String, PathBuf)>> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (target, path) in targets.iter().zip(paths) {
        let (variant, arch) = match target.as_str() {
            "aarch64-apple-ios" => (Variant::Device, "arm64"),
            "aarch64-apple-ios-sim" => (Variant::Simulator, "arm64"),
            "x86_64-apple-ios" => (Variant::Simulator, "x86_64"),
            _ => return Err(invalid(format!("unsupported iOS library target {target}"))),
        };
        if !seen.insert((variant, arch)) {
            return Err(invalid("duplicate platform/architecture slice"));
        }
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(invalid("archive input must be a regular file"));
        }
        groups.entry(variant).or_default().push((arch.into(), path));
    }
    Ok(groups
        .into_iter()
        .map(|(variant, inputs)| Group { variant, inputs })
        .collect())
}

struct CancelOnDrop(Arc<AtomicU8>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(RUNNING, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub(crate) async fn package(
    ctx: &BuilderContext,
    artifacts: &BuildArtifacts,
) -> BuildResult<FinalArtifacts> {
    let Platform::IOS { targets } = &ctx.platform else {
        return Err(invalid("expected iOS targets"));
    };
    let groups = plan(targets, &artifacts.rust_libs)?;
    let output = ctx.output_dir.clone();
    let cancelled = Arc::new(AtomicU8::new(RUNNING));
    let _guard = CancelOnDrop(Arc::clone(&cancelled));
    // A dropped async waiter does not abort a running blocking operation. The
    // worker retains its child and scratch until termination is observed.
    tokio::task::spawn_blocking(move || build(groups, &output, cancelled))
        .await
        .map_err(|error| invalid(format!("packaging worker failed: {error}")))?
}

struct Work {
    scratch: Option<tempfile::TempDir>,
    child: Option<Child>,
    cancel: Arc<AtomicU8>,
    serial: usize,
}
impl Work {
    fn new(parent: &Path, cancel: Arc<AtomicU8>) -> BuildResult<Self> {
        Ok(Self {
            scratch: Some(
                tempfile::Builder::new()
                    .prefix(".flui-ios-")
                    .tempdir_in(parent)?,
            ),
            child: None,
            cancel,
            serial: 0,
        })
    }
    fn path(&self) -> &Path {
        self.scratch
            .as_ref()
            .expect("BUG: work owns scratch until preservation")
            .path()
    }
    fn preserve(&mut self) -> PathBuf {
        self.scratch
            .take()
            .expect("BUG: preserve scratch once")
            .keep()
    }
    fn check_cancel(&self) -> BuildResult<()> {
        if self.cancel.load(Ordering::Acquire) == CANCELLED {
            Err(invalid("packaging cancelled before publication"))
        } else {
            Ok(())
        }
    }
    fn stop(&mut self) -> BuildResult<()> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        if child.try_wait()?.is_none() {
            // Even a failed kill can race normal exit. Only successful wait
            // proves the child no longer accesses scratch.
            if let Err(error) = child.kill() {
                if child.try_wait()?.is_none() {
                    return Err(error.into());
                }
            } else {
                child.wait()?;
            }
        }
        self.child = None;
        Ok(())
    }
    fn command(&mut self, program: &str, args: &[OsString]) -> BuildResult<Vec<u8>> {
        self.command_with_timeout(program, args, TOOL_TIMEOUT)
    }
    fn command_with_timeout(
        &mut self,
        program: &str,
        args: &[OsString],
        timeout: Duration,
    ) -> BuildResult<Vec<u8>> {
        self.check_cancel()?;
        self.serial += 1;
        let out = self.path().join(format!("stdout-{}", self.serial));
        let err = self.path().join(format!("stderr-{}", self.serial));
        self.child = Some(
            Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(File::create(&out)?)
                .stderr(File::create(&err)?)
                .spawn()?,
        );
        let started = Instant::now();
        let status = loop {
            if let Some(status) = self
                .child
                .as_mut()
                .expect("BUG: child installed")
                .try_wait()?
            {
                break status;
            }
            if self.cancel.load(Ordering::Acquire) == CANCELLED || started.elapsed() >= timeout {
                self.stop()?;
                return Err(invalid(format!(
                    "{program} cancelled or exceeded tool deadline"
                )));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        self.child = None;
        if !status.success() {
            return Err(BuildError::CommandFailed {
                command: program.into(),
                exit_code: status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&fs::read(err)?).into_owned(),
            });
        }
        self.check_cancel()?;
        Ok(fs::read(out)?)
    }
}
impl Drop for Work {
    fn drop(&mut self) {
        if self.child.is_some() && self.stop().is_err() {
            let path = self.preserve();
            // Cleanup uncertainty must never delete files a surviving child can
            // still access, including when the worker itself unwinds.
            if let Err(payload) = std::panic::catch_unwind(|| {
                tracing::error!(path = %path.display(), "child termination unconfirmed; retained iOS packaging scratch");
            }) {
                std::mem::forget(payload);
            }
        }
    }
}
fn no_symlink_destination(path: &Path) -> BuildResult<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(invalid("output XCFramework must not be a symlink"))
        }
        Ok(meta) if !meta.is_dir() => {
            Err(invalid("existing XCFramework output must be a directory"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
fn build(groups: Vec<Group>, output: &Path, cancel: Arc<AtomicU8>) -> BuildResult<FinalArtifacts> {
    let destination = output.join("flui.xcframework");
    no_symlink_destination(&destination)?;
    fs::create_dir_all(output)?;
    let parent = output.canonicalize()?;
    let final_path = parent.join("flui.xcframework");
    let mut work = Work::new(&parent, cancel)?;
    let mut libraries = BTreeMap::new();
    for group in &groups {
        for (arch, input) in &group.inputs {
            let bytes = work.command(
                "xcrun",
                &["lipo".into(), "-archs".into(), input.as_os_str().into()],
            )?;
            let text = String::from_utf8(bytes).map_err(|error| invalid(error.to_string()))?;
            if text.split_whitespace().collect::<Vec<_>>() != [arch.as_str()] {
                return Err(invalid(format!(
                    "archive {} does not contain exactly {arch}",
                    input.display()
                )));
            }
        }
        let directory = work.path().join(group.variant.name());
        fs::create_dir(&directory)?;
        let library = directory.join("libflui.a");
        if group.inputs.len() == 1 {
            fs::copy(&group.inputs[0].1, &library)?;
        } else {
            let mut args: Vec<OsString> = vec!["lipo".into(), "-create".into()];
            args.extend(group.inputs.iter().map(|(_, path)| path.as_os_str().into()));
            args.extend(["-output".into(), library.as_os_str().into()]);
            work.command("xcrun", &args)?;
        }
        libraries.insert(group.variant, library);
    }
    let staged = work.path().join("flui.xcframework");
    let mut args: Vec<OsString> = vec!["-create-xcframework".into()];
    for path in libraries.values() {
        args.extend(["-library".into(), path.as_os_str().into()]);
    }
    args.extend(["-output".into(), staged.as_os_str().into()]);
    work.command("xcodebuild", &args)?;
    let plist = work.command(
        "plutil",
        &[
            "-convert".into(),
            "json".into(),
            "-o".into(),
            "-".into(),
            staged.join("Info.plist").into_os_string(),
        ],
    )?;
    let metadata = serde_json::from_slice(&plist)
        .map_err(|error| invalid(format!("invalid XCFramework plist: {error}")))?;
    validate(&staged, &metadata, &groups, &libraries)?;
    let size_bytes = size(&staged)?;
    commit(&mut work, &staged, &final_path)?;
    Ok(FinalArtifacts {
        app_binary: destination,
        size_bytes,
    })
}
fn commit(work: &mut Work, staged: &Path, final_path: &Path) -> BuildResult<()> {
    work.check_cancel()?;
    no_symlink_destination(final_path)?;
    work.cancel
        .compare_exchange(RUNNING, COMMITTING, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| invalid("packaging cancelled before publication"))?;
    // No await/cancellation point after commit wins.
    publish(work, staged, final_path, |from, to| fs::rename(from, to))
}
fn publish(
    work: &mut Work,
    staged: &Path,
    destination: &Path,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> BuildResult<()> {
    let backup = work.path().join("previous-output");
    let existed = destination.exists();
    if existed {
        rename(destination, &backup)?;
    }
    if let Err(error) = rename(staged, destination) {
        if existed && let Err(rollback) = rename(&backup, destination) {
            let retained = work.preserve();
            return Err(invalid(format!(
                "publication failed: {error}; rollback failed: {rollback}; previous output retained at {}",
                retained.join("previous-output").display()
            )));
        }
        return Err(error.into());
    }
    Ok(())
}
fn checked_child(root: &Path, value: &str) -> BuildResult<PathBuf> {
    let path = Path::new(value);
    if value.is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(invalid(
            "XCFramework reference must be a relative normal path",
        ));
    }
    let mut child = root.to_path_buf();
    for part in path.components() {
        child.push(part);
        if fs::symlink_metadata(&child)?.file_type().is_symlink() {
            return Err(invalid("XCFramework references a symlink"));
        }
    }
    Ok(child)
}
fn validate(
    root: &Path,
    metadata: &serde_json::Value,
    groups: &[Group],
    libraries: &BTreeMap<Variant, PathBuf>,
) -> BuildResult<()> {
    let entries = metadata["AvailableLibraries"]
        .as_array()
        .ok_or_else(|| invalid("missing XCFramework libraries"))?;
    if entries.len() != groups.len() {
        return Err(invalid("XCFramework variant count mismatch"));
    }
    let mut seen = BTreeSet::new();
    for entry in entries {
        if entry["SupportedPlatform"] != "ios" {
            return Err(invalid("XCFramework platform is not iOS"));
        }
        let variant = match entry.get("SupportedPlatformVariant") {
            None => Variant::Device,
            Some(value) if value == "simulator" => Variant::Simulator,
            _ => return Err(invalid("unsupported XCFramework variant")),
        };
        if !seen.insert(variant) {
            return Err(invalid("duplicate XCFramework variant"));
        }
        let group = groups
            .iter()
            .find(|group| group.variant == variant)
            .ok_or_else(|| invalid("unexpected XCFramework variant"))?;
        let arches = entry["SupportedArchitectures"]
            .as_array()
            .ok_or_else(|| invalid("missing architectures"))?;
        let actual: BTreeSet<_> = arches
            .iter()
            .map(|arch| arch.as_str().ok_or_else(|| invalid("invalid architecture")))
            .collect::<BuildResult<_>>()?;
        let expected: BTreeSet<_> = group.inputs.iter().map(|(arch, _)| arch.as_str()).collect();
        if actual != expected || actual.len() != arches.len() {
            return Err(invalid("XCFramework architectures mismatch"));
        }
        let id = entry["LibraryIdentifier"]
            .as_str()
            .ok_or_else(|| invalid("missing library identifier"))?;
        let name = entry["LibraryPath"]
            .as_str()
            .ok_or_else(|| invalid("missing library path"))?;
        let directory = checked_child(root, id)?;
        let file = checked_child(&directory, name)?;
        if !file.is_file() {
            return Err(invalid("XCFramework library is not a file"));
        }
        if !same_bytes(&file, &libraries[&variant])? {
            return Err(invalid(
                "XCFramework library does not match requested platform slice",
            ));
        }
    }
    Ok(())
}
fn same_bytes(a: &Path, b: &Path) -> BuildResult<bool> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    let mut a = BufReader::new(File::open(a)?);
    let mut b = BufReader::new(File::open(b)?);
    let mut left = [0; 8192];
    let mut right = [0; 8192];
    loop {
        let count = a.read(&mut left)?;
        b.read_exact(&mut right[..count])?;
        if left[..count] != right[..count] {
            return Ok(false);
        }
        if count == 0 {
            return Ok(true);
        }
    }
}
fn size(path: &Path) -> BuildResult<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("XCFramework output contains a symlink"));
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    fs::read_dir(path)?.try_fold(0u64, |total, entry| Ok(total + size(&entry?.path())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token() -> Arc<AtomicU8> {
        Arc::new(AtomicU8::new(RUNNING))
    }
    fn archive(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        fs::write(&path, name).expect("archive");
        path
    }
    #[test]
    fn slice_planning_rejects_missing_duplicate_and_unsupported_inputs() {
        let dir = tempfile::tempdir().expect("temp");
        let path = archive(dir.path(), "input.a");
        assert!(plan(&[], &[]).is_err());
        assert!(plan(&["aarch64-apple-ios".into()], &[]).is_err());
        assert!(
            plan(
                &["aarch64-apple-darwin".into()],
                std::slice::from_ref(&path)
            )
            .is_err()
        );
        assert!(
            plan(
                &["aarch64-apple-ios".into(), "aarch64-apple-ios".into()],
                &[path.clone(), path.clone()]
            )
            .is_err()
        );
        let groups = plan(
            &[
                "aarch64-apple-ios".into(),
                "aarch64-apple-ios-sim".into(),
                "x86_64-apple-ios".into(),
            ],
            &[path.clone(), path.clone(), path],
        )
        .expect("groups");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[1].inputs.len(), 2);
    }
    #[test]
    fn publication_rolls_back_and_retains_backup_when_rollback_fails() {
        for rollback_fails in [false, true] {
            let dir = tempfile::tempdir().expect("temp");
            let output = dir.path().join("flui.xcframework");
            fs::create_dir(&output).expect("old");
            fs::write(output.join("sentinel"), "old").expect("sentinel");
            let mut work = Work::new(dir.path(), token()).expect("work");
            let scratch = work.path().to_path_buf();
            let staged = scratch.join("new");
            fs::create_dir(&staged).expect("new");
            let mut calls = 0;
            let error = publish(&mut work, &staged, &output, |from, to| {
                calls += 1;
                if calls == 2 || (calls == 3 && rollback_fails) {
                    Err(std::io::Error::other("injected rename failure"))
                } else {
                    fs::rename(from, to)
                }
            })
            .expect_err("publication failure");
            drop(work);
            if rollback_fails {
                assert!(error.to_string().contains(scratch.to_str().expect("path")));
                assert_eq!(
                    fs::read(scratch.join("previous-output/sentinel")).expect("retained backup"),
                    b"old"
                );
                fs::remove_dir_all(scratch).expect("test cleanup");
            } else {
                assert_eq!(
                    fs::read(output.join("sentinel")).expect("restored old"),
                    b"old"
                );
                assert!(!scratch.exists());
            }
        }
    }
    #[test]
    fn cancelled_before_commit_preserves_destination() {
        let dir = tempfile::tempdir().expect("temp");
        let output = dir.path().join("flui.xcframework");
        fs::create_dir(&output).expect("output");
        fs::write(output.join("sentinel"), "old").expect("sentinel");
        let state = token();
        let mut work = Work::new(dir.path(), Arc::clone(&state)).expect("work");
        let staged = work.path().join("new");
        fs::create_dir(&staged).expect("new");
        drop(CancelOnDrop(state));
        assert!(commit(&mut work, &staged, &output).is_err());
        assert_eq!(
            fs::read(output.join("sentinel")).expect("old output"),
            b"old"
        );
        let state = token();
        state.store(COMMITTING, Ordering::Release);
        drop(CancelOnDrop(Arc::clone(&state)));
        assert_eq!(state.load(Ordering::Acquire), COMMITTING);
    }
    #[cfg(unix)]
    fn process_gone(pid: &str) -> bool {
        !Command::new("kill")
            .args(["-0", pid])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("query child")
            .success()
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn abandoned_waiter_reaps_running_child_before_scratch_cleanup() {
        let dir = tempfile::tempdir().expect("temp");
        let state = token();
        let cancel = CancelOnDrop(Arc::clone(&state));
        let mut work = Work::new(dir.path(), state).expect("work");
        let scratch = work.path().to_path_buf();
        let marker = scratch.join("started");
        let marker_arg = marker.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished = Arc::clone(&done);
        let waiter = tokio::spawn(async move {
            let _cancel = cancel;
            tokio::task::spawn_blocking(move || {
                let result = work.command(
                    "sh",
                    &[
                        "-c".into(),
                        "echo $$ > \"$1\"; exec sleep 30".into(),
                        "probe".into(),
                        marker_arg.into_os_string(),
                    ],
                );
                drop(work);
                finished.store(true, Ordering::Release);
                result
            })
            .await
        });
        let until = Instant::now() + Duration::from_secs(5);
        while !marker.exists() {
            assert!(Instant::now() < until, "child started");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let pid = fs::read_to_string(&marker).expect("pid");
        waiter.abort();
        assert!(waiter.await.expect_err("cancelled waiter").is_cancelled());
        while !done.load(Ordering::Acquire) {
            assert!(Instant::now() < until, "worker completed cancellation");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(process_gone(pid.trim()), "child reaped");
        assert!(!scratch.exists(), "cleanup after child wait");
    }
    #[cfg(unix)]
    #[test]
    fn tool_deadline_and_worker_unwind_reap_before_cleanup() {
        let dir = tempfile::tempdir().expect("temp");
        let mut work = Work::new(dir.path(), token()).expect("work");
        assert!(
            work.command_with_timeout("sleep", &["30".into()], Duration::from_millis(20))
                .is_err()
        );
        assert!(work.child.is_none(), "deadline observed terminal status");
        let scratch = work.path().to_path_buf();
        work.child = Some(Command::new("sleep").arg("30").spawn().expect("child"));
        let pid = work.child.as_ref().expect("child").id().to_string();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _work = work;
            panic!("worker failure");
        }));
        assert!(unwound.is_err());
        assert!(process_gone(&pid));
        assert!(!scratch.exists());
    }
    fn metadata(variant: Option<&str>) -> serde_json::Value {
        let mut entry = serde_json::json!({"LibraryIdentifier":"slice", "LibraryPath":"libflui.a", "SupportedPlatform":"ios", "SupportedArchitectures":["arm64"]});
        if let Some(variant) = variant {
            entry["SupportedPlatformVariant"] = variant.into();
        }
        serde_json::json!({"AvailableLibraries":[entry]})
    }
    #[test]
    fn plist_validation_rejects_wrong_variant_duplicates_and_escaping_paths() {
        let dir = tempfile::tempdir().expect("temp");
        let source = archive(dir.path(), "source.a");
        fs::create_dir(dir.path().join("slice")).expect("slice");
        fs::copy(&source, dir.path().join("slice/libflui.a")).expect("copy");
        let groups =
            plan(&["aarch64-apple-ios".into()], std::slice::from_ref(&source)).expect("plan");
        let libs = BTreeMap::from([(Variant::Device, source)]);
        validate(dir.path(), &metadata(None), &groups, &libs).expect("valid");
        assert!(validate(dir.path(), &metadata(Some("simulator")), &groups, &libs).is_err());
        let mut duplicate = metadata(None);
        let entry = duplicate["AvailableLibraries"][0].clone();
        duplicate["AvailableLibraries"]
            .as_array_mut()
            .expect("array")
            .push(entry);
        assert!(validate(dir.path(), &duplicate, &groups, &libs).is_err());
        for path in ["../source.a", "/source.a"] {
            let mut bad = metadata(None);
            bad["AvailableLibraries"][0]["LibraryPath"] = path.into();
            assert!(validate(dir.path(), &bad, &groups, &libs).is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_never_redirect_destination_or_library_reads() {
        let dir = tempfile::tempdir().expect("temp");
        let outside = archive(dir.path(), "outside");
        let output = dir.path().join("flui.xcframework");
        std::os::unix::fs::symlink(&outside, &output).expect("link");
        assert!(no_symlink_destination(&output).is_err());
        fs::remove_file(&outside).expect("dangling");
        assert!(no_symlink_destination(&output).is_err());
        assert!(checked_child(dir.path(), "flui.xcframework").is_err());
    }
}

/// Application delivery uses the same child/scratch/publication ownership as libraries.
pub(crate) async fn package_application(
    ctx: &BuilderContext,
    artifacts: &BuildArtifacts,
    executable: &Path,
) -> BuildResult<FinalArtifacts> {
    let Platform::IOS { targets } = &ctx.platform else {
        return Err(invalid("expected iOS application target"));
    };
    if targets.len() != 1 {
        return Err(invalid("an application requires exactly one iOS target"));
    }
    let triple = targets[0].clone();
    let mut bundle = if let Some(bundle) = &ctx.bundle {
        bundle.clone()
    } else {
        let name = artifacts.metadata["package_name"].as_str().ok_or_else(|| {
            invalid("application bundle metadata or Cargo package metadata is required")
        })?;
        let mut bundle = crate::AppBundle::new(name, "dev.flui");
        bundle.version = artifacts.metadata["package_version"]
            .as_str()
            .ok_or_else(|| invalid("Cargo package version is required"))?
            .into();
        bundle
    };
    let version = cargo_metadata::semver::Version::parse(&bundle.version)
        .map_err(|error| invalid(format!("invalid application SemVer: {error}")))?;
    let numeric = format!("{}.{}.{}", version.major, version.minor, version.patch);
    bundle.version = version.to_string();
    validate_bundle(&bundle)?;
    let executable = executable.canonicalize()?;
    if !executable.is_file() {
        return Err(invalid("application executable must be a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&executable)?.permissions().mode() & 0o111 == 0 {
            return Err(invalid("application binary is not executable"));
        }
    }
    let output = ctx.output_dir.clone();
    let cancel = Arc::new(AtomicU8::new(RUNNING));
    let _guard = CancelOnDrop(Arc::clone(&cancel));
    tokio::task::spawn_blocking(move || {
        let (arch, platform) = match triple.as_str() {
            "aarch64-apple-ios" => ("arm64", "IOS"),
            "aarch64-apple-ios-sim" => ("arm64", "IOSSIMULATOR"),
            "x86_64-apple-ios" => ("x86_64", "IOSSIMULATOR"),
            _ => return Err(invalid(format!("unsupported application target {triple}"))),
        };
        let filename = format!("{}.app", bundle.name);
        let destination = output.join(&filename); no_symlink_destination(&destination)?;
        fs::create_dir_all(&output)?; let parent = output.canonicalize()?;
        let mut work = Work::new(&parent, cancel)?;
        let architectures = work.command("xcrun", &["lipo".into(), "-archs".into(), executable.as_os_str().into()])?;
        if String::from_utf8_lossy(&architectures).split_whitespace().collect::<Vec<_>>() != [arch] { return Err(invalid("application Mach-O architecture does not match target")); }
        let header = work.command("xcrun", &["vtool".into(), "-show-build".into(), executable.as_os_str().into()])?;
        let minimum = macho_minimum(&String::from_utf8_lossy(&header), platform)?;
        let staged = work.path().join(&filename); fs::create_dir(&staged)?;
        fs::copy(&executable, staged.join("flui_app"))?;
        let plist = serde_json::json!({
            "CFBundlePackageType":"APPL", "CFBundleExecutable":"flui_app",
            "CFBundleName":bundle.name, "CFBundleDisplayName":bundle.name,
            "CFBundleIdentifier":bundle.identifier, "CFBundleVersion":numeric,
            "CFBundleShortVersionString":numeric, "FLUIVersion":bundle.version,
            "MinimumOSVersion":minimum, "LSRequiresIPhoneOS":true,
            "CFBundleSupportedPlatforms":[if platform == "IOS" {"iPhoneOS"} else {"iPhoneSimulator"}],
            "UIDeviceFamily":[1,2], "UILaunchScreen":{},
            "UIApplicationSceneManifest": {
                "UIApplicationSupportsMultipleScenes": false,
                "UISceneConfigurations": {
                    "UIWindowSceneSessionRoleApplication": [{
                        "UISceneConfigurationName": "FLUI",
                        "UISceneDelegateClassName": "FluiSceneDelegate"
                    }]
                }
            },
            "UISupportedInterfaceOrientations":["UIInterfaceOrientationPortrait","UIInterfaceOrientationLandscapeLeft","UIInterfaceOrientationLandscapeRight"]
        });
        let info = staged.join("Info.plist"); fs::write(&info, serde_json::to_vec(&plist).map_err(|error| invalid(error.to_string()))?)?;
        work.command("plutil", &["-convert".into(), "xml1".into(), info.into_os_string()])?;
        let size_bytes = size(&staged)?;
        commit(&mut work, &staged, &parent.join(filename))?;
        Ok(FinalArtifacts { app_binary: destination, size_bytes })
    }).await.map_err(|error| invalid(format!("application packaging worker failed: {error}")))?
}
fn validate_bundle(bundle: &crate::AppBundle) -> BuildResult<()> {
    if bundle.name.is_empty()
        || bundle.name.contains(['/', '\\'])
        || !matches!(
            Path::new(&bundle.name)
                .components()
                .collect::<Vec<_>>()
                .as_slice(),
            [Component::Normal(_)]
        )
    {
        return Err(invalid(
            "application name must be one normal filename component",
        ));
    }
    if !bundle.identifier.contains('.')
        || !bundle.identifier.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(invalid(
            "application identifier must contain nonempty dot-separated ASCII alphanumeric/hyphen components",
        ));
    }
    Ok(())
}
fn macho_minimum(text: &str, expected: &str) -> BuildResult<String> {
    let platforms: Vec<_> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("platform "))
        .map(str::trim)
        .collect();
    let minimums: Vec<_> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("minos "))
        .map(str::trim)
        .collect();
    if platforms.len() != 1 || platforms[0] != expected || minimums.len() != 1 {
        return Err(invalid(
            "Mach-O must declare exactly one matching platform and minimum OS",
        ));
    }
    let minimum = minimums[0];
    if minimum.is_empty()
        || minimum.split('.').count() > 3
        || !minimum
            .split('.')
            .all(|part| !part.is_empty() && part.parse::<u32>().is_ok())
    {
        return Err(invalid("invalid Mach-O deployment minimum"));
    }
    Ok(minimum.into())
}

#[cfg(test)]
mod app_tests {
    use super::*;
    #[test]
    fn actual_macho_metadata_is_required_and_conflicts_are_rejected() {
        assert_eq!(
            macho_minimum(" platform IOSSIMULATOR\n minos 14.0\n", "IOSSIMULATOR")
                .expect("metadata"),
            "14.0"
        );
        for text in [
            "",
            "platform IOS\nminos 14.0",
            "platform IOSSIMULATOR\nminos bad",
            "platform IOSSIMULATOR\nplatform IOS\nminos 14.0",
            "platform IOSSIMULATOR\nminos 14.0\nminos 15.0",
        ] {
            assert!(macho_minimum(text, "IOSSIMULATOR").is_err());
        }
    }
    #[test]
    fn bundle_identity_rejects_path_escape_and_underscore_identifier() {
        for name in ["../bad", "/outside", "x\\bad", ""] {
            assert!(validate_bundle(&crate::AppBundle::new(name, "org.test")).is_err());
        }
        let mut bundle = crate::AppBundle::new("counter-app", "org.test");
        assert_eq!(bundle.identifier, "org.test.counter-app");
        validate_bundle(&bundle).expect("valid");
        bundle.identifier = "org.test.counter_app".into();
        assert!(validate_bundle(&bundle).is_err());
    }
}
