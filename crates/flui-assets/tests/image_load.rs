//! `AssetRegistry::load_image`: a decoded-image load on the lanes it is
//! given, shared by every request for a source and owned by the registry
//! rather than by any one request. A load the lanes or its request drop
//! unfinished is handed over to the next request still waiting.
#![cfg(feature = "images")]

use std::num::NonZeroUsize;
use std::sync::Arc;

use flui_assets::{AssetError, AssetRegistry, AssetRegistryBuilder, Image, LoadServices};
use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};
use flui_platform::executor::SpawnError;

use crate::lanes::{QueuedLanes, ThreadLanes, block_on};

/// Absolute path to the committed 4x2 RGBA fixture PNG (shared with
/// `image_asset_integration.rs`).
fn fixture_path() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tiny.png")
}

fn registry() -> AssetRegistry {
    AssetRegistryBuilder::new().with_default_capacity().build()
}

/// Loads `path` on a native host's default lanes and waits for it.
fn load(registry: &AssetRegistry, path: impl Into<Arc<str>>) -> Result<Image, AssetError> {
    block_on(registry.load_image(path, ThreadLanes::new().services()))
}

fn assert_fixture(image: &Image) {
    assert_eq!(
        (image.width(), image.height()),
        (4, 2),
        "the decoded image keeps the fixture's 4x2 dimensions"
    );
}

/// The file is read on the IO lane and decoded on the compute lane.
fn loads_on_the_given_lanes() {
    let lanes = QueuedLanes::new();
    let registry = registry();
    let pending = registry.load_image(fixture_path(), lanes.services());
    assert_eq!(lanes.pending_io(), 1, "the read waits on the IO lane");
    lanes.run_until_idle();
    assert_eq!(lanes.computes(), 1, "the decode ran on the compute lane");
    assert_fixture(&block_on(pending).expect("the fixture decodes"));
}

/// With no lanes the load runs inside the request, where it is polled.
fn without_lanes_the_load_runs_where_it_is_polled() {
    let image = block_on(registry().load_image(fixture_path(), LoadServices::new()))
        .expect("the fixture decodes");
    assert_fixture(&image);
}

/// Lanes with no room for the load leave it to the request, which runs it
/// where it is polled.
///
/// Red-check: answer the refusal as the load's failure.
fn saturated_lanes_leave_the_load_to_the_request() {
    let lanes = QueuedLanes::new();
    lanes.refuse(SpawnError::Saturated);
    let image = block_on(registry().load_image(fixture_path(), lanes.services()))
        .expect("the fixture decodes");
    assert_fixture(&image);
    assert_eq!((lanes.pending_io(), lanes.computes()), (0, 0));
}

/// A missing file resolves to a typed error, and the next load succeeds.
fn a_missing_file_is_an_error_and_the_next_load_succeeds() {
    let registry = registry();
    let missing = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/does-not-exist.png"
    );
    let result = load(&registry, missing);
    assert!(
        matches!(result, Err(AssetError::LoadFailed { .. })),
        "a nonexistent path is a typed error, got {result:?}",
    );
    assert_fixture(&load(&registry, fixture_path()).expect("the next load succeeds"));
}

/// Dropping one of two requests, the one that started the load or the one
/// that joined it, leaves the load running for the other.
///
/// Red-check: cancel the load when any subscriber leaves.
fn a_request_that_leaves_does_not_cancel_the_load_for_the_rest() {
    for drop_lead in [false, true] {
        let lanes = QueuedLanes::new();
        let registry = registry();
        let lead = registry.load_image(fixture_path(), lanes.services());
        let joined = registry.load_image(fixture_path(), lanes.services());
        assert_eq!(lanes.pending_io(), 1, "one load for both requests");
        let kept = if drop_lead {
            drop(lead);
            joined
        } else {
            drop(joined);
            lead
        };
        lanes.run_until_idle();
        assert_fixture(&block_on(kept).expect("the load ran for the request still waiting"));
    }
}

