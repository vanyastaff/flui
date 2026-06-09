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
//! use flui_interaction::processing::lsq_solver::{LeastSquaresSolver, PolynomialFit};
//!
//! // Fit a quadratic (degree=2) to (t, y) with weights w.
//! let x = vec![-100.0, -50.0, 0.0];       // time in ms
//! let y = vec![0.0, 50.0, 100.0];         // position in px
//! let w = vec![0.6, 0.8, 1.0];            // weights (recent = higher)
//!
//! let fit: Option<PolynomialFit> = LeastSquaresSolver::new(&x, &y, &w).solve(2);
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

    /// Number of active coefficients (= `degree + 1`).
    #[inline]
    pub fn len(&self) -> usize {
        self.degree as usize + 1
    }

    /// Always false — `PolynomialFit` always has at least one coefficient
    /// (the constant term). Exists so callers can use `.is_empty()` for
    /// genericity.
    #[inline]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Iterate over the active coefficient slice.
    #[inline]
    pub fn coefficients_slice(&self) -> &[f64] {
        &self.coefficients[..self.len()]
    }

    /// Return the first derivative at the given `x`, i.e. the
    /// instantaneous velocity if `x` is time and the fit is on
    /// position samples.
    ///
    /// For a quadratic `y = a + b·x + c·x²`, the derivative is
    /// `b + 2c·x`. Evaluated at `x = 0` this is just `coefficients[1]`,
    /// which is the velocity the Flutter velocity tracker returns.
    pub fn derivative_at(&self, x: f64) -> f64 {
        // d/dx Σ cᵢ xⁱ = Σ i·cᵢ xⁱ⁻¹, summed for i ≥ 1
        let mut result = 0.0_f64;
        for (i, c) in self.coefficients_slice().iter().enumerate().skip(1) {
            result += (i as f64) * c * x.powi(i as i32 - 1);
        }
        result
    }
}

// ============================================================================
// LeastSquaresSolver
// ============================================================================

/// Weighted least-squares polynomial fitter.
///
/// The constructor takes slices of equal length representing `(x, y, w)`
/// data points. Call [`solve`](Self::solve) with the desired polynomial
/// degree to compute the fit.
///
/// Returns `None` if the data is insufficient (degree > n), linearly
/// dependent, or numerically singular.
#[derive(Debug, Clone)]
pub struct LeastSquaresSolver<'a> {
    x: &'a [f64],
    y: &'a [f64],
    w: &'a [f64],
}

impl<'a> LeastSquaresSolver<'a> {
    /// Create a new solver from x/y/w slices. The slices must be of equal
    /// length; debug builds assert this.
    pub fn new(x: &'a [f64], y: &'a [f64], w: &'a [f64]) -> Self {
        debug_assert_eq!(x.len(), y.len(), "x and y must have the same length");
        debug_assert_eq!(x.len(), w.len(), "x and w must have the same length");
        Self { x, y, w }
    }

    /// Number of data points.
    #[inline]
    pub fn len(&self) -> usize {
        self.x.len()
    }

