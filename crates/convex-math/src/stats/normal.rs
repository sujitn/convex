//! Standard normal CDF. Hand-rolled (not `statrs`) so we can keep the math
//! crate dep-light; this single closed-form is all the swaption / Black-76
//! paths need and has tighter error bounds than statrs' default.

use crate::portable;
/// Standard normal CDF via Abramowitz & Stegun 26.2.17. Max abs error ~7.5e-8.
pub fn standard_normal_cdf(x: f64) -> f64 {
    let ax = x.abs();
    let k = 1.0 / (1.0 + 0.231_641_9 * ax);
    let phi = portable::exp(-0.5 * ax * ax) / (2.0 * std::f64::consts::PI).sqrt();
    let poly = k
        * (0.319_381_530
            + k * (-0.356_563_782
                + k * (1.781_477_937 + k * (-1.821_255_978 + k * 1.330_274_429))));
    let upper = 1.0 - phi * poly;
    if x >= 0.0 {
        upper
    } else {
        1.0 - upper
    }
}

/// Inverse standard-normal CDF for a finite probability strictly between 0 and 1.
/// Uses a bounded log-survival solve on `[0,40]`, including subnormal tails.
/// The existing approximate CDF and its original financial bounds are unchanged.
pub fn standard_normal_inverse(probability: f64) -> crate::MathResult<f64> {
    if !probability.is_finite() || probability <= 0.0 || probability >= 1.0 {
        return Err(crate::MathError::invalid_input(
            "normal probability must be finite and strictly between 0 and 1",
        ));
    }
    if probability == 0.5 {
        return Ok(0.0);
    }
    let tail = if probability < 0.5 {
        probability
    } else {
        1.0 - probability
    };
    let log_tail = portable::log(tail);
    let (mut low, mut high) = (0.0_f64, 40.0_f64);
    for _ in 0..80 {
        let mid = low + (high - low) * 0.5;
        if high - low <= 4.0 * f64::EPSILON * (1.0 + mid) {
            return Ok(if probability < 0.5 { -mid } else { mid });
        }
        if normal_log_survival(mid) > log_tail {
            low = mid;
        } else {
            high = mid;
        }
    }
    Err(crate::MathError::convergence_failed(80, high - low))
}

// erfc avoids cancellation in ordinary tails. Beyond26, a decreasing Mills
// series avoids all underflow: every factor is <127/26^2 and work is bounded.
fn normal_log_survival(z: f64) -> f64 {
    if z <= 26.0 {
        return portable::log(0.5 * portable::erfc(z * std::f64::consts::FRAC_1_SQRT_2));
    }
    let inverse_square = 1.0 / (z * z);
    let (mut sum, mut term) = (1.0, 1.0);
    for n in 1..=64 {
        term *= -f64::from(2 * n - 1) * inverse_square;
        let next = sum + term;
        if next == sum {
            break;
        }
        sum = next;
    }
    -0.5 * z * z - portable::log(z) - 0.5 * portable::log(2.0 * std::f64::consts::PI)
        + portable::log(sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_values() {
        let cases = [
            (-2.0, 0.022_750_131_948_179_19),
            (-1.0, 0.158_655_253_931_457_05),
            (0.0, 0.5),
            (1.0, 0.841_344_746_068_542_9),
            (2.0, 0.977_249_868_051_820_8),
            (3.0, 0.998_650_101_968_369_8),
        ];
        for (x, expected) in cases {
            assert!((standard_normal_cdf(x) - expected).abs() < 1e-7);
        }
    }
}
