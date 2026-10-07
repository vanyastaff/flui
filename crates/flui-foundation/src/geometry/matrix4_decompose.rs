//! Decomposition of a [`Matrix4`] for interpolation (CSS Transforms 2, "Decomposing a 3D
//! matrix"), in `f64`.
//!
//! A matrix `M` (column vectors, `p' = M·p`) whose bottom-right element is non-zero is
//! normalised by it and written as
//!
//! ```text
//! M = P · T · R · K · S
//! ```
//!
//! where `P` carries the perspective row, `T` the translation, `R` a rotation (stored as a
//! unit quaternion), `K` the upper unit-triangular shear (`xy`, `xz`, `yz`) and `S` the
//! diagonal scale. `R`, `K` and `S` come from Gram–Schmidt on the columns of the upper-left
//! 3×3 block, so `K·S` is upper triangular and `R` orthonormal; a negative determinant is
//! carried by negating all three scales and the rotation.
//!
//! An affine matrix whose linear block collapses an axis (zero scale) has no orientation of
//! its own: its scales are kept, and the rotation and shear are left for the caller to take
//! from the other endpoint ([`Decomposed::Collapsed`]).

use crate::geometry::Matrix4;

/// A matrix split into parts that interpolate independently.
#[derive(Debug, Clone, Copy)]
pub(super) struct Parts {
    /// The perspective row `(πx, πy, πz, w)`: the bottom row of `M` is `(πᵀ·A, πᵀ·t + w)`.
    perspective: [f64; 4],
    translation: [f64; 3],
    scale: [f64; 3],
    /// Shear `(xy, xz, yz)`.
    skew: [f64; 3],
    /// Unit quaternion `(x, y, z, w)`.
    rotation: [f64; 4],
}

/// The outcome of decomposing one endpoint.
#[derive(Debug, Clone, Copy)]
pub(super) enum Decomposed {
    /// A full decomposition.
    Full(Parts),
    /// An affine matrix with a collapsed axis: perspective, translation and scale are its
    /// own; rotation and skew are identity placeholders for the caller to replace.
    Collapsed(Parts),
    /// Not decomposable (`m33 = 0`, normalising by `m33` overflows, or a perspective row
    /// over a singular linear block).
    Singular,
}

/// A Gram–Schmidt residual at or below this fraction of the longest column counts as a
/// collapsed axis.
const COLLAPSE_TOLERANCE: f64 = 1e-12;

/// `M[row][col]` in column-major storage.
fn at(m: &[f64; 16], row: usize, col: usize) -> f64 {
    m[col * 4 + row]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f64; 3]) -> f64 {
    a[0].hypot(a[1]).hypot(a[2])
}

fn sub_scaled(a: [f64; 3], b: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] - b[0] * k, a[1] - b[1] * k, a[2] - b[2] * k]
}

