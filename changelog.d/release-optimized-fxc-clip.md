### Fixed

- Fix optimized Windows DX12/FXC shader compilation for shared shape clipping while preserving fractional coverage and destination-sensitive blending.
- Composite untextured SrcOver vertex meshes using their effective color alpha and fractional clip coverage, preserving the existing destination.
