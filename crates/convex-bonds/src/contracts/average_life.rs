//! A bond's average life: the time to each repayment of its principal, weighted by the face
//! each retires.

use convex_core::{cashflows::BoundarySide, Date};
use convex_math::decimal::BoundedDecimal;
use rust_decimal::Decimal;

use super::{invalid, BondContract, BondContractError};

/// Days in a year on the average life's time axis, ACT/365 fixed: the axis supplied
/// government yields are interpolated on.
const DAYS_PER_YEAR: i64 = 365;

/// A bond's average life from a settlement date, in years of 365 days.
///
/// Each change in outstanding principal after settlement is weighted by the face it retires
/// and timed at its contractual date, not the business day it pays, so a bullet's average
/// life is the time to its maturity date.
/// The time is exact until one final nearest/even rounding to a `Decimal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AverageLife {
    settlement: Date,
    years: Decimal,
}

impl AverageLife {
    /// The settlement date it is measured from.
    #[must_use]
    pub fn settlement(&self) -> Date {
        self.settlement
    }

    /// Years of 365 days from settlement.
    #[must_use]
    pub fn years(&self) -> Decimal {
        self.years
    }
}

impl BondContract {
    /// The average life of the principal outstanding after `settlement`: the face each
    /// later change in its outstanding principal retires, times the days from settlement
    /// to the change's contractual date, summed and divided by 365 times the face
    /// outstanding.
    ///
    /// It reads the outstanding principal, not its cash: a sink priced above par retires
    /// the same face as one at par. Calls and puts are not exercised; the life is that of
    /// the scheduled principal.
    ///
    /// # Errors
    ///
    /// An invalid-terms error when no principal is outstanding after `settlement` (before
    /// issue, or at or after the final redemption), or when an amount exceeds what the
    /// exact sums hold.
    pub fn average_life(&self, settlement: Date) -> Result<AverageLife, BondContractError> {
        let outstanding = self.outstanding_principal(settlement, BoundarySide::After)?;
        if outstanding <= Decimal::ZERO {
            return Err(invalid(
                "settlement",
                "no principal is outstanding after it",
            ));
        }
        let overflow = |_| invalid("average_life", "exceeds what the exact sums hold");
        let notional = self.profiles.notional();
        let mut weighted_days = BoundedDecimal::zero();
        for change_date in notional
            .entries()
            .iter()
            .map(|entry| entry.end)
            .filter(|end| *end > settlement)
        {
            let retired =
                BoundedDecimal::decimal(notional.principal_change(change_date)?.repayment);
            let days = Decimal::from(settlement.days_between(&change_date));
            weighted_days
                .add(&retired.multiply(days).map_err(overflow)?)
                .map_err(overflow)?;
        }
        // The profile is contiguous, so the face retired after settlement is the face
        // outstanding.
        let year_face = BoundedDecimal::decimal(outstanding)
            .multiply(Decimal::from(DAYS_PER_YEAR))
            .map_err(overflow)?;
        Ok(AverageLife {
            settlement,
            years: weighted_days.ratio(&year_face).map_err(overflow)?,
        })
    }
}
