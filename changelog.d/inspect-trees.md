### Added

- **Typed inspection of the element, render and layer trees** (`flui-view`, `flui-rendering`,
  `flui-layer`): each tree implements `flui_foundation::inspect::InspectTree`, so `snapshot`
  reads it as typed fields. An element node is named by its view and links its render object. A
  render node carries its protocol, constraints, size or sliver geometry, offset and dirty flags,
  and links the semantics node it contributes. A layer node carries its own fields and links the
  boundary's render object. `PipelineOwner::inspect` gives the render tree's view;
  `LaidOut::inspect_elements`, `inspect_render` and `inspect_layers` (`flui-testing`) read a
  mounted app.
- **`View::inspect`** (`flui-view`): a provided method through which a view describes its
  configuration in the element tree's snapshot. `#[view(inspect)]` on `#[derive(StatelessView)]`
  or `#[derive(StatefulView)]` fills it from the type's own `#[derive(Inspect)]`.
- `flui_foundation::inspect::kind_of` and `kind_args_of` read a kind and its type arguments from
  a `type_name`, for trees whose nodes are erased.

### Changed

- `flui_foundation::inspect::snapshot` answers `Result<SnapshotNode, SnapshotError>`: `Empty` for
  a tree with no root, `Gone(id)` for a start node that is no longer in the tree, which includes an
  id whose slot now holds a newer node.
- `flui_foundation::inspect::Link::Layer` and `Links::layer` carry a `SceneLayer`: the `LayerId`
  and the scene it was minted in, which `LayerTree::scene` gives and `PipelineOwner` numbers per
  painted frame. A layer id kept from an earlier frame answers `Gone` instead of naming whatever
  holds its slot now, and reads as `l4.37` past the first scene.
- `flui_foundation::inspect::SnapshotNode` keeps the tree's facts (`facts`, read with `fact` and
  `get_fact`) apart from the object's own fields (`fields`, read with `field` and `get`), so a
  view's or render object's field named like a fact (`lifecycle`, `dirty`, `size`) reads as its
  own value. `diff` compares facts with facts and fields with fields, and reports a changed fact
  as the new `Change::Fact`.
- `snapshot`, and a `SnapshotNode`'s `render`, `diff`, clone, comparison and drop, keep their own
  stack, so a tree as deep as a long layer chain snapshots without overflowing the thread's stack.
  Every layer variant reports the parameters that change what it paints (a colour or image
  filter, a shader mask, a texture, a follower's anchors), and a picture a fingerprint of its
  commands, so `diff` sees a changed effect or drawing. An opacity layer reports its blend mode,
  the performance overlay its readout, a backdrop filter the group it samples with, and an
  annotated region its payload's `Debug` text and which payload it is. Groups and payloads are
  `Shared` values, numbered per type in the order the snapshot meets them (`BackdropGroup#2`),
  so equal scenes snapshot alike in every run and a freed address cannot alias two objects.
- `diff` reports bounds that appear or disappear as the new `Change::Bounds`.
- `SnapshotQuery::max_nodes` is an `Option<NonZeroU32>`: the start node counts toward it, so a
  limit of zero, which no snapshot could honour, cannot be written.
- `SnapshotNode`'s `Debug` lists the subtree in tree order, each node with its depth and its
  number of children, instead of nesting them.
- `diff` treats a node whose type arguments changed (`Widget<u8>` to `Widget<String>`) as removed
  and added.
- `#[derive(Inspect)]` rejects two fields that report under one name, and an enum field that
  reports as `variant`, which names the enum's variant.
- `flui_layer::AnnotationValue` is a type instead of an alias: it keeps the payload's type name
  and `Debug` form, with `downcast_ref`, `downcast` and `ptr_eq`. Annotation payloads
  (`AnnotatedRegionLayer::new`, `RenderAnnotatedRegion::new`) now need `Debug`.
