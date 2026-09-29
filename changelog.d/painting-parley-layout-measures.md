### Changed

- `TextPainter`'s measuring methods take the `TextContext` they measure through: `layout`, `min_intrinsic_width`, `max_intrinsic_width`, `intrinsic_height`, `dry_size` and `dry_baseline` each take `&mut TextContext` first. A painter's cached layout is measured again when it is given a context over another font collection, or after a face is registered on the collection. Render objects pass their layout context's `ctx.text()`; elsewhere build one with `TextContext::new(&FontCollection::new())`.
- `RenderObject::intrinsic_raw`, `dry_layout_raw` and `dry_baseline_raw`, `Protocol::with_leaf_erased_ctx`, `RenderEntry::layout_leaf_only`, `RenderNode::layout_leaf_erased` and `ErasedBoxLayoutCtx::new` take the realm's text context as `Option<&RefCell<TextContext>>`; pass `None` where no pipeline lends one.
- `RenderParagraph` and `RenderEditable` measure through the realm's text context, lent by each presentation's pipeline (ADR-0092 §10 step 3a). The default build measures as before.

### Added

- `flui-painting`'s `parley-layout` feature: `TextPainter` measures size, baselines and intrinsic widths with Parley on the realm's `TextContext`; glyphs and carets still come from cosmic-text until ADR-0092 §10 step 5.
- `TextContextHandle`, the realm's shared text context, and `TextCx`, the scoped loan a render object measures with: `BoxLayoutContext::text`, `BoxIntrinsicsCtx::text`, `BoxDryLayoutCtx::text` and `BoxDryBaselineCtx::text`.
- `PipelineOwner::set_text_context`, which the runtime calls for every presentation's pipeline.
- `FontCollection::generation`, which counts the registrations that added a face, and `ParagraphSpec::max_lines` and `ParagraphLayout::content_widths` on the Parley path.
