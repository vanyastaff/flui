//! The table of every variant an input recording carries (ADR-0215 §2).
//!
//! The event vocabulary is `#[non_exhaustive]`, so the recording's wire
//! conversions in `flui-runtime` cannot match it exhaustively: a variant added
//! to the vocabulary would compile and fail only when recorded. This test reads
//! each recorded enum from its source with `syn` and compares the list of its
//! variants with `tests/generated/recording_variants.rs`, which
//! `every_vocabulary_variant_round_trips_through_the_recording_dto` walks,
//! needing one sample per entry that survives the DTO round trip.
//!
//! A new variant fails the comparison until the table is regenerated with
//! `FLUI_BLESS=1 cargo nextest run -p flui-testing --test flui_testing_it -E
//! 'test(recording_vocabulary_table)'` (and the DTO test then fails until the DTO
//! mirrors it). The accesskit enums are read from the release `flui-runtime`
//! resolves, so an accesskit bump regenerates them too.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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
        "crates/flui-platform/src/input/event.rs",
        "PlatformInput",
    ),
    repo(
        "DragDropEvent",
        "crates/flui-platform/src/input/event.rs",
        "DragDropEvent",
    ),
    repo(
        "PointerEvent",
        "crates/flui-platform/src/input/pointer/mod.rs",
        "PointerEvent",
    ),
    repo(
        "ButtonChange",
        "crates/flui-platform/src/input/pointer/mod.rs",
        "ButtonChange",
    ),
    repo(
        "PointerKind",
        "crates/flui-platform/src/input/pointer/mod.rs",
        "PointerKind",
    ),
    repo(
        "PenTool",
        "crates/flui-platform/src/input/pointer/mod.rs",
        "PenTool",
    ),
    repo(
        "PointerRole",
        "crates/flui-platform/src/input/pointer/mod.rs",
        "PointerRole",
    ),
    repo(
        "CancelReason",
        "crates/flui-platform/src/input/pointer/mod.rs",
        "CancelReason",
    ),
    repo(
        "ScrollUnit",
        "crates/flui-platform/src/input/pointer/scroll.rs",
        "ScrollUnit",
    ),
    repo(
        "ScrollPrecision",
        "crates/flui-platform/src/input/pointer/scroll.rs",
        "ScrollPrecision",
    ),
    repo(
        "ScrollPhase",
        "crates/flui-platform/src/input/pointer/scroll.rs",
        "ScrollPhase",
    ),
    repo(
        "ScrollInversion",
        "crates/flui-platform/src/input/pointer/scroll.rs",
        "ScrollInversion",
    ),
    repo(
        "PanZoomPhase",
        "crates/flui-platform/src/input/pointer/pan_zoom.rs",
        "PanZoomPhase",
    ),
    repo(
        "MotionAcceleration",
        "crates/flui-platform/src/input/pointer/raw_motion.rs",
        "MotionAcceleration",
    ),
    repo(
        "TouchpadHoldPhase",
        "crates/flui-platform/src/input/pointer/touchpad.rs",
        "TouchpadHoldPhase",
    ),
    repo(
        "TouchpadSwipePhase",
        "crates/flui-platform/src/input/pointer/touchpad.rs",
        "TouchpadSwipePhase",
    ),
    repo(
        "Key",
        "crates/flui-platform/src/input/keyboard/mod.rs",
        "Key",
    ),
    repo(
        "KeyState",
        "crates/flui-platform/src/input/keyboard/mod.rs",
        "KeyState",
    ),
    repo(
        "Location",
        "crates/flui-platform/src/input/keyboard/mod.rs",
        "Location",
    ),
    repo(
        "KeyPress",
        "crates/flui-platform/src/input/keyboard/mod.rs",
        "KeyPress",
    ),
    repo(
        "ImeComposition",
        "crates/flui-platform/src/input/keyboard/mod.rs",
        "ImeComposition",
    ),
    repo(
        "ImeEvent",
        "crates/flui-platform/src/text/ime.rs",
        "ImeEvent",
    ),
    repo(
        "TransferFormat",
        "crates/flui-platform/src/transfer/data.rs",
        "TransferFormat",
    ),
    repo(
        "Availability",
        "crates/flui-platform/src/unsupported.rs",
        "Availability",
    ),
    repo(
        "WindowExecutionState",
        "crates/flui-platform/src/window/options.rs",
        "WindowExecutionState",
    ),
    repo(
        "Brightness",
        "crates/flui-platform/src/window/brightness.rs",
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
    repo(
        "ScrollBarVisibility",
        "crates/flui-platform/src/preferences.rs",
        "ScrollBarVisibility",
    ),
    repo(
        "ScrollTrackClick",
        "crates/flui-platform/src/preferences.rs",
        "ScrollTrackClick",
    ),
    repo(
        "Weekday",
        "crates/flui-platform/src/preferences/regional.rs",
        "Weekday",
    ),
    repo(
        "HourCycle",
        "crates/flui-platform/src/preferences/regional.rs",
        "HourCycle",
    ),
    repo(
        "MeasurementSystem",
        "crates/flui-platform/src/preferences/regional.rs",
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

/// The repository root, two levels above this crate's manifest.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("BUG: the crate sits two directories below the repository root")
}

/// The table the recorded enums' sources generate.
fn generate(root: &Path, accesskit_root: &Path) -> String {
    let mut out = String::from(
        "// @generated by `FLUI_BLESS=1` and the test `recording_vocabulary_table`; do not edit.\n\
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
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        let file = syn::parse_file(&source)
            .unwrap_or_else(|error| panic!("parsing {}: {error}", path.display()));
        let variants = variants(&file, recorded.name)
            .unwrap_or_else(|message| panic!("reading from {}: {message}", path.display()));
        for variant in variants {
            writeln!(out, "    ({:?}, {variant:?}),", recorded.label)
                .expect("BUG: writing to a String cannot fail");
        }
    }
    out.push_str("];\n");
    out
}

/// The variants of `enum name` in `file`, in declaration order; a variant
/// behind a `cfg` is listed too, since the DTO mirrors every configuration.
fn variants(file: &syn::File, name: &str) -> Result<Vec<String>, String> {
    let item = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Enum(item) if item.ident == name => Some(item),
            _ => None,
        })
        .ok_or_else(|| format!("no `enum {name}`"))?;
    Ok(item
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect())
}