fn scaled(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The determinant of the 3×3 matrix with columns `c`.
fn det3(c: [[f64; 3]; 3]) -> f64 {
    dot(c[0], cross(c[1], c[2]))
}

/// Solves `Aᵀ·x = b` for the 3×3 `A` with columns `c` (Cramer's rule): `x[i]` is the
/// determinant with column `i` of `Aᵀ` replaced, i.e. `dot(c[i], ·)` equations.
fn solve_transposed(c: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    // Aᵀ·x = b  ⇔  dot(c[i], x) = b[i]. Rows of Aᵀ are the columns c[i].
    let det = det3(c);
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    // Inverse of a matrix with rows r0, r1, r2: columns are (r1×r2, r2×r0, r0×r1) / det.
    let (r0, r1, r2) = (c[0], c[1], c[2]);
    let (k0, k1, k2) = (cross(r1, r2), cross(r2, r0), cross(r0, r1));
    let x = [
        (k0[0] * b[0] + k1[0] * b[1] + k2[0] * b[2]) / det,
        (k0[1] * b[0] + k1[1] * b[1] + k2[1] * b[2]) / det,
        (k0[2] * b[0] + k1[2] * b[1] + k2[2] * b[2]) / det,
    ];
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Unit quaternion `(x, y, z, w)` of the rotation with orthonormal columns `q`
/// (Shepperd's method: divide by the largest of the four candidate magnitudes).
fn quaternion(q: [[f64; 3]; 3]) -> [f64; 4] {
    // r[row][col] = q[col][row]
    let r = |row: usize, col: usize| q[col][row];
    let trace = r(0, 0) + r(1, 1) + r(2, 2);
    let quat = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (r(2, 1) - r(1, 2)) / s,
            (r(0, 2) - r(2, 0)) / s,
            (r(1, 0) - r(0, 1)) / s,
            s / 4.0,
        ]
    } else if r(0, 0) > r(1, 1) && r(0, 0) > r(2, 2) {
        let s = (1.0 + r(0, 0) - r(1, 1) - r(2, 2)).sqrt() * 2.0;
        [
            s / 4.0,
            (r(0, 1) + r(1, 0)) / s,
            (r(0, 2) + r(2, 0)) / s,
            (r(2, 1) - r(1, 2)) / s,
        ]
    } else if r(1, 1) > r(2, 2) {
        let s = (1.0 + r(1, 1) - r(0, 0) - r(2, 2)).sqrt() * 2.0;
        [
            (r(0, 1) + r(1, 0)) / s,
            s / 4.0,
            (r(1, 2) + r(2, 1)) / s,
            (r(0, 2) - r(2, 0)) / s,
        ]
    } else {
        let s = (1.0 + r(2, 2) - r(0, 0) - r(1, 1)).sqrt() * 2.0;
        [
            (r(0, 2) + r(2, 0)) / s,
            (r(1, 2) + r(2, 1)) / s,
            s / 4.0,
            (r(1, 0) - r(0, 1)) / s,
        ]
    };
    normalized(quat)
}

fn normalized(q: [f64; 4]) -> [f64; 4] {
    let norm = q[0].hypot(q[1]).hypot(q[2]).hypot(q[3]);
    if norm == 0.0 || !norm.is_finite() {
        return IDENTITY_ROTATION;
    }
    q.map(|v| v / norm)
}

const IDENTITY_ROTATION: [f64; 4] = [0.0, 0.0, 0.0, 1.0];

/// Decomposes `matrix` (finite elements expected; the caller filters the rest).
pub(super) fn decompose(matrix: &Matrix4) -> Decomposed {
    let m33 = at(&matrix.m, 3, 3);
    if m33 == 0.0 {
        return Decomposed::Singular;
    }
    let m = matrix.m.map(|v| v / m33);
    // A tiny m33 can push finite elements past f64::MAX; such a matrix has no
    // finite decomposition, so it takes the discrete path.
    if !m.iter().all(|v| v.is_finite()) {
        return Decomposed::Singular;
    }
    let columns = [0, 1, 2].map(|col| [at(&m, 0, col), at(&m, 1, col), at(&m, 2, col)]);
    let translation = [at(&m, 0, 3), at(&m, 1, 3), at(&m, 2, 3)];
    let bottom = [at(&m, 3, 0), at(&m, 3, 1), at(&m, 3, 2)];

    let perspective = if bottom == [0.0; 3] {
        [0.0, 0.0, 0.0, 1.0]
    } else {
        let Some(pi) = solve_transposed(columns, bottom) else {
            return Decomposed::Singular;
        };
        [pi[0], pi[1], pi[2], 1.0 - dot(pi, translation)]
    };

    let longest = columns.map(length).into_iter().fold(0.0_f64, f64::max);
    let tolerance = longest * COLLAPSE_TOLERANCE;

    // Gram–Schmidt, column by column, against the axes found so far.
    let mut axes: [Option<[f64; 3]>; 3] = [None; 3];
    let mut scale = [0.0; 3];
    let mut raw_skew = [0.0; 3]; // xy, xz, yz before division by the later scale
    for col in 0..3 {
        let mut residual = columns[col];
        for (earlier, axis) in axes.iter().enumerate().take(col) {
            if let Some(axis) = axis {
                let projection = dot(residual, *axis);
                // xy for (0, 1), xz for (0, 2), yz for (1, 2).
                raw_skew[earlier + col - 1] = projection;
                residual = sub_scaled(residual, *axis, projection);
            }
        }
        let len = length(residual);
        if len > tolerance && len > 0.0 {
            scale[col] = len;
            axes[col] = Some(scaled(residual, 1.0 / len));
        }
    }

    let [Some(q0), Some(q1), Some(q2)] = axes else {
        return Decomposed::Collapsed(Parts {
            perspective,
            translation,
            scale,
            skew: [0.0; 3],
            rotation: IDENTITY_ROTATION,
        });
    };

    let skew = [
        raw_skew[0] / scale[1],
        raw_skew[1] / scale[2],
        raw_skew[2] / scale[2],
    ];
    let (axes, scale) = if det3([q0, q1, q2]) < 0.0 {
        (
            [scaled(q0, -1.0), scaled(q1, -1.0), scaled(q2, -1.0)],
            scale.map(|s| -s),
        )
    } else {
        ([q0, q1, q2], scale)
    };
    Decomposed::Full(Parts {
        perspective,
        translation,
        scale,
        skew,
        rotation: quaternion(axes),
    })
}

