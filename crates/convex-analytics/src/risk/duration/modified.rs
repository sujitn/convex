//! Modified duration. `D_mod = D_mac / (1 + y/f)` for periodic compounding;
//! `D_mod = D_mac` for continuous; `D_mod = D_mac / (1 + y·D_mac)` for simple.

use convex_core::types::Compounding;

use super::{macaulay_duration, Duration};
use crate::error::AnalyticsResult;

fn modified_denominator(ytm: f64, compounding: Compounding, mac_dur: f64) -> f64 {
    match compounding {
        Compounding::Continuous => 1.0,
        Compounding::Simple => 1.0 + ytm * mac_dur,
        _ => 1.0 + ytm / compounding.periods_per_year() as f64,
    }
}

pub fn modified_duration(
    times: &[f64],
    cash_flows: &[f64],
    ytm: f64,
    compounding: Compounding,
) -> AnalyticsResult<Duration> {
    let freq_for_mac = if matches!(compounding, Compounding::Continuous) {
        1
    } else {
        compounding.periods_per_year()
    };
    let mac = macaulay_duration(times, cash_flows, ytm, freq_for_mac)?;
    Ok(modified_from_macaulay(mac, ytm, compounding))
}

pub fn modified_from_macaulay(macaulay: Duration, ytm: f64, compounding: Compounding) -> Duration {
    let denom = modified_denominator(ytm, compounding, macaulay.as_f64());
    Duration::from(macaulay.as_f64() / denom)
}

pub fn price_change_from_duration(mod_duration: Duration, price: f64, yield_change: f64) -> f64 {
    -mod_duration.as_f64() * price * yield_change
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let mod_dur = modified_from_macaulay(Duration::from(5.0), 0.06, Compounding::SemiAnnual);
        assert_relative_eq!(mod_dur.as_f64(), 4.854, epsilon = 0.001);
    }

    #[test]
    fn test_continuous_skips_periodic_divisor() {
        let times = vec![10.0];
        let cfs = vec![100.0];
        let mac = macaulay_duration(&times, &cfs, 0.05, 1).unwrap();
        let m = modified_duration(&times, &cfs, 0.05, Compounding::Continuous).unwrap();
        assert_relative_eq!(m.as_f64(), mac.as_f64(), epsilon = 1e-12);
    }

    #[test]
    fn test_price_change_approximation() {
        let mod_dur = Duration::from(5.0);
        let price = 100.0;
        let yield_change = 0.01; // 100 bps

        let change = price_change_from_duration(mod_dur, price, yield_change);

        // Price should drop by approximately 5% for 100bp yield increase
        assert_relative_eq!(change, -5.0, epsilon = 0.01);
    }
}