    /// Returns true if no data points have been provided.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// Fit a polynomial of the given degree to the data.
    ///
    /// Returns `None` if:
    /// - `degree` exceeds the number of data points
    /// - The data is linearly dependent (rank-deficient)
    /// - All weights are zero (degenerate)
    pub fn solve(&self, degree: usize) -> Option<PolynomialFit> {
        let m = self.x.len();
        if m == 0 {
            // No data — can't fit a constant to zero points.
            return None;
        }
        if degree > m {
            // Not enough data to fit a curve.
            return None;
        }
        if degree > MAX_DEGREE {
            // Clamp to MAX_DEGREE — our velocity/predictor hot paths
            // only ever need linear or quadratic fits. Higher-degree
            // fits are numerically dangerous for ≤ MAX_SAMPLES samples.
            return None;
        }
        if m > SCRATCH_M {
            // Defensive bound check — stack buffers below assume m ≤ MAX_SAMPLES.
            // Callers that need bigger fits should use a heap-backed path.
            return None;
        }

        let mut result = PolynomialFit::new(degree)?;

        // Shorthand matching Flutter's notation.
        let n = degree + 1;

        // Step 1: Build weighted Vandermonde matrix A (n × m), row-major.
        // Element (i, h) lives at a[i * m + h]. Using stack-allocated
        // fixed-size buffers (Vec has no SBO) to avoid 4 heap
        // allocations on every velocity() call. m ≤ MAX_SAMPLES and
        // n ≤ MAX_DEGREE+1 are enforced above.
        let mut a = [0.0_f64; SCRATCH_N * SCRATCH_M];
        for h in 0..m {
            let wh = self.w[h];
            a[h] = wh;
            let mut x_pow = self.x[h];
            for i in 1..n {
                a[i * m + h] = wh * x_pow;
                x_pow *= self.x[h];
            }
        }

        // Step 2: Apply Gram-Schmidt to obtain QR decomposition.
        // Q is n × m, R is n × n.
        let mut q = [0.0_f64; SCRATCH_N * SCRATCH_M];
        let mut r = [0.0_f64; SCRATCH_N * SCRATCH_N];
        for j in 0..n {
            // Copy row j of A into Q.
            for h in 0..m {
                q[j * m + h] = a[j * m + h];
            }
            // Orthogonalise Q row j against the previous Q rows for i < j.
            for i in 0..j {
                // dot = Q row j · Q row i
                let mut dot = 0.0_f64;
                for h in 0..m {
                    dot += q[j * m + h] * q[i * m + h];
                }
                // Q row j -= dot · Q row i
                for h in 0..m {
                    q[j * m + h] -= dot * q[i * m + h];
                }
            }

            // Compute Q row j norm.
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
            // R[j, i] = Q row j · A row i for i ≥ j (upper triangular).
            for i in j..n {
                let mut dot = 0.0_f64;
                for h in 0..m {
                    dot += q[j * m + h] * a[i * m + h];
                }
                r[j * n + i] = dot;
            }
        }

        // Step 3: Solve R B = Qᵀ W Y for the coefficients B.
        // Qᵀ W Y is computed inline as Q row i · (W * Y) for each row i.
        // Back-substitute from bottom-right to top-left. w*y fits in a
        // stack buffer alongside the result coefficients.
        let mut wy = [0.0_f64; SCRATCH_M];
        for (h, (&y, &w)) in self.y[..m].iter().zip(self.w[..m].iter()).enumerate() {
            wy[h] = y * w;
        }
        for i in (0..n).rev() {
            let mut sum = 0.0_f64;
            for h in 0..m {
                sum += q[i * m + h] * wy[h];
            }
            // Subtract contributions from already-solved higher coefficients.
            for j in (i + 1)..n {
                sum -= r[i * n + j] * result.coefficients[j];
            }
            let r_ii = r[i * n + i];
            if r_ii.abs() < PRECISION_ERROR_TOLERANCE {
                return None;
            }
            result.coefficients[i] = sum / r_ii;
        }

        // Step 4: Compute R² (confidence).
        let y_mean: f64 = {
            let mut s = 0.0_f64;
            for h in 0..m {
                s += self.y[h];
            }
            s / m as f64
        };
        let mut sum_squared_error = 0.0_f64;
        let mut sum_squared_total = 0.0_f64;
        for h in 0..m {
            // Polynomial evaluation: y ≈ Σ_{i=0..degree} cᵢ xⁱ
            // Use only the active coefficient slice — the trailing
            // `MAX_DEGREE - degree` slots in `result.coefficients` are
            // zero padding and must not contribute.
            let mut predicted = 0.0_f64;
            let mut x_pow = 1.0_f64;
            for c in result.coefficients_slice() {
                predicted += c * x_pow;
                x_pow *= self.x[h];
            }
            let err = self.y[h] - predicted;
            // Flutter weights residuals by w² (line 193 of lsq_solver.dart).
            let wh = self.w[h];
            let wh_sq = wh * wh;
            sum_squared_error += wh_sq * err * err;
            let v = self.y[h] - y_mean;
            sum_squared_total += wh_sq * v * v;
        }
        result.confidence = if sum_squared_total <= PRECISION_ERROR_TOLERANCE {
            1.0
        } else {
            1.0 - (sum_squared_error / sum_squared_total)
        };

        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build (t, v) samples for a perfect line v = a + b·t.
    fn linear_samples(a: f64, b: f64, count: usize, dt: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let x: Vec<f64> = (0..count).map(|i| -((count - 1 - i) as f64) * dt).collect();
        let y: Vec<f64> = x.iter().map(|&t| a + b * t).collect();
        let w = vec![1.0; count];
        (x, y, w)
    }

    #[test]
    fn linear_fit_perfect() {
        // y = 10 + 5t (a=10, b=5)
        let (x, y, w) = linear_samples(10.0, 5.0, 5, 10.0);
        let fit = LeastSquaresSolver::new(&x, &y, &w).solve(1).expect("fits");
        assert!((fit.coefficients[0] - 10.0).abs() < 1e-6);
        assert!((fit.coefficients[1] - 5.0).abs() < 1e-6);
        // Perfect linear data → confidence is 1.0
        assert!((fit.confidence - 1.0).abs() < 1e-6);
    }

    #[test]
    fn quadratic_fit_perfect() {
        // y = 1 + 2t + 3t²
        let xs = [-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0];
        let y: Vec<f64> = xs.iter().map(|&t| 1.0 + 2.0 * t + 3.0 * t * t).collect();
        let w = vec![1.0; xs.len()];
        let fit = LeastSquaresSolver::new(&xs, &y, &w).solve(2).expect("fits");
        assert!((fit.coefficients[0] - 1.0).abs() < 1e-6);
        assert!((fit.coefficients[1] - 2.0).abs() < 1e-6);
        assert!((fit.coefficients[2] - 3.0).abs() < 1e-6);
        assert!((fit.confidence - 1.0).abs() < 1e-6);
    }

    #[test]
    fn linear_fit_noisy() {
        // y ≈ 0 + 100t with noise — fit should approximate the slope.
        let x = vec![-100.0, -75.0, -50.0, -25.0, 0.0];
        let y = vec![-10000.0, -7400.0, -5050.0, -2480.0, 50.0];
        let w = vec![1.0; x.len()];
        let fit = LeastSquaresSolver::new(&x, &y, &w).solve(1).expect("fits");
        // Velocity ≈ 100 px/unit
        assert!(
            (fit.coefficients[1] - 100.0).abs() < 1.0,
            "got slope {}",
            fit.coefficients[1]
        );
        // Confidence should be high (close to 1.0) for ~linear data
        assert!(fit.confidence > 0.99);
    }

    #[test]
    fn insufficient_data_returns_none() {
        // Only 2 points, asking for degree=2.
        let x = vec![0.0, 1.0];
        let y = vec![0.0, 1.0];
        let w = vec![1.0, 1.0];
        assert!(LeastSquaresSolver::new(&x, &y, &w).solve(2).is_none());
    }

    #[test]
    fn degree_clamped_above_max() {
        // Degree 3 is above MAX_DEGREE=2 — should return None.
        let x = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let y = vec![0.0, 1.0, 4.0, 9.0, 16.0];
        let w = vec![1.0; x.len()];
        assert!(LeastSquaresSolver::new(&x, &y, &w).solve(3).is_none());
    }

    #[test]
    fn zero_weights_returns_none() {
        // All weights zero → rank-deficient
        let x = vec![0.0, 1.0, 2.0];
        let y = vec![0.0, 1.0, 4.0];
        let w = vec![0.0, 0.0, 0.0];
        assert!(LeastSquaresSolver::new(&x, &y, &w).solve(1).is_none());
    }

    #[test]
    fn derivative_at_zero() {
        // Quadratic y = 0 + 5t + 2t². Velocity at t=0 is 5.
        let x = vec![-2.0, -1.0, 0.0, 1.0, 2.0];
        let y: Vec<f64> = x.iter().map(|&t| 5.0 * t + 2.0 * t * t).collect();
        let w = vec![1.0; x.len()];
        let fit = LeastSquaresSolver::new(&x, &y, &w).solve(2).expect("fits");
        let v0 = fit.derivative_at(0.0);
        assert!((v0 - 5.0).abs() < 1e-6);
    }

    #[test]
    fn weight_emphasis() {
        // Four points along y = 10x with one outlier at the low-x end:
        // (0, 50), (3, 30), (7, 70), (10, 100). With uniform weights the
        // outlier pulls the slope below 10. With w=[0.01, 1, 1, 1] the
        // outlier is suppressed and the slope is closer to 10.
        let x = vec![0.0, 3.0, 7.0, 10.0];
        let y = vec![50.0, 30.0, 70.0, 100.0];
        let w_uniform = vec![1.0, 1.0, 1.0, 1.0];
        let w_skewed = vec![0.01, 1.0, 1.0, 1.0];
        let fit_u = LeastSquaresSolver::new(&x, &y, &w_uniform)
            .solve(1)
            .expect("fits");
        let fit_s = LeastSquaresSolver::new(&x, &y, &w_skewed)
            .solve(1)
            .expect("fits");
        // Uniform: the (0, 50) outlier sits well above the y=10x trend, so the
        // fitted line has intercept > 0 and slope < 10 to accommodate it.
        assert!(
            fit_u.coefficients[1] < 10.0,
            "uniform slope {} should be < 10 due to outlier",
            fit_u.coefficients[1]
        );
        // Skewed: outlier weight 0.01 effectively removes (0, 50) from the
        // fit, so slope is steeper than uniform.
        assert!(
            fit_s.coefficients[1] > fit_u.coefficients[1],
            "skewed slope {} should exceed uniform slope {}",
            fit_s.coefficients[1],
            fit_u.coefficients[1]
        );
    }

    #[test]
    fn empty_input_returns_none() {
        let x: Vec<f64> = vec![];
        let y: Vec<f64> = vec![];
        let w: Vec<f64> = vec![];
        // No data → degree=0 is "more than 0 points", so still None.
        assert!(LeastSquaresSolver::new(&x, &y, &w).solve(0).is_none());
    }
}