/// A load running inside a request that leaves is taken over by the next
/// request still waiting.
///
/// Red-check: answer a closed load as a failure instead of handing it over.
fn a_load_whose_request_left_is_taken_over() {
    let registry = registry();
    let lead = registry.load_image(fixture_path(), LoadServices::new());
    let joined = registry.load_image(fixture_path(), LoadServices::new());
    drop(lead);
    assert_fixture(&block_on(joined).expect("the waiting request took the load over"));
}

/// Lanes that shut down with a load queued drop it; a request from another
/// runtime still waiting takes it over on its own lanes. A new request on
/// the retired lanes is refused with a typed error.
///
/// Red-check: answer a closed load as a failure instead of handing it over.
fn a_load_on_lanes_that_shut_down_is_taken_over_on_other_lanes() {
    let retiring = QueuedLanes::new();
    let other = ThreadLanes::new();
    let registry = registry();
    let first = registry.load_image(fixture_path(), retiring.services());
    let second = registry.load_image(fixture_path(), other.services());
    retiring.shut_down();
    assert_fixture(&block_on(second).expect("the other runtime's request took the load over"));
    assert_fixture(&block_on(first).expect("the first request then finds the image kept"));

    let refused = block_on(registry.load_image(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/never-loaded.png"
        ),
        retiring.services(),
    ));
    assert!(
        matches!(
            refused,
            Err(AssetError::LanesRefused {
                reason: SpawnError::ShuttingDown,
                ..
            })
        ),
        "retired lanes refuse a new load, got {refused:?}"
    );
}

/// A load cancelled before its read never asks for a decode slot or reaches
/// the decoder, and the one slot stays free for the next load.
///
/// Red-check: poll the read before the cancel signal and skip the cancelled
/// check before the decode slot (the decoder is reached).
fn a_cancelled_load_never_reaches_the_decoder() {
    let lanes = QueuedLanes::new();
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decode_threads(NonZeroUsize::MIN)
        .build();
    drop(registry.load_image(fixture_path(), lanes.services()));
    lanes.run_until_idle();
    assert_eq!(lanes.computes(), 0, "the cancelled load never decoded");
    assert_eq!(lanes.pending_io(), 0, "and its read is gone");

    let next = registry.load_image(fixture_path(), lanes.services());
    lanes.run_until_idle();
    assert_fixture(&block_on(next).expect("the decode slot is free"));
}

/// The compute lane retires before the IO lane and drops the decode it
/// accepted: the load closes unsettled and the request on other lanes takes
/// it over, rather than every request failing with the retiring lanes.
///
/// Red-check: settle a dropped decode as `DecodeFailed`.
fn a_decode_its_lanes_drop_is_taken_over_on_other_lanes() {
    let retiring = QueuedLanes::new();
    let other = ThreadLanes::new();
    let registry = registry();
    let first = registry.load_image(fixture_path(), retiring.services());
    let second = registry.load_image(fixture_path(), other.services());
    retiring.run_io_until_idle();
    assert_eq!(
        retiring.computes(),
        1,
        "the read is done and its decode queued"
    );
    retiring.drop_compute();
    retiring.run_io_until_idle();
    assert_fixture(&block_on(second).expect("the other lanes' request took the load over"));
    assert_fixture(&block_on(first).expect("the first request then finds the image kept"));
}

/// Lanes shutting down refuse the decode: the load closes unsettled instead
/// of decoding on the retiring IO lane, and the request on other lanes takes
/// it over.
///
/// Red-check: decode a `ShuttingDown` refusal inline (the image is decoded
/// on the retiring lane).
fn a_decode_refused_by_retiring_lanes_is_taken_over_on_other_lanes() {
    let retiring = QueuedLanes::new();
    let other = ThreadLanes::new();
    let registry = registry();
    let first = registry.load_image(fixture_path(), retiring.services());
    let second = registry.load_image(fixture_path(), other.services());
    retiring.refuse(SpawnError::ShuttingDown);
    retiring.run_until_idle();
    let source = flui_assets::ImageSource::Asset(fixture_path().into());
    assert!(
        registry.cached_image(&source).is_none(),
        "the retiring IO lane decoded nothing"
    );
    assert_fixture(&block_on(second).expect("the other lanes' request took the load over"));
    drop(first);
}

