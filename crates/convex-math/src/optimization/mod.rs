//! Optimization helpers used by the analytics calibration paths.

/// Golden-section minimiser on `[a, b]`. Robust for unimodal smooth objectives.
/// Returns the argmin.
pub fn golden_section<F: Fn(f64) -> f64>(
    f: F,
    mut a: f64,
    mut b: f64,
    tol: f64,
    max_iter: usize,
) -> f64 {
    let phi = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut c = b - phi * (b - a);
    let mut d = a + phi * (b - a);
    let mut fc = f(c);
    let mut fd = f(d);
    for _ in 0..max_iter {
        if (b - a).abs() < tol {
            break;
        }
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - phi * (b - a);
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + phi * (b - a);
            fd = f(d);
        }
    }
    0.5 * (a + b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_section_quadratic() {
        let opt = golden_section(|x: f64| (x - 3.0).powi(2), -10.0, 10.0, 1e-9, 200);
        assert!((opt - 3.0).abs() < 1e-6);
    }
}