impl Parts {
    /// `self` with `other`'s rotation and skew.
    pub(super) fn with_orientation_of(self, other: &Parts) -> Parts {
        Parts {
            rotation: other.rotation,
            skew: other.skew,
            ..self
        }
    }

    /// Interpolates every part linearly except the rotation, which is slerped along the
    /// shorter arc. `t` extrapolates.
    pub(super) fn interpolate(&self, other: &Parts, t: f64) -> Parts {
        // Finite components on opposite sides of the range overflow `b - a`; the
        // weighted sum stays finite wherever the result is representable.
        let mix = |a: f64, b: f64| {
            let span = b - a;
            if span.is_finite() {
                a + span * t
            } else {
                a * (1.0 - t) + b * t
            }
        };
        Parts {
            perspective: std::array::from_fn(|i| mix(self.perspective[i], other.perspective[i])),
            translation: std::array::from_fn(|i| mix(self.translation[i], other.translation[i])),
            scale: std::array::from_fn(|i| mix(self.scale[i], other.scale[i])),
            skew: std::array::from_fn(|i| mix(self.skew[i], other.skew[i])),
            rotation: slerp(self.rotation, other.rotation, t),
        }
    }

    /// `P · T · R · K · S` as a matrix.
    pub(super) fn recompose(&self) -> Matrix4 {
        let [x, y, z, w] = self.rotation;
        let q = [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y + z * w),
                2.0 * (x * z - y * w),
            ],
            [
                2.0 * (x * y - z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z + x * w),
            ],
            [
                2.0 * (x * z + y * w),
                2.0 * (y * z - x * w),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ];
        let [s0, s1, s2] = self.scale;
        let [xy, xz, yz] = self.skew;
        let add = |a: [f64; 3], b: [f64; 3]| [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
        let columns = [
            scaled(q[0], s0),
            scaled(add(q[1], scaled(q[0], xy)), s1),
            scaled(add(add(q[2], scaled(q[0], xz)), scaled(q[1], yz)), s2),
        ];
        let t = self.translation;
        let [px, py, pz, pw] = self.perspective;
        let pi = [px, py, pz];
        let mut m = [0.0; 16];
        for (col, column) in columns.iter().enumerate() {
            m[col * 4..col * 4 + 3].copy_from_slice(column);
            m[col * 4 + 3] = dot(pi, *column);
        }
        m[12..15].copy_from_slice(&t);
        m[15] = dot(pi, t) + pw;
        Matrix4 { m }
    }
}

/// Spherical interpolation of unit quaternions along the shorter arc; `t` extrapolates
/// along the same great circle.
fn slerp(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    let cos = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    let b = if cos < 0.0 { b.map(|v| -v) } else { b };
    // The angle from the chord and the sum keeps its precision for nearly equal
    // rotations, where `acos(cos)` loses it, so extrapolation stays on the great
    // circle however far `t` carries a tiny arc.
    let norm = |v: [f64; 4]| v.iter().map(|c| c * c).sum::<f64>().sqrt();
    let chord = norm(std::array::from_fn(|i| a[i] - b[i]));
    let sum = norm(std::array::from_fn(|i| a[i] + b[i]));
    let theta = 2.0 * chord.atan2(sum);
    let sin = theta.sin();
    let (wa, wb) = if sin == 0.0 {
        // The same rotation: every weighting of it is that rotation.
        (1.0 - t, t)
    } else {
        (((1.0 - t) * theta).sin() / sin, (t * theta).sin() / sin)
    };
    normalized(std::array::from_fn(|i| wa * a[i] + wb * b[i]))
}
