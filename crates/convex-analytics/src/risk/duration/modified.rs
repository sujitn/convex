//! Complete-cash modified duration; Simple needs each original cash denominator.
use super::Duration;
use crate::error::{AnalyticsError, AnalyticsResult};
use convex_core::types::Compounding;

/// Minus the native first derivative divided by the same complete cash PV.
pub fn modified_duration(
    times: &[f64],
    cash_flows: &[f64],
    ytm: f64,
    compounding: Compounding,
) -> AnalyticsResult<Duration> {
    let risk = crate::risk::cash::evaluate(times, cash_flows, ytm, compounding)?;
    Ok(Duration::new(crate::risk::cash::decimal(
        risk.modified_duration,
    )?))
}

/// Convert a Macaulay duration under periodic or continuous compounding.
/// Simple interest requires complete cash; an aggregate time cannot recover its derivative.
pub fn modified_from_macaulay(
    macaulay: Duration,
    ytm: f64,
    compounding: Compounding,
) -> AnalyticsResult<Duration> {
    if !ytm.is_finite() || compounding == Compounding::Simple {
        return Err(AnalyticsError::InvalidInput(
            "finite yield and periodic/continuous basis required; Simple needs complete cash"
                .into(),
        ));
    }
    let denominator = if compounding == Compounding::Continuous {
        1.0
    } else {
        1.0 + ytm / f64::from(compounding.periods_per_year())
    };
    if !denominator.is_finite() || denominator <= 0.0 {
        return Err(AnalyticsError::InvalidInput(
            "invalid periodic yield domain".into(),
        ));
    }
    Ok(Duration::new(crate::risk::cash::decimal(
        macaulay.as_f64() / denominator,
    )?))
}

/// First-order price change: `ΔP ≈ −D_mod · P · Δy`.
pub fn price_change_from_duration(
    mod_duration: Duration,
    price: f64,
    yield_change: f64,
) -> AnalyticsResult<f64> {
    convex_bonds::pricing::risk_math::duration_price_change(
        mod_duration.as_f64(),
        price,
        yield_change,
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::macaulay_duration;
    use approx::assert_relative_eq;

    #[test]
    fn test_modified_duration_par_bond() {
        let times = vec![0.5, 1.0, 1.5, 2.0];
        let cash_flows = vec![2.5, 2.5, 2.5, 102.5];

        let dur = modified_duration(&times, &cash_flows, 0.05, Compounding::SemiAnnual).unwrap();
        assert_relative_eq!(dur.as_f64(), 1.88, epsilon = 0.01);
    }

    #[test]
    fn test_modified_from_macaulay() {
        let mod_dur = modified_from_macaulay(
            Duration::try_from(5.0).unwrap(),
            0.06,
            Compounding::SemiAnnual,
        )
        .unwrap();
        assert_relative_eq!(mod_dur.as_f64(), 4.854, epsilon = 0.001);
    }

    #[test]
    fn test_continuous_skips_periodic_divisor() {
        let times = vec![10.0];
        let cfs = vec![100.0];
        let mac = macaulay_duration(&times, &cfs, 0.05, Compounding::Continuous).unwrap();
        let m = modified_duration(&times, &cfs, 0.05, Compounding::Continuous).unwrap();
        assert_relative_eq!(m.as_f64(), mac.as_f64(), epsilon = 1e-12);
    }

    #[test]
    fn test_price_change_approximation() {
        let mod_dur = Duration::try_from(5.0).unwrap();
        let price = 100.0;
        let yield_change = 0.01; // 100 bps

        let change = price_change_from_duration(mod_dur, price, yield_change).unwrap();

        // Price should drop by approximately 5% for 100bp yield increase
        assert_relative_eq!(change, -5.0, epsilon = 0.01);
    }
}
