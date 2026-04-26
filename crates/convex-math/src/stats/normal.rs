//! Standard normal CDF.

/// Standard normal CDF via Abramowitz & Stegun 26.2.17. Max abs error ~7.5e-8.
#[must_use]
pub fn standard_normal_cdf(x: f64) -> f64 {
    let ax = x.abs();
    let k = 1.0 / (1.0 + 0.231_641_9 * ax);
    let phi = (-0.5 * ax * ax).exp() / (2.0 * std::f64::consts::PI).sqrt();
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
