### Fixed

- A parent's writes to a child's parent data during layout (`BoxLayoutContext::child_parent_data_mut`, a lazy sliver's `layout_offset`) now stay with the child; the layout walk used to write them into a copy it then dropped, and it no longer allocates that copy per child per pass.
