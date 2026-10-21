//! A build context lends no lock handle: build code cannot hold a tree guard
//! across a call that takes the owner's write lock.
use flui_view::ElementBuildContext;

fn reach_the_locks(ctx: &ElementBuildContext) {
    let _tree = ctx.tree().read();
    let _owner = ctx.build_owner().write();
}

fn main() {}
