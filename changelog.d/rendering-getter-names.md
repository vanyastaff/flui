### Changed

- `RenderSliver::get_absolute_size` is now `absolute_size`, and `get_absolute_size_relative_to_origin` is now `absolute_size_relative_to_origin`.
- The viewport trait's `get_offset_to_reveal` is now `offset_to_reveal`.
- `SingleChildLayoutDelegate`, `MultiChildLayoutDelegate` and `FlowDelegate`: `get_size` is now `size`, `get_constraints_for_child` is now `constraints_for_child`, and `get_position_for_child` is now `position_for_child`.
- `SliverGridDelegate::get_layout` is now `layout`.
- `SliverGridLayout`: `get_scroll_offset_of_child` is now `scroll_offset_of_child`, `get_cross_axis_offset_of_child` is now `cross_axis_offset_of_child`, `get_min_child_index_for_scroll_offset` is now `min_child_index_for_scroll_offset` and `get_max_child_index_for_scroll_offset` is now `max_child_index_for_scroll_offset`.
- `RenderNode::debug_name` is now `RenderNode::kind`, which forwards the render object's `Inspect::kind`. The name an inspector shows for a render object is that kind, without type arguments.
- `flui-testing`'s `render_debug_names` is now `render_kinds`.

### Removed

- `RenderBox::get_distance_to_baseline`, which had no caller; `compute_distance_to_actual_baseline` stays.
- `RenderObject::debug_name`, `RenderBox::debug_name` and `RenderSliver::debug_name`; use `Inspect::kind`.
