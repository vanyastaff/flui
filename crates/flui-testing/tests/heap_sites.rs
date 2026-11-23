//! `heap::sites` against a real DHAT profile: the counting allocator forwards to DHAT's, and a
//! profile names the line of this file that allocated.
//!
//! Its own target, built only with the `dhat` feature: it installs the process's global
//! allocator and runs its one DHAT profiler.

use flui_testing::heap::sites::SiteProfiler;
use flui_testing::heap::{CountingAllocator, HeapCounts};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Pushes 100 bytes into a new `Vec`: one allocation of 8, then a growth to 16, 32, 64 and 128.
#[inline(never)]
fn grow_a_vec() -> Vec<u8> {
    let mut grown = Vec::new();
    for byte in 0..100_u8 {
        grown.push(byte);
    }
    grown
}

/// While a profiler records, the counters still see only what the program asked: DHAT's own
/// records of each allocation (its captured stack, its tables) go through the same global
/// allocator, nested inside the call that counts. The profile then puts the allocations at the
/// line that pushed, under `Vec`'s and the allocator's frames.
#[test]
fn a_profiled_stretch_counts_and_names_only_what_the_program_asked() {
    let profiler = SiteProfiler::start(format!(
        "{}/heap_sites/grow_a_vec.json",
        env!("CARGO_TARGET_TMPDIR")
    ))
    .expect("this target is built with the `dhat` feature");

    let before = HeapCounts::now();
    let grown = std::hint::black_box(grow_a_vec());
    let counts = HeapCounts::now().since(before);
    drop(grown);

    assert_eq!(
        (
            counts.allocations,
            counts.reallocations,
            counts.allocated_bytes
        ),
        (5, 4, 8 + 16 + 32 + 64 + 128),
        "DHAT's own allocations are not the program's"
    );

    let sites = profiler
        .finish("grow_a_vec")
        .expect("the profile reads back");
    let [site] = sites.sites() else {
        panic!("one site, the push in `grow_a_vec`:\n{sites}");
    };
    assert!(
        site.location
            .starts_with("crates/flui-testing/tests/heap_sites.rs:"),
        "the site is this file's line, not std's or DHAT's:\n{sites}"
    );
    assert!(
        site.function.ends_with("grow_a_vec"),
        "the site's function is named in full:\n{sites}"
    );
    assert_eq!((site.blocks, site.bytes), (5, 248), "{sites}");
}
