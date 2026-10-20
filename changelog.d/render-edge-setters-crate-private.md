### Changed

- `flui-rendering`: `RenderNode::{set_parent, add_child, remove_child, links_mut}` and the matching `NodeLinks` setters are crate-private, and the unused `insert_child` and `clear_children` setters are gone. Change a render-tree edge through `RenderTree::adopt_child`, `drop_child` or `set_children`, which keep depth and the layout-walk stamp in step. The `testing` feature adds `RenderTree::append_unlinked_child` for tests that need a torn child list.
