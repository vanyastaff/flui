//! Least-Squares polynomial regression
//!
//! Reusable solver for fitting a polynomial to a weighted dataset. Used by
//! the velocity tracker (gesture fling estimation) and the input predictor
//! (position extrapolation). Mirrors Flutter's `lsq_solver.dart` API but
//! with Rust-native types and a static-allocating design for the
//! 20-sample hot path.
//!
//! # Algorithm
//!
//! Computes the QR decomposition of the (weighted) Vandermonde matrix via
//! the Gram-Schmidt process, then solves the upper-triangular system
//! `R B = Qᵀ W y` for the polynomial coefficients via back-substitution.
//! The coefficient of determination (R²) is also reported as a
//! confidence measure.
//!
//! This is the same algorithm Flutter uses; it is numerically stable for
//! the small sample sizes (≤ 20) typical of pointer tracking.
//!
//! # Example
//!
//! ```rust,ignore
//! use crate::processing::lsq_solver::{PolynomialFit, solve_two};
//!
//! // Fit a quadratic (degree=2) to (t, x) and (t, y) with weights w.
//! let t = vec![-100.0, -50.0, 0.0];       // time in ms
//! let x = vec![0.0, 50.0, 100.0];         // position in px
//! let y = vec![0.0, 10.0, 20.0];
//! let w = vec![0.6, 0.8, 1.0];            // weights (recent = higher)
//!
//! let (fit, _): (Option<PolynomialFit>, _) = solve_two(&t, &w, &x, &y, 2);
//! if let Some(fit) = fit {
//!     // Coefficients are [a₀, a₁, a₂] for y = a₀ + a₁·t + a₂·t².
//!     // Velocity at t=0 is a₁.
//!     let velocity = fit.coefficients[1];
//! }
//! ```
//!
//! Flutter reference: <https://api.flutter.dev/flutter/gestures/LeastSquaresSolver-class.html>

// ============================================================================
// Constants
// ============================================================================

/// Flutter's `precisionErrorTolerance` (1e-6). Vectors whose norm falls
/// below this are considered linearly dependent and the solve returns
/// `None`.
const PRECISION_ERROR_TOLERANCE: f64 = 1e-6;

/// Default sample size. The velocity tracker keeps a 20-sample circular
/// buffer; this is the maximum size the solver will see in practice.
pub const MAX_SAMPLES: usize = 20;

/// Default maximum polynomial degree. Flutter fits degree=2 (quadratic)
/// for the velocity estimator; higher degrees are not meaningful for
/// the sample sizes we have.
pub const MAX_DEGREE: usize = 2;

/// Capacity of a single matrix row/column in the stack-allocated
/// scratch space. Derived from `MAX_SAMPLES` so the bound is enforced
/// at compile time.
const SCRATCH_M: usize = MAX_SAMPLES;
/// Capacity of the matrix dimension (rows for A/Q, both dims for R).
/// Derived from `MAX_DEGREE` so the bound is enforced at compile time.
const SCRATCH_N: usize = MAX_DEGREE + 1;

// ============================================================================
// PolynomialFit
// ============================================================================

/// An n-th degree polynomial fit to a dataset.
///
/// `coefficients[i]` is the coefficient of the `i`-th power of the
/// independent variable, so a quadratic fit yields
/// `y = coefficients[0] + coefficients[1]·x + coefficients[2]·x²`.
///
/// `confidence` is the R² value (1.0 = perfect fit, 0.0 = no
/// explanatory power).
///
/// Coefficients are stored in a fixed-size stack buffer
/// (`[f64; MAX_DEGREE + 1]`) — not a heap `Vec` — to keep the
/// velocity-tracker's hot path zero-allocation. Coefficients past
/// `degree` are zero.
#[derive(Debug, Clone, PartialEq)]
pub struct PolynomialFit {
    /// Polynomial coefficients. `coefficients[0..=degree]` is the
    /// active range; `[degree+1..]` is zero padding.
    pub coefficients: [f64; MAX_DEGREE + 1],

