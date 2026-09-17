# Flutter getTransformTo post-frame liveness lesson

Date: 2026-09-14

External signal:

- flutter/flutter#148656: production crashes in `RenderObject.getTransformTo`.
- Maintainer comments identify the likely root cause: a post-frame callback
  reading `localToGlobal/getTransformTo` after the placeholder/render box has
  been unmounted, missing an `attached` check.

FLUI mapping:

- `PipelineOwner::transform_to`, `local_to_global`, `global_to_local`, and
  `box_size` all return `Option`, so unattached/not-laid-out/not-descendant
  cases are routine `None` rather than panics.
- Inspected callers in hero, overlay, editable text, draggable, interactive
  viewer, and focus rect providers. The important callers propagate `None`
  with `?`/`and_then` rather than fabricating geometry.
- Existing hero/overlay/editable tests cover stale handles and post-frame
  liveness.

Verification:

- `cargo nextest run -p flui-widgets --lib -E 'test(hero) or test(overlay) or test(editable_text)' --no-fail-fast`
  => 187 passed / 686 skipped.
- Existing FLUI issue search for stale `transform_to` / post-frame
  `RenderId` geometry found no matching open issue.

Disposition:

- No new issue filed. Keep this as a regression lesson: future post-frame or
  async geometry readers must keep the `attached-or-None` contract and avoid
  replacing `None` with zero-size/identity fallback geometry.

