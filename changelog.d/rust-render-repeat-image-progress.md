### Fixed

- Repeated images crop the final partial tile rather than shrinking the whole source, in ordinary and advanced blend modes.
- Repeated-image recording omits the entire draw when destination geometry is nonfinite or a tile edge cannot advance, preserving neighboring draws. Recording stops immediately after its existing frame quota refuses growth, and the next frame can recover.