    /// Polynomial degree. Must be in `[0, MAX_DEGREE]`.
    pub degree: u8,

    /// R² goodness-of-fit. Range: [0.0, 1.0].
    pub confidence: f64,
}

impl PolynomialFit {
    /// Create a polynomial fit of the given degree with all coefficients
    /// initialised to zero.
    ///
    /// Returns `None` if `degree > MAX_DEGREE`.
    pub fn new(degree: usize) -> Option<Self> {
        if degree > MAX_DEGREE {
            return None;
        }
        Some(Self {
            coefficients: [0.0; MAX_DEGREE + 1],
            degree: degree as u8,
            confidence: 0.0,
        })
    }

    /// Number of active coefficients (= `degree + 1`). Crate-internal helper
    /// for [`coefficients_slice`](Self::coefficients_slice).
    #[inline]
    fn len(&self) -> usize {
        self.degree as usize + 1
    }

    /// Iterate over the active coefficient slice.
    #[inline]
    fn coefficients_slice(&self) -> &[f64] {
        &self.coefficients[..self.len()]
    }
}

// ============================================================================
// Solve entry points
// ============================================================================

/// Factorize the weighted Vandermonde design matrix built from sample positions
/// `x` and weights `w` into its Gram-Schmidt QR form, writing `Q` (n×m) into `q`
/// and `R` (n×n, upper-triangular) into `r`. Returns `(m, n)` on success, or
/// `None` for insufficient or linearly-dependent data.
///
/// The factorization depends only on `(x, w, degree)`, not on the right-hand
/// side, so it can be reused across multiple `y`-vectors — see [`solve_two`].
/// Complexity: O(n²·m), and with n ≤ `MAX_DEGREE`+1 and m ≤ `MAX_SAMPLES` that is
/// a small bounded constant.
// Math-style names (x, w, q, r, a, m, n, h, i, j) mirror Flutter's
// lsq_solver.dart QR/Gram-Schmidt algorithm; renaming would hurt parity review.
#[expect(clippy::many_single_char_names)]
fn factorize(
    x: &[f64],
    w: &[f64],
    degree: usize,
    q: &mut [f64],
    r: &mut [f64],
) -> Option<(usize, usize)> {
    let m = x.len();
    // No data / not enough points / degree too high / stack buffers too small.
    // A degree-`d` fit has `d + 1` coefficients and needs at least that many
    // points, so `degree >= m` (equivalently `degree + 1 > m`) is underdetermined
    // and rejected here rather than relying on a later singular-matrix failure.
    if m == 0 || degree >= m || degree > MAX_DEGREE || m > SCRATCH_M {
        return None;
    }
    let n = degree + 1;

    // Step 1: weighted Vandermonde matrix A (n × m), row-major, on the stack
    // (Vec has no SBO; these fixed buffers avoid heap allocation per fit).
    let mut a = [0.0_f64; SCRATCH_N * SCRATCH_M];
    for h in 0..m {
        let wh = w[h];
        a[h] = wh;
        let mut x_pow = x[h];
        for i in 1..n {
            a[i * m + h] = wh * x_pow;
            x_pow *= x[h];
        }
    }

    // Step 2: Gram-Schmidt QR.
    for j in 0..n {
        for h in 0..m {
            q[j * m + h] = a[j * m + h];
        }
        for i in 0..j {
            let mut dot = 0.0_f64;
            for h in 0..m {
                dot += q[j * m + h] * q[i * m + h];
            }
            for h in 0..m {
                q[j * m + h] -= dot * q[i * m + h];
            }
        }
        let mut norm_sq = 0.0_f64;
        for h in 0..m {
            let qjh = q[j * m + h];
            norm_sq += qjh * qjh;
        }
        let norm = norm_sq.sqrt();
        if norm < PRECISION_ERROR_TOLERANCE {
            // Linearly dependent — no unique solution.
            return None;
        }
        let inv_norm = 1.0 / norm;
        for h in 0..m {
            q[j * m + h] *= inv_norm;
        }
        for i in j..n {
            let mut dot = 0.0_f64;
            for h in 0..m {
                dot += q[j * m + h] * a[i * m + h];
            }
            r[j * n + i] = dot;
        }
    }

    Some((m, n))
}

