//! Linear algebra utilities.
//!
//! This module provides matrix operations and decompositions
//! needed for financial calculations.

use crate::error::{MathError, MathResult};
use nalgebra::{DMatrix, DVector};

/// Solves a tridiagonal system of equations efficiently.
///
/// The system has the form:
/// ```text
/// | b[0]  c[0]   0    ...   0   | | x[0]   |   | d[0]   |
/// | a[1]  b[1]  c[1]  ...   0   | | x[1]   |   | d[1]   |
/// |  0    a[2]  b[2]  ...   0   | | x[2]   | = | d[2]   |
/// | ...   ...   ...   ...  ...  | | ...    |   | ...    |
/// |  0     0     0   a[n-1] b[n-1] | | x[n-1] |   | d[n-1] |
/// ```
///
/// # Arguments
///
/// * `a` - Lower diagonal (length n-1)
/// * `b` - Main diagonal (length n)
/// * `c` - Upper diagonal (length n-1)
/// * `d` - Right-hand side (length n)
///
/// # Returns
///
/// Solution vector x.
pub fn solve_tridiagonal(a: &[f64], b: &[f64], c: &[f64], d: &[f64]) -> MathResult<Vec<f64>> {
    let n = b.len();

    if a.len() != n - 1 || c.len() != n - 1 || d.len() != n {
        return Err(MathError::invalid_input(
            "Tridiagonal system has inconsistent dimensions",
        ));
    }

    if n == 0 {
        return Ok(vec![]);
    }

    // Forward elimination
    let mut c_prime = vec![0.0; n];
    let mut d_prime = vec![0.0; n];

    c_prime[0] = c[0] / b[0];
    d_prime[0] = d[0] / b[0];

    for i in 1..n {
        let denom = b[i] - a[i - 1] * c_prime[i - 1];
        if denom.abs() < 1e-15 {
            return Err(MathError::SingularMatrix);
        }

        if i < n - 1 {
            c_prime[i] = c[i] / denom;
        }
        d_prime[i] = (d[i] - a[i - 1] * d_prime[i - 1]) / denom;
    }

    // Back substitution
    let mut x = vec![0.0; n];
    x[n - 1] = d_prime[n - 1];

    for i in (0..n - 1).rev() {
        x[i] = d_prime[i] - c_prime[i] * x[i + 1];
    }

    Ok(x)
}

/// Performs LU decomposition of a square matrix.
///
/// Returns matrices L and U such that A = L * U, where L is lower
/// triangular and U is upper triangular.
pub fn lu_decomposition(matrix: &DMatrix<f64>) -> MathResult<(DMatrix<f64>, DMatrix<f64>)> {
    let n = matrix.nrows();
    if n != matrix.ncols() {
        return Err(MathError::invalid_input("Matrix must be square for LU decomposition"));
    }

    let mut l = DMatrix::identity(n, n);
    let mut u = matrix.clone();

    for k in 0..n {
        if u[(k, k)].abs() < 1e-15 {
            return Err(MathError::SingularMatrix);
        }

        for i in k + 1..n {
            let factor = u[(i, k)] / u[(k, k)];
            l[(i, k)] = factor;

            for j in k..n {
                u[(i, j)] -= factor * u[(k, j)];
            }
        }
    }

    Ok((l, u))
}

/// Solves a linear system Ax = b using LU decomposition.
pub fn solve_linear_system(a: &DMatrix<f64>, b: &DVector<f64>) -> MathResult<DVector<f64>> {
    let n = a.nrows();
    if n != a.ncols() {
        return Err(MathError::invalid_input("Matrix must be square"));
    }
    if n != b.len() {
        return Err(MathError::DimensionMismatch {
            rows1: n,
            cols1: n,
            rows2: b.len(),
            cols2: 1,
        });
    }

    let (l, u) = lu_decomposition(a)?;

    // Solve Ly = b (forward substitution)
    let mut y = DVector::zeros(n);
    for i in 0..n {
        let mut sum = b[i];
        for j in 0..i {
            sum -= l[(i, j)] * y[j];
        }
        y[i] = sum / l[(i, i)];
    }

    // Solve Ux = y (back substitution)
    let mut x = DVector::zeros(n);
    for i in (0..n).rev() {
        let mut sum = y[i];
        for j in i + 1..n {
            sum -= u[(i, j)] * x[j];
        }
        if u[(i, i)].abs() < 1e-15 {
            return Err(MathError::SingularMatrix);
        }
        x[i] = sum / u[(i, i)];
    }

    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_tridiagonal_simple() {
        // Simple 3x3 system
        let a = vec![1.0, 1.0];
        let b = vec![2.0, 2.0, 2.0];
        let c = vec![1.0, 1.0];
        let d = vec![1.0, 2.0, 3.0];

        let x = solve_tridiagonal(&a, &b, &c, &d).unwrap();

        // Verify solution
        assert_relative_eq!(b[0] * x[0] + c[0] * x[1], d[0], epsilon = 1e-10);
        assert_relative_eq!(
            a[0] * x[0] + b[1] * x[1] + c[1] * x[2],
            d[1],
            epsilon = 1e-10
        );
        assert_relative_eq!(a[1] * x[1] + b[2] * x[2], d[2], epsilon = 1e-10);
    }

    #[test]
    fn test_lu_decomposition() {
        let a = DMatrix::from_row_slice(3, 3, &[2.0, 1.0, 1.0, 4.0, 3.0, 3.0, 8.0, 7.0, 9.0]);

        let (l, u) = lu_decomposition(&a).unwrap();

        // Verify L * U = A
        let product = &l * &u;
        for i in 0..3 {
            for j in 0..3 {
                assert_relative_eq!(product[(i, j)], a[(i, j)], epsilon = 1e-10);
            }
        }
    }

    #[test]
    fn test_solve_linear_system() {
        let a = DMatrix::from_row_slice(2, 2, &[2.0, 1.0, 1.0, 3.0]);
        let b = DVector::from_vec(vec![5.0, 5.0]);

        let x = solve_linear_system(&a, &b).unwrap();

        assert_relative_eq!(x[0], 2.0, epsilon = 1e-10);
        assert_relative_eq!(x[1], 1.0, epsilon = 1e-10);
    }
}