/// The package root of the accesskit `flui-runtime` depends on.
fn accesskit_root(root: &Path) -> PathBuf {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .expect("running `cargo metadata --locked`");
    let resolve = metadata
        .resolve
        .as_ref()
        .expect("`cargo metadata` returned no dependency graph");
    let runtime = metadata
        .workspace_packages()
        .into_iter()
        .find(|package| package.name.as_str() == "flui-runtime")
        .expect("flui-runtime is not a workspace package");
    let node = resolve
        .nodes
        .iter()
        .find(|node| node.id == runtime.id)
        .expect("flui-runtime is not in the dependency graph");
    let dependency = node
        .deps
        .iter()
        .find(|dep| dep.name == "accesskit")
        .expect("flui-runtime does not depend on accesskit");
    metadata[&dependency.pkg]
        .manifest_path
        .parent()
        .expect("accesskit's manifest path has no parent")
        .as_std_path()
        .to_owned()
}

/// The table is read from the files it names: each repository path exists, so
/// a moved module fails here rather than as a missing enum.
#[test]
fn every_recorded_repository_source_exists() {
    let root = repo_root();
    for recorded in RECORDED {
        if let Source::Repo(path) = recorded.source {
            assert!(root.join(path).is_file(), "{path} is missing");
        }
    }
}

/// The checked-in table is what the recorded enums' sources generate.
/// `FLUI_BLESS=1` rewrites it instead.
#[test]
fn the_variant_table_matches_the_recorded_vocabulary() {
    let root = repo_root();
    let generated = generate(&root, &accesskit_root(&root));
    let output = root.join(OUTPUT);
    if std::env::var("FLUI_BLESS").as_deref() == Ok("1") {
        std::fs::write(&output, &generated)
            .unwrap_or_else(|error| panic!("writing {}: {error}", output.display()));
        return;
    }
    let committed = std::fs::read_to_string(&output).ok();
    assert!(
        committed.as_deref() == Some(generated.as_str()),
        "{OUTPUT} is not what the recorded vocabulary generates; run `FLUI_BLESS=1 cargo \
         nextest run -p flui-testing --test flui_testing_it -E 'test(recording_vocabulary_table)'`, \
         then give each new variant a sample in the recording DTO round-trip test"
    );
}