/// Loads with no lanes run only while polled, so ones their caller holds
/// unpolled hold nothing: the load it awaits gets a load slot even when
/// there are more held loads than slots, and a request for a held source
/// runs the load itself instead of waiting on a run nobody drives.
///
/// Red-check: admit a load without lanes when it is created (the awaited
/// load never starts).
fn loads_held_unpolled_leave_the_load_to_the_awaited_one() {
    let registry = AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decode_threads(NonZeroUsize::MIN)
        .build();
    let held: Vec<_> = (0..8)
        .map(|index| {
            registry.load_image(
                format!(
                    "{}/tests/fixtures/held-{index}.png",
                    env!("CARGO_MANIFEST_DIR")
                ),
                LoadServices::new(),
            )
        })
        .collect();
    assert_fixture(
        &block_on(registry.load_image(fixture_path(), LoadServices::new()))
            .expect("the awaited load runs"),
    );
    drop(held);

    let shared = AssetRegistryBuilder::new().with_default_capacity().build();
    let held = shared.load_image(fixture_path(), LoadServices::new());
    let lanes = ThreadLanes::new();
    assert_fixture(
        &block_on(shared.load_image(fixture_path(), lanes.services()))
            .expect("the request for the held source loads it"),
    );
    drop(held);
}

/// A path that names no regular file is refused before it is opened: a FIFO
/// or a device could block the shared IO lane in `open`.
///
/// Red-check: drop the regular-file check (the directory opens and its read
/// fails with an OS error instead).
fn a_path_that_is_not_a_regular_file_is_refused() {
    let refused = block_on(registry().load_image(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"),
        LoadServices::new(),
    ));
    assert!(
        matches!(&refused, Err(AssetError::LoadFailed { reason, .. })
            if reason.contains("not a regular file")),
        "a directory is refused, got {refused:?}"
    );
}

/// A file over the 256 MiB encoded-image cap fails with a typed error before
/// it is buffered. `set_len` makes the file without writing its bytes.
fn an_oversized_file_is_refused_before_it_is_read() {
    let files = FixtureCopies::new("oversized");
    let path = files.0.join("huge.png");
    std::fs::File::create(&path)
        .and_then(|file| file.set_len(256 * 1024 * 1024 + 1))
        .expect("oversized test file is created");
    let registry = budgeted(FIXTURE_BYTES);
    let result = load(&registry, path.to_string_lossy().into_owned());
    assert!(
        matches!(&result, Err(flui_assets::AssetError::LoadFailed { reason, .. })
            if reason.contains("larger than")),
        "an oversized file is refused by size, got {result:?}"
    );
}

/// Copies of the 4x2 fixture (32 decoded bytes each) under distinct paths, in
/// a directory removed on drop.
struct FixtureCopies(std::path::PathBuf);

/// Decoded bytes of one fixture image.
const FIXTURE_BYTES: usize = 4 * 2 * 4;

impl FixtureCopies {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "flui-assets-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test clock is after the epoch")
                .as_nanos()
        ));
        std::fs::create_dir(&directory).expect("test directory is created");
        Self(directory)
    }

    fn copy(&self, name: &str) -> String {
        let path = self.0.join(name);
        std::fs::copy(fixture_path(), &path).expect("fixture is copied");
        path.to_string_lossy().into_owned()
    }
}

