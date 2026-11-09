//! A build context lends no lock handle: build code cannot hold a tree guard
//! across a call that takes the owner's write lock.
#![allow(deprecated, reason = "the deprecated detached context is the subject of this check")]
use flui_view::testing::ElementBuildContext;

fn reach_the_locks(ctx: &ElementBuildContext) {
    let _tree = ctx.tree().read();
    let _owner = ctx.build_owner().write();
}

fn main() {}
