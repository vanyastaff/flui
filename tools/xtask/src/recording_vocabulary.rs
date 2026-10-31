//! The table of every variant an input recording carries (ADR-0215 §2).
//!
//! The event vocabulary and `InputRecord` are `#[non_exhaustive]`, so the
//! recording DTO in `flui-testing` cannot match them exhaustively: a variant
//! added to the vocabulary would compile and fail only when recorded. This
//! command reads each recorded enum from its source with `syn` and writes the
//! list of its variants to `crates/flui-testing/tests/generated/recording_variants.rs`,
//! which `every_vocabulary_variant_round_trips_through_the_recording_dto`
//! walks, needing one sample per entry that survives the DTO round trip.
//!
//! Without arguments it checks that the checked-in table is what the sources
//! generate, so `cargo xtask checks` fails on a new variant until the table is
//! regenerated with `--write` (and the test then fails until the DTO mirrors
//! it). The accesskit enums are read from the release `flui-runtime` resolves,
//! so an accesskit bump regenerates them too.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};

use crate::util::repo_root;

/// Where the generated table lives, relative to the repository root.
const OUTPUT: &str = "crates/flui-testing/tests/generated/recording_variants.rs";

/// Where an enum's source is read from.
#[derive(Debug, Clone, Copy)]
enum Source {
    /// A file in the repository.
    Repo(&'static str),
    /// A file in the accesskit package `flui-runtime` depends on.
    Accesskit(&'static str),
}

/// One recorded enum: the label the table and the test name it by, its
/// source and its name there.
#[derive(Debug, Clone, Copy)]
struct Recorded {
    label: &'static str,
    source: Source,
    name: &'static str,
}

const fn repo(label: &'static str, path: &'static str, name: &'static str) -> Recorded {
    Recorded {
        label,
        source: Source::Repo(path),
        name,
    }
}

const fn accesskit(label: &'static str, name: &'static str) -> Recorded {
    Recorded {
        label,
        source: Source::Accesskit("src/lib.rs"),
        name,
    }
}

/// Every enum a recording row can carry.
const RECORDED: &[Recorded] = &[
    repo(
        "InputRecord",
        "crates/flui-runtime/src/input_record.rs",
        "InputRecord",
    ),
    repo(
        "HostInput",
        "crates/flui-runtime/src/host_input.rs",
        "HostInput",
    ),
    repo(
        "RecordedSurface",
        "crates/flui-runtime/src/input_record.rs",
        "RecordedSurface",
    ),
    repo(
        "InputOutcome",
        "crates/flui-runtime/src/owner/presentation_dispatch.rs",
        "InputOutcome",
    ),
    repo(
        "WindowObservation",
        "crates/flui-runtime/src/owner/presentation_dispatch.rs",
        "WindowObservation",
    ),
    repo(
        "PlatformInput",
        "crates/flui-platform/src/input.rs",
        "PlatformInput",
    ),
    repo(
        "DragDropEvent",
        "crates/flui-platform/src/input.rs",
        "DragDropEvent",
    ),
    repo(
        "PointerEvent",
        "crates/flui-platform/src/pointer/mod.rs",
        "PointerEvent",
    ),
    repo(
        "ButtonChange",
        "crates/flui-platform/src/pointer/mod.rs",
        "ButtonChange",
    ),
    repo(
        "PointerKind",
        "crates/flui-platform/src/pointer/mod.rs",
        "PointerKind",
    ),
    repo(
        "PenTool",
        "crates/flui-platform/src/pointer/mod.rs",
        "PenTool",
    ),
    repo(
        "PointerRole",
        "crates/flui-platform/src/pointer/mod.rs",
        "PointerRole",
    ),
    repo(
        "CancelReason",
        "crates/flui-platform/src/pointer/mod.rs",
        "CancelReason",
    ),
    repo(
        "PenSqueezePhase",
        "crates/flui-platform/src/pointer/mod.rs",
        "PenSqueezePhase",
    ),
    repo(
        "ScrollUnit",
        "crates/flui-platform/src/pointer/scroll.rs",
        "ScrollUnit",
    ),
    repo(
        "ScrollPrecision",
        "crates/flui-platform/src/pointer/scroll.rs",
        "ScrollPrecision",
    ),
    repo(
        "ScrollPhase",
        "crates/flui-platform/src/pointer/scroll.rs",
        "ScrollPhase",
    ),
    repo(
        "PanZoomPhase",
        "crates/flui-platform/src/pointer/pan_zoom.rs",
        "PanZoomPhase",
    ),
    repo("Key", "crates/flui-platform/src/keyboard/mod.rs", "Key"),
    repo(
        "KeyState",
        "crates/flui-platform/src/keyboard/mod.rs",
        "KeyState",
    ),
    repo(
        "Location",
        "crates/flui-platform/src/keyboard/mod.rs",
        "Location",
    ),
    repo(
        "KeyRepeat",
        "crates/flui-platform/src/keyboard/mod.rs",
        "KeyRepeat",
    ),
    repo(
        "ImeComposition",
        "crates/flui-platform/src/keyboard/mod.rs",
        "ImeComposition",
    ),
    repo("ImeEvent", "crates/flui-platform/src/ime.rs", "ImeEvent"),
    repo(
        "TransferFormat",
        "crates/flui-platform/src/data_transfer.rs",
        "TransferFormat",
    ),
    repo(
        "Availability",
        "crates/flui-platform/src/unsupported.rs",
        "Availability",
    ),
    repo(
        "WindowExecutionState",
        "crates/flui-platform/src/window.rs",
        "WindowExecutionState",
    ),
    repo(
        "Brightness",
        "crates/flui-platform/src/brightness.rs",
        "Brightness",
    ),
    repo(
        "MotionPreference",
        "crates/flui-platform/src/preferences.rs",
        "MotionPreference",
    ),
    repo(
        "TextWeightPreference",
        "crates/flui-platform/src/preferences.rs",
        "TextWeightPreference",
    ),
    repo(
        "WheelStep",
        "crates/flui-platform/src/preferences.rs",
        "WheelStep",
    ),
    repo("Weekday", "crates/flui-platform/src/regional.rs", "Weekday"),
    repo(
        "HourCycle",
        "crates/flui-platform/src/regional.rs",
        "HourCycle",
    ),
    repo(
        "MeasurementSystem",
        "crates/flui-platform/src/regional.rs",
        "MeasurementSystem",
    ),
    repo(
        "GestureDisposition",
        "crates/flui-interaction/src/gesture/arena/mod.rs",
        "GestureDisposition",
    ),
    accesskit("accesskit::Action", "Action"),
    accesskit("accesskit::ActionData", "ActionData"),
    accesskit("accesskit::ScrollUnit", "ScrollUnit"),
    accesskit("accesskit::ScrollHint", "ScrollHint"),
];

/// Arguments for `cargo xtask recording-vocabulary`.
#[derive(Debug, clap::Args)]
pub(crate) struct RecordingVocabularyArgs {
    /// Regenerate the table instead of checking it.
    #[arg(long)]
    write: bool,
}

/// `cargo xtask recording-vocabulary`: check (or `--write`) the recorded
/// variant table.
pub(crate) fn recording_vocabulary(args: &RecordingVocabularyArgs) -> anyhow::Result<ExitCode> {
    let root = repo_root();
    let accesskit_root = accesskit_root(&root)?;
    let generated = generate(&root, &accesskit_root)?;
    let output = root.join(OUTPUT);
    if args.write {
        std::fs::write(&output, &generated)
            .with_context(|| format!("writing {}", output.display()))?;
        println!("recording-vocabulary: wrote {OUTPUT}");
        return Ok(ExitCode::SUCCESS);
    }
    if std::fs::read_to_string(&output).ok().as_deref() == Some(generated.as_str()) {
        println!("recording-vocabulary: the recorded variant table matches the vocabulary");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "recording-vocabulary: {OUTPUT} is not what the vocabulary generates; run \
         `cargo xtask recording-vocabulary --write`, then give each new variant a sample in \
         flui-testing's recording DTO round-trip test"
    );
    Ok(ExitCode::FAILURE)
}

fn generate(root: &Path, accesskit_root: &Path) -> anyhow::Result<String> {
    let mut out = String::from(
        "// @generated by `cargo xtask recording-vocabulary --write`; do not edit.\n\
         //\n\
         // Every variant of every enum an input recording carries (ADR-0215 §2), which\n\
         // `every_vocabulary_variant_round_trips_through_the_recording_dto` walks.\n\n\
         /// `(enum, variant)` for each recorded variant.\n\
         pub(crate) const VARIANTS: &[(&str, &str)] = &[\n",
    );
    for recorded in RECORDED {
        let path = match recorded.source {
            Source::Repo(path) => root.join(path),
            Source::Accesskit(path) => accesskit_root.join(path),
        };
        let source = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let file =
            syn::parse_file(&source).with_context(|| format!("parsing {}", path.display()))?;
        let variants = variants(&file, recorded.name)
            .with_context(|| format!("reading `{}` from {}", recorded.name, path.display()))?;
        for variant in variants {
            writeln!(out, "    ({:?}, {variant:?}),", recorded.label)?;
        }
    }
    out.push_str("];\n");
    Ok(out)
}

/// The variants of `enum name` in `file`, in declaration order; a variant
/// behind a `cfg` is listed too, since the DTO mirrors every configuration.
fn variants(file: &syn::File, name: &str) -> anyhow::Result<Vec<String>> {
    let Some(item) = file.items.iter().find_map(|item| match item {
        syn::Item::Enum(item) if item.ident == name => Some(item),
        _ => None,
    }) else {
        bail!("no `enum {name}`");
    };
    Ok(item
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect())
}

/// The package root of the accesskit `flui-runtime` depends on.
fn accesskit_root(root: &Path) -> anyhow::Result<PathBuf> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .context("running `cargo metadata --locked`")?;
    let resolve = metadata
        .resolve
        .as_ref()
        .context("`cargo metadata` returned no dependency graph")?;
    let runtime = metadata
        .workspace_packages()
        .into_iter()
        .find(|package| package.name.as_str() == "flui-runtime")
        .context("flui-runtime is not a workspace package")?;
    let node = resolve
        .nodes
        .iter()
        .find(|node| node.id == runtime.id)
        .context("flui-runtime is not in the dependency graph")?;
    let dependency = node
        .deps
        .iter()
        .find(|dep| dep.name == "accesskit")
        .context("flui-runtime does not depend on accesskit")?;
    let package = &metadata[&dependency.pkg];
    Ok(package
        .manifest_path
        .parent()
        .context("accesskit's manifest path has no parent")?
        .as_std_path()
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::{RECORDED, Source};

    /// The table is read from the files it names: each repository path
    /// exists, so a moved module fails here rather than as a missing enum.
    #[test]
    fn every_recorded_repository_source_exists() {
        let root = crate::util::repo_root();
        for recorded in RECORDED {
            if let Source::Repo(path) = recorded.source {
                assert!(root.join(path).is_file(), "{path} is missing");
            }
        }
    }
}
