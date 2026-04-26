//! ARRC compound-in-arrears for SOFR FRNs (and SONIA / €STR analogues).
//! Spread-additive convention; matches QL `OvernightIndexedCoupon`.
//! See ARRC FRN conventions doc and Brigo–Mercurio §1.4.

use rust_decimal::Decimal;

use convex_core::calendars::Calendar;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::Date;

use crate::fixings::OvernightFixings;

#[derive(Debug, Clone, Copy)]
pub struct ArrcConfig {
    pub observation_shift: bool,
    pub lookback_days: u32,
    pub lockout_days: u32,
}

impl ArrcConfig {
    /// USD corporate SOFR FRN: observation-shift, 2BD lookback, 0 lockout.
    #[must_use]
    pub fn usd_corporate_sofr() -> Self {
        Self {
            observation_shift: true,
            lookback_days: 2,
            lockout_days: 0,
        }
    }
}

/// Compounded period: `period_factor = Π(1 + r_d × τ_d)`. Subtract one
/// for the period interest amount per unit face (excluding spread).
#[derive(Debug, Clone, Copy)]
pub struct ArrcCompounded {
    pub period_factor: Decimal,
    pub period_year_fraction: Decimal,
}

impl ArrcCompounded {
    #[must_use]
    pub fn compounded_rate_minus_one(&self) -> Decimal {
        self.period_factor - Decimal::ONE
    }
}

/// Compound `[accrual_start, accrual_end)` under ARRC mechanics.
///
/// `daily_forward(d)` is called for business days that have no published
/// fixing in `fixings` (or whose fixing date is past the registry's
/// `as_of` cutoff). It returns the *annualized* rate to apply on
/// `[d, next_bd]`, typically `(DF(d)/DF(d⁺) − 1) / yf(d, d⁺)`.
pub fn compound_in_arrears<F>(
    accrual_start: Date,
    accrual_end: Date,
    day_count: DayCountConvention,
    calendar: &dyn Calendar,
    config: ArrcConfig,
    fixings: &OvernightFixings,
    mut daily_forward: F,
) -> ArrcCompounded
where
    F: FnMut(Date) -> Decimal,
{
    let dc = day_count.to_day_count();
    let period_yf =
        Decimal::try_from(dc.year_fraction(accrual_start, accrual_end)).unwrap_or(Decimal::ZERO);

    if accrual_end <= accrual_start {
        return ArrcCompounded {
            period_factor: Decimal::ONE,
            period_year_fraction: period_yf,
        };
    }

    let lookback = config.lookback_days as i32;
    let (obs_start, obs_end) = if config.observation_shift {
        (
            calendar.add_business_days(accrual_start, -lookback),
            calendar.add_business_days(accrual_end, -lookback),
        )
    } else {
        (accrual_start, accrual_end)
    };

    let lockout = config.lockout_days as i32;
    let freeze_after = (lockout > 0).then(|| calendar.add_business_days(obs_end, -lockout));

    let mut factor = Decimal::ONE;
    let mut iter_day = if calendar.is_business_day(obs_start) {
        obs_start
    } else {
        calendar.add_business_days(obs_start, 1)
    };

    while iter_day < obs_end {
        let next_bd = calendar.add_business_days(iter_day, 1);
        let weight_end = next_bd.min(obs_end);
        if iter_day.days_between(&weight_end) <= 0 {
            break;
        }
        let tau =
            Decimal::try_from(dc.year_fraction(iter_day, weight_end)).unwrap_or(Decimal::ZERO);

        let fixing_day = match freeze_after {
            Some(freeze) if iter_day > freeze => freeze,
            _ => iter_day,
        };
        let rate = fixings
            .lookup(fixing_day)
            .unwrap_or_else(|| daily_forward(fixing_day));

        factor *= Decimal::ONE + rate * tau;
        iter_day = next_bd;
    }

    ArrcCompounded {
        period_factor: factor,
        period_year_fraction: period_yf,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::calendars::WeekendCalendar;
    use rust_decimal_macros::dec;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn flat_arrc() -> ArrcConfig {
        ArrcConfig {
            observation_shift: false,
            lookback_days: 0,
            lockout_days: 0,
        }
    }

    #[test]
    fn flat_3pct_one_week_compounds_correctly() {
        let mut fix = OvernightFixings::new();
        for day in 8..=12 {
            fix.insert(d(2025, 12, day), dec!(0.03));
        }
        let result = compound_in_arrears(
            d(2025, 12, 8),
            d(2025, 12, 15),
            DayCountConvention::Act360,
            &WeekendCalendar,
            flat_arrc(),
            &fix,
            |_| dec!(0),
        );
        // Mon-Thu carry 1/360 each, Fri carries 3/360 (over weekend).
        let expected = (1.0_f64 + 0.03 / 360.0).powi(4) * (1.0 + 0.03 * 3.0 / 360.0);
        let got: f64 = result.period_factor.try_into().unwrap();
        assert!((got - expected).abs() < 1e-12);
    }

    #[test]
    fn missing_fixing_falls_back_to_curve() {
        let result = compound_in_arrears(
            d(2025, 12, 8),
            d(2025, 12, 9),
            DayCountConvention::Act360,
            &WeekendCalendar,
            flat_arrc(),
            &OvernightFixings::new(),
            |_| dec!(0.04),
        );
        let got: f64 = result.period_factor.try_into().unwrap();
        assert!((got - (1.0 + 0.04 / 360.0)).abs() < 1e-15);
    }

    #[test]
    fn as_of_cutoff_ignores_post_valuation_fixings() {
        let mut fix = OvernightFixings::new();
        for day in 8..=12 {
            fix.insert(d(2025, 12, day), dec!(0.05));
        }
        let fix = fix.with_as_of(d(2025, 12, 9));
        let result = compound_in_arrears(
            d(2025, 12, 8),
            d(2025, 12, 15),
            DayCountConvention::Act360,
            &WeekendCalendar,
            flat_arrc(),
            &fix,
            |_| dec!(0.10),
        );
        // Dec 8-9 use fixings (0.05); Dec 10-12 fall through to curve (0.10).
        let expected = (1.0_f64 + 0.05 / 360.0).powi(2)
            * (1.0_f64 + 0.10 / 360.0).powi(2)
            * (1.0_f64 + 0.10 * 3.0 / 360.0);
        let got: f64 = result.period_factor.try_into().unwrap();
        assert!((got - expected).abs() < 1e-15);
    }

    #[test]
    fn observation_shift_uses_earlier_fixings() {
        let mut fix = OvernightFixings::new();
        // 5% on the obs days (Dec 3-5), 10% on accrual days (Dec 8-9).
        // 2BD shift on accrual [Dec 8, Dec 9) → obs [Dec 4, Dec 5).
        for day in [3, 4, 5, 8, 9] {
            fix.insert(
                d(2025, 12, day),
                if day < 6 { dec!(0.05) } else { dec!(0.10) },
            );
        }
        let result = compound_in_arrears(
            d(2025, 12, 8),
            d(2025, 12, 9),
            DayCountConvention::Act360,
            &WeekendCalendar,
            ArrcConfig {
                observation_shift: true,
                lookback_days: 2,
                lockout_days: 0,
            },
            &fix,
            |_| dec!(0),
        );
        let got: f64 = result.period_factor.try_into().unwrap();
        assert!((got - (1.0 + 0.05 / 360.0)).abs() < 1e-15);
    }
}
