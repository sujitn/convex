//! Curve bootstrap algorithms.
//!
//! This module provides algorithms for constructing yield curves from
//! market instruments.

use rust_decimal::Decimal;

use convex_core::Date;
use convex_math::solvers::{newton_raphson, SolverConfig};

use crate::curves::{ZeroCurve, ZeroCurveBuilder};
use crate::error::{CurveError, CurveResult};
use crate::interpolation::InterpolationMethod;

/// A market instrument for bootstrapping.
#[derive(Debug, Clone)]
pub enum BootstrapInstrument {
    /// Cash deposit rate.
    Deposit {
        /// Maturity date.
        maturity: Date,
        /// Deposit rate (decimal).
        rate: Decimal,
    },
    /// Forward rate agreement.
    Fra {
        /// Start date.
        start: Date,
        /// End date.
        end: Date,
        /// FRA rate (decimal).
        rate: Decimal,
    },
    /// Interest rate swap.
    Swap {
        /// Maturity date.
        maturity: Date,
        /// Fixed leg rate (decimal).
        rate: Decimal,
        /// Payment frequency (periods per year).
        frequency: u32,
    },
}

impl BootstrapInstrument {
    /// Returns the maturity date of the instrument.
    #[must_use]
    pub fn maturity(&self) -> Date {
        match self {
            Self::Deposit { maturity, .. } => *maturity,
            Self::Fra { end, .. } => *end,
            Self::Swap { maturity, .. } => *maturity,
        }
    }
}

/// Bootstraps a zero curve from market instruments.
///
/// # Arguments
///
/// * `reference_date` - Curve valuation date
/// * `instruments` - Market instruments sorted by maturity
/// * `interpolation` - Interpolation method to use
///
/// # Returns
///
/// A zero curve that prices all instruments to par.
pub fn bootstrap_curve(
    reference_date: Date,
    instruments: &[BootstrapInstrument],
    interpolation: InterpolationMethod,
) -> CurveResult<ZeroCurve> {
    if instruments.is_empty() {
        return Err(CurveError::invalid_data("No instruments provided"));
    }

    let mut builder = ZeroCurveBuilder::new()
        .reference_date(reference_date)
        .interpolation(interpolation);

    let mut bootstrapped_dates: Vec<Date> = vec![];
    let mut bootstrapped_rates: Vec<Decimal> = vec![];

    for instrument in instruments {
        let rate = bootstrap_single(
            reference_date,
            instrument,
            &bootstrapped_dates,
            &bootstrapped_rates,
        )?;

        let maturity = instrument.maturity();
        bootstrapped_dates.push(maturity);
        bootstrapped_rates.push(rate);

        builder = builder.add_rate(maturity, rate);
    }

    builder.build()
}

/// Bootstraps a single instrument to find the zero rate.
fn bootstrap_single(
    reference_date: Date,
    instrument: &BootstrapInstrument,
    known_dates: &[Date],
    known_rates: &[Decimal],
) -> CurveResult<Decimal> {
    match instrument {
        BootstrapInstrument::Deposit { maturity, rate } => {
            // For deposits: DF = 1 / (1 + r * t)
            let t = reference_date.days_between(maturity) as f64 / 360.0;
            let r = rate.to_string().parse::<f64>().unwrap_or(0.0);
            let df = 1.0 / (1.0 + r * t);

            // Convert to continuous rate: r_c = -ln(DF) / t
            let t_365 = reference_date.days_between(maturity) as f64 / 365.0;
            let zero_rate = -df.ln() / t_365;

            Ok(Decimal::from_f64_retain(zero_rate).unwrap_or(Decimal::ZERO))
        }

        BootstrapInstrument::Fra { start, end, rate } => {
            // Need to have rates up to start date
            let t_start = reference_date.days_between(start) as f64 / 365.0;
            let t_end = reference_date.days_between(end) as f64 / 365.0;

            // Interpolate to get DF at start (simplified)
            let df_start = if known_dates.is_empty() {
                1.0
            } else {
                // Use the last known rate for interpolation
                let last_rate = known_rates
                    .last()
                    .map(|r| r.to_string().parse::<f64>().unwrap_or(0.0))
                    .unwrap_or(0.0);
                (-last_rate * t_start).exp()
            };

            // FRA implies: DF_end = DF_start / (1 + fra_rate * tau)
            let r = rate.to_string().parse::<f64>().unwrap_or(0.0);
            let tau = start.days_between(end) as f64 / 360.0;
            let df_end = df_start / (1.0 + r * tau);

            // Convert to zero rate
            let zero_rate = -df_end.ln() / t_end;

            Ok(Decimal::from_f64_retain(zero_rate).unwrap_or(Decimal::ZERO))
        }

        BootstrapInstrument::Swap {
            maturity,
            rate,
            frequency,
        } => {
            // Swap bootstrap using Newton-Raphson
            let swap_rate = rate.to_string().parse::<f64>().unwrap_or(0.0);
            let t_mat = reference_date.days_between(maturity) as f64 / 365.0;

            // Initial guess
            let initial_guess = swap_rate;

            // Objective: PV(fixed leg) = PV(floating leg) = 1
            let objective = |zero_rate: f64| {
                // Calculate sum of fixed payments
                let periods = (t_mat * (*frequency as f64)).round() as u32;
                let period_length = 1.0 / (*frequency as f64);

                let mut fixed_leg_pv = 0.0;
                let mut last_df = 1.0;

                for i in 1..=periods {
                    let t_i = (i as f64) * period_length;
                    let df_i = (-zero_rate * t_i).exp();
                    fixed_leg_pv += swap_rate * period_length * df_i;
                    last_df = df_i;
                }

                // Add notional at maturity
                fixed_leg_pv += last_df;

                // Should equal 1 (par swap)
                fixed_leg_pv - 1.0
            };

            let derivative = |zero_rate: f64| {
                let h = 1e-8;
                (objective(zero_rate + h) - objective(zero_rate - h)) / (2.0 * h)
            };

            let config = SolverConfig::new(1e-10, 50);
            let result = newton_raphson(objective, derivative, initial_guess, &config)
                .map_err(|e| CurveError::bootstrap_failed(format!("{maturity}"), e.to_string()))?;

            Ok(Decimal::from_f64_retain(result.root).unwrap_or(Decimal::ZERO))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_bootstrap_deposit() {
        let ref_date = Date::from_ymd(2025, 1, 1).unwrap();

        let instruments = vec![
            BootstrapInstrument::Deposit {
                maturity: Date::from_ymd(2025, 4, 1).unwrap(),
                rate: dec!(0.04),
            },
            BootstrapInstrument::Deposit {
                maturity: Date::from_ymd(2025, 7, 1).unwrap(),
                rate: dec!(0.045),
            },
        ];

        let curve = bootstrap_curve(ref_date, &instruments, InterpolationMethod::Linear).unwrap();

        assert_eq!(curve.dates().len(), 2);

        // Check that rates are reasonable
        for rate in curve.rates() {
            assert!(*rate > Decimal::ZERO && *rate < dec!(0.1));
        }
    }
}
