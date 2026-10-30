### Added

- **Act and read back through the development agent** (`flui-view`, `flui-runtime`,
  `flui-semantics`; ADR-0238): `AgentWindow::act_and_read` serves a semantics action and, when
  asked, answers with the tree change it caused: a delta (appeared, gone and changed nodes, with
  bounds only on request, capped with a `truncated` flag) or the full tree, read after the frame
  the action's effect landed in. A window that closes first answers the action as `ran` with a
  `readback_failed` reading. `flui_semantics::TreeSnapshot` is the wire-node snapshot the delta is
  computed from.
