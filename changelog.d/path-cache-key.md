### Fixed

- The GPU path tessellation cache compares the path and its stroke parameters on every hit: two different paths whose 64-bit fingerprints collided drew each other's triangles. The fingerprint now only picks the bucket, under a per-cache random seed.