impl Drop for FixtureCopies {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn budgeted(bytes: usize) -> flui_assets::AssetRegistry {
    AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decoded_image_budget(bytes)
        .build()
}

fn is_kept(registry: &flui_assets::AssetRegistry, path: &str) -> bool {
    registry
        .cached_image(&flui_assets::ImageSource::Asset(path.into()))
        .is_some()
}

fn budget_evicts_the_least_recently_used_image() {
    let files = FixtureCopies::new("budget");
    let registry = budgeted(2 * FIXTURE_BYTES);
    let [a, b, c] = ["a.png", "b.png", "c.png"].map(|name| files.copy(name));
    for path in [&a, &b, &c] {
        load(&registry, path.as_str()).expect("fixture decodes");
    }
    assert!(
        !is_kept(&registry, &a),
        "the oldest image goes past the budget"
    );
    assert!(is_kept(&registry, &b) && is_kept(&registry, &c));
}

fn an_image_over_the_budget_is_returned_but_not_kept() {
    let files = FixtureCopies::new("over-budget");
    let registry = budgeted(FIXTURE_BYTES - 1);
    let path = files.copy("large.png");
    let image = load(&registry, path.as_str()).expect("fixture decodes");
    assert_eq!((image.width(), image.height()), (4, 2));
    assert!(!is_kept(&registry, &path));
}

fn trims_by_pressure_level(level: MemoryPressureLevel, older_kept: bool, newer_kept: bool) {
    let files = FixtureCopies::new("pressure");
    let registry = budgeted(2 * FIXTURE_BYTES);
    let [older, newer] = ["older.png", "newer.png"].map(|name| files.copy(name));
    for path in [&older, &newer] {
        load(&registry, path.as_str()).expect("fixture decodes");
    }

    registry.reclaim(level);

    assert_eq!(is_kept(&registry, &older), older_kept, "{level:?}");
    assert_eq!(is_kept(&registry, &newer), newer_kept, "{level:?}");
    let reloaded = load(&registry, older.as_str()).expect("reloads");
    assert_eq!((reloaded.width(), reloaded.height()), (4, 2));
}

fn moderate_pressure_trims_to_half_the_budget() {
    trims_by_pressure_level(MemoryPressureLevel::Moderate, false, true);
}

fn critical_pressure_drops_every_image() {
    trims_by_pressure_level(MemoryPressureLevel::Critical, false, false);
}

/// A registry keeps decoded images within its byte budget, least recently used
/// first, and gives them back under memory pressure by level.
#[test]
fn decoded_images_stay_within_their_byte_budget_and_give_back_under_pressure() {
    crate::cases::run_cases(&[
        (
            "budget evicts the least recent",
            budget_evicts_the_least_recently_used_image,
        ),
        (
            "an image over the budget",
            an_image_over_the_budget_is_returned_but_not_kept,
        ),
        (
            "moderate pressure",
            moderate_pressure_trims_to_half_the_budget,
        ),
        ("critical pressure", critical_pressure_drops_every_image),
    ]);
}

/// A load runs on the lanes it is given, or where it is polled without
/// them, is shared by every request for its source, is handed over when it
/// is dropped unfinished, and never decodes once cancelled.
#[test]
fn image_loads_run_on_their_lanes_and_survive_the_requests_that_leave() {
    crate::cases::run_cases(&[
        ("on the given lanes", loads_on_the_given_lanes),
        (
            "without lanes",
            without_lanes_the_load_runs_where_it_is_polled,
        ),
        (
            "saturated lanes",
            saturated_lanes_leave_the_load_to_the_request,
        ),
        (
            "missing file recovery",
            a_missing_file_is_an_error_and_the_next_load_succeeds,
        ),
        (
            "oversized file",
            an_oversized_file_is_refused_before_it_is_read,
        ),
        (
            "a request leaves",
            a_request_that_leaves_does_not_cancel_the_load_for_the_rest,
        ),
        (
            "the running request leaves",
            a_load_whose_request_left_is_taken_over,
        ),
        (
            "lanes shut down",
            a_load_on_lanes_that_shut_down_is_taken_over_on_other_lanes,
        ),
        (
            "cancelled before the read",
            a_cancelled_load_never_reaches_the_decoder,
        ),
        (
            "compute lane retires first",
            a_decode_its_lanes_drop_is_taken_over_on_other_lanes,
        ),
        (
            "compute lane refuses while shutting down",
            a_decode_refused_by_retiring_lanes_is_taken_over_on_other_lanes,
        ),
        (
            "unpolled loads hold nothing",
            loads_held_unpolled_leave_the_load_to_the_awaited_one,
        ),
        (
            "not a regular file",
            a_path_that_is_not_a_regular_file_is_refused,
        ),
    ]);
}
