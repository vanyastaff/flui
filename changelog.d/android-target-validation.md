### Fixed

- Build Android examples for the selected device's native ABI and report failed non-reloading builds promptly.
- Deliver Android system bar and cutout insets to SafeArea when native window geometry matches the captured insets.
- Avoid redundant clip masks and supersampled attachments for eligible rounded rectangle fills while preserving clipping.
- Clear supersampled path and coverage textures in their geometry pass without a separate render pass.
- Isolate activity-indicator animation damage and refresh nested retained paint boundaries after keyed relocation.
- Present retained frames with a direct GPU texture copy on compatible surfaces, preserving shader fallback for other sizes, formats and capabilities.
- Submit seed, clear, drawing and presentation commands together while preserving retained-frame recovery and GPU resource retirement.
- Share render passes between adjacent tessellated and instanced geometry while preserving texture and destination-read boundaries.
- Retire lazy sliver descendants before their host render objects during scene replacement.
- Preserve Android keyboard layout and modifier-dependent characters instead of substituting lowercase ASCII.
