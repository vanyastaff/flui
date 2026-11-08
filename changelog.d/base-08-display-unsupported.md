### Changed

- **`flui-platform`**: display colour and headroom refusals use `Unsupported` directly. `DisplayInfo::color_space()` and `DisplayInfo::headroom()` preserve the complete refusal, including its reason and optional capability name. Display recording keeps its existing wire representation and refuses values it cannot mirror without information loss.
- **`flui-platform`**: window capability snapshots store `UnsupportedReason` directly. `WindowCapabilities::command_capability()` returns `Result<Availability, Unsupported>`, and restoring `CapabilityParts` or `WindowStateParts` rejects reasons that cannot describe a lasting absence.

### Removed

- **`flui-platform`**: `CapabilityAbsence` and `WindowCapabilities::refusal()`; use the typed capability constructors and `command_capability()`.
