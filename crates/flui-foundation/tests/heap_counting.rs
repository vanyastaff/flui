//! The counting global allocator counts what this thread asks of the heap, and nothing else.
//!
//! A target of its own, because a `#[global_allocator]` is the whole binary's.

use std::hint::black_box;

use flui_foundation::testing::heap::{CountingAllocator, HeapCounts, largest_allocation_during};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn a_binary_that_installs_the_allocator_reads_as_installed() {
    assert!(CountingAllocator::is_installed());
}

#[test]
fn a_window_counts_its_blocks_their_bytes_and_what_is_still_live() {
    let (counts, kept) = HeapCounts::during(|| black_box(Vec::<u8>::with_capacity(100)));
    assert_eq!(
        counts,
        HeapCounts {
            allocations: 1,
            reallocations: 0,
            allocated_bytes: 100,
            live_bytes: 100,
        }
    );
    let (freed, ()) = HeapCounts::during(|| drop(kept));
    assert_eq!(freed.allocations, 0, "a free is no allocation");
    assert_eq!(
        freed.live_bytes.cast_signed(),
        -100,
        "the freed block leaves the live bytes"
    );
}

#[test]
fn a_reallocation_is_one_block_of_its_new_size_and_frees_the_old_one() {
    let mut buffer = black_box(Vec::<u8>::with_capacity(16));
    let (counts, ()) = HeapCounts::during(|| buffer.reserve_exact(48));
    assert_eq!(
        counts,
        HeapCounts {
            allocations: 1,
            reallocations: 1,
            allocated_bytes: 48,
            live_bytes: 32,
        },
        "16 bytes grown to 48 asks for 48 and keeps 32 more live"
    );
    drop(buffer);
}

#[test]
fn another_threads_allocations_are_not_charged_to_this_one() {
    const BLOCK: usize = 1 << 20;
    // The thread is spawned before the window, so the window holds only its work and the join.
    let other = std::thread::spawn(|| {
        let (counts, block) = HeapCounts::during(|| black_box(vec![0_u8; BLOCK]));
        drop(block);
        counts.allocated_bytes
    });
    let (here, theirs) = HeapCounts::during(|| other.join().expect("the other thread allocates"));
    assert!(
        theirs >= BLOCK as u64,
        "the other thread counts its own block"
    );
    assert!(
        here.allocated_bytes < BLOCK as u64,
        "{} bytes charged here for a block another thread allocated",
        here.allocated_bytes
    );
}

#[test]
fn the_largest_block_of_a_window_is_its_own_and_the_enclosing_window_still_sees_it() {
    let (outer, inner) = largest_allocation_during(|| {
        drop(black_box(Vec::<u8>::with_capacity(4096)));
        let (inner, ()) =
            largest_allocation_during(|| drop(black_box(Vec::<u8>::with_capacity(512))));
        inner
    });
    assert_eq!(inner, 512, "the inner window reports only its own blocks");
    assert_eq!(
        outer, 4096,
        "the outer window keeps its larger block across the inner one"
    );

    let (outer, ()) = largest_allocation_during(|| {
        let (inner, ()) =
            largest_allocation_during(|| drop(black_box(Vec::<u8>::with_capacity(8192))));
        assert_eq!(inner, 8192);
    });
    assert_eq!(
        outer, 8192,
        "the outer window sees the inner window's block"
    );

    let (none, ()) = largest_allocation_during(|| {});
    assert_eq!(none, 0, "a window that asks for nothing reports zero");
}
