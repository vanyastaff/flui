### Fixed

- A render-tree slot whose generations are exhausted retires instead of panicking after it was freed, which let the next insert reissue the removed node's last `RenderId`.
- `RenderTree::clear` empties the tree and resets the root before any render object's destructor runs, so a destructor that panics no longer leaves old ids resolving to nodes inserted after the clear.
- `PipelineOwner::remove_render_object` drops the removed nodes after it has cleared the root and invalidated the former parent, so a render object's destructor that panics no longer leaves the subtree's ancestors in the tree or the parent without its relayout.
- A signal slot at its last generation is retired instead of wrapping to generation 0, where an old handle would read the new signal.
