### Removed

- `flui_platform::CapabilityAbsence`, the three-variant subset of `UnsupportedReason`
  that `WindowCapabilities` recorded against each unavailable capability. It is collapsed
  into `UnsupportedReason` (T10 step 5): the capability surface names the reason directly,
  and the "retry later may help" distinction stays inside `UnsupportedReason::Unavailable`.
  `From<UnsupportedReason> for Unsupported` replaces the two `From<CapabilityAbsence>` impls.
