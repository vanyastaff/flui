//! A recursive walk lists into a frame arena through the public API: each level's list lives
//! beside the ones above it, and each list drops its own elements. The arena's own rules (reuse,
//! release, in-place growth) are pinned by `arena::tests`, which miri runs.

use std::rc::Rc;

use flui_foundation::arena::{FrameArena, FrameVec};

/// Lists `0..depth` into one list per level, each alive while the levels below it run, and
/// returns the sum every level saw.
fn walk(arena: &FrameArena, depth: u32, held: &Rc<()>) -> u64 {
    let mut level: FrameVec<'_, (u32, Rc<()>)> = Vec::new_in(arena);
    level.extend((0..depth).map(|n| (n, Rc::clone(held))));
    let below = if depth == 0 {
        0
    } else {
        walk(arena, depth - 1, held)
    };
    below + level.iter().map(|&(n, _)| u64::from(n)).sum::<u64>()
}

#[test]
fn each_level_of_a_walk_has_its_own_list_and_drops_its_own_elements() {
    let mut arena = FrameArena::new();
    let held = Rc::new(());
    for frame in 0..3 {
        let sum = walk(&arena, 40, &held);
        // Level `d` sums `0..d`; the levels together sum each of those.
        let expected: u64 = (0..=40_u64).map(|d| d * d.saturating_sub(1) / 2).sum();
        assert_eq!(sum, expected, "frame {frame}");
        assert_eq!(
            Rc::strong_count(&held),
            1,
            "frame {frame}: every element dropped"
        );
        arena.reset();
    }
}