/// Solve for one right-hand side `y` against a precomputed `(q, r)`
/// factorization from [`factorize`]. Back-substitutes `R B = Qᵀ W Y` and
/// computes the R² confidence. `x`/`w` must be the slices that produced the
/// factorization. Complexity: O(n·m).
// Math-style names (x, y, w, q, r, m, n, h, i, j) mirror Flutter's
// lsq_solver.dart back-substitution; renaming would hurt parity review.
#[expect(clippy::many_single_char_names)]
fn solve_rhs(
    x: &[f64],
    y: &[f64],
    w: &[f64],
    q: &[f64],
    r: &[f64],
    m: usize,
    n: usize,
) -> Option<PolynomialFit> {
    let mut result = PolynomialFit::new(n - 1)?;

    // Step 3: back-substitute R B = Qᵀ W Y from bottom-right to top-left.
    let mut wy = [0.0_f64; SCRATCH_M];
    for (h, (&yh, &wh)) in y[..m].iter().zip(w[..m].iter()).enumerate() {
        wy[h] = yh * wh;
    }
    for i in (0..n).rev() {
        let mut sum = 0.0_f64;
        for h in 0..m {
            sum += q[i * m + h] * wy[h];
        }
        for j in (i + 1)..n {
            sum -= r[i * n + j] * result.coefficients[j];
        }
        let r_ii = r[i * n + i];
        if r_ii.abs() < PRECISION_ERROR_TOLERANCE {
            return None;
        }
        result.coefficients[i] = sum / r_ii;
    }

    // Step 4: R² confidence (only the active coefficient slice contributes; the
    // trailing padding slots are zero).
    let y_mean: f64 = y[..m].iter().sum::<f64>() / m as f64;
    let mut sum_squared_error = 0.0_f64;
    let mut sum_squared_total = 0.0_f64;
    for h in 0..m {
        let mut predicted = 0.0_f64;
        let mut x_pow = 1.0_f64;
        for c in result.coefficients_slice() {
            predicted += c * x_pow;
            x_pow *= x[h];
        }
        let err = y[h] - predicted;
        // Flutter weights residuals by w² (lsq_solver.dart).
        let wh_sq = w[h] * w[h];
        sum_squared_error += wh_sq * err * err;
        let v = y[h] - y_mean;
        sum_squared_total += wh_sq * v * v;
    }
    result.confidence = if sum_squared_total <= PRECISION_ERROR_TOLERANCE {
        1.0
    } else {
        // R² = 1 - SSE/SST. Clamp to [0, 1]: floating-point rounding can produce
        // a tiny negative when SSE ≈ SST, and a fit worse than the mean would
        // give R² < 0 — neither is a meaningful "confidence".
        (1.0 - (sum_squared_error / sum_squared_total)).clamp(0.0, 1.0)
    };

    Some(result)
}

/// Fit two right-hand sides (e.g. the x and y pointer coordinates) that share
/// the same sample times `x` and weights `w`. The QR factorization — the
/// dominant O(n²·m) cost — is computed once and reused for both, halving the
/// factorization work versus two independent solves.
// Math-style names (x, w, q, r, m, n) mirror Flutter's lsq_solver.dart.
#[expect(clippy::many_single_char_names)]
pub(crate) fn solve_two(
    x: &[f64],
    w: &[f64],
    y1: &[f64],
    y2: &[f64],
    degree: usize,
) -> (Option<PolynomialFit>, Option<PolynomialFit>) {
    let mut q = [0.0_f64; SCRATCH_N * SCRATCH_M];
    let mut r = [0.0_f64; SCRATCH_N * SCRATCH_N];
    match factorize(x, w, degree, &mut q, &mut r) {
        Some((m, n)) => (
            solve_rhs(x, y1, w, &q, &r, m, n),
            solve_rhs(x, y2, w, &q, &r, m, n),
        ),
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {

    proptest::proptest! {}
}
