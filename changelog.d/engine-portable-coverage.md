### Fixed

- Preserve independent geometric coverage for direct destination-sensitive
  tessellated and gradient draws on devices without dual-source blending,
  including transparent Clear and
  gradient edges. Clamp fractional Plus before mixing with the destination.
- Preserve rounded-gradient edge coverage when cropping scratch passes, without
  widening their scissors, and skip invisible SSAA paths before backdrop admission.

### Added

- Add `WgpuPainter::render_to_texture` for validated, texture-aware rendering.
