### Added

- **`flui-interaction`**: the primary selection is a capability of the seam:
  `cx.capability::<PrimarySelectionCapability>()` hands out a `PrimarySelectionHandle`, whose
  `claim` offers a `SelectionText` that is asked for its text only when the selection is read, and
  whose `read_text` reads it. The returned `PrimaryClaim` goes stale when another claim or
  another application takes the selection, and its drop releases the selection only while it is
  current. `PrimaryClaims` keeps a backend's claims. Hosts without a primary selection register
  no provider.
- **`flui-interaction`** (`testing`, re-exported by `flui_testing::host`):
  `ScriptedPrimarySelection`, the primary selection a `HeadlessHost` provides
  beside its clipboard (`HeadlessHost::primary_selection`). Its `hold_reads` is one hold of its
  read script, and `answer_held_reads` lets go of it: the reads it kept are answered once no
  other hold lives, the claim asked for its text as each is answered.
