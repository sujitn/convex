//! Independent validated half-open coupon/spread/gearing/notional intervals.

use super::{validate_span, CashflowError, MAX_SCHEDULE_PERIODS};
use crate::{fixings::validate_rate, Date};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Economic role of an independently dated profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ProfileKind {
    /// Deterministic annualized decimal coupon component.
    Coupon,
    /// Annualized decimal spread component, not basis points.
    Spread,
    /// Multiplier applied to an explicitly resolved index rate.
    Gearing,
    /// Nonnegative absolute principal; signed position lives outside the contract.
    Notional,
}

/// One caller-supplied [start, end) interval. No implicit sorting or truncation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DatedValue {
    /// Inclusive start of the economic value.
    pub start: Date,
    /// Exclusive end of the economic value.
    pub end: Date,
    /// Value in the units declared by ProfileKind.
    #[serde(with = "crate::contracts::decimal_serde")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "crate::contracts::decimal_serde::schema")
    )]
    pub value: Decimal,
}

/// Selection at a dated step; inclusion is never inferred from payment timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundarySide {
    /// Value immediately before the boundary; (start, end].
    Before,
    /// Value on/after the boundary; [start, end).
    After,
}

/// Principal transition with an explicit pre/post-boundary amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct PrincipalChange {
    /// Principal immediately before the effective date.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub before: Decimal,
    /// Principal on/after the effective date (zero at profile maturity).
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub after: Decimal,
    /// Before minus after; negative means a principal advance.
    #[serde(serialize_with = "rust_decimal::serde::str::serialize")]
    pub repayment: Decimal,
}

/// A contiguous validated, immutable piecewise-constant dated profile.
/// It cannot be deserialized directly, bypassing validation; deserialize
/// DatedValue inputs and construct with new().
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatedProfile {
    kind: ProfileKind,
    entries: Vec<DatedValue>,
}

impl DatedProfile {
    /// Reject empty, unsorted, overlapping, gapped or out-of-domain values.
    pub fn new(kind: ProfileKind, entries: Vec<DatedValue>) -> Result<Self, CashflowError> {
        if entries.is_empty() {
            return Err(CashflowError::terms("profile", "must contain an interval"));
        }
        if entries.len() > MAX_SCHEDULE_PERIODS {
            return Err(CashflowError::BoundExceeded("profile entries"));
        }
        for entry in &entries {
            validate_span(entry.start, entry.end)?;
            match kind {
                ProfileKind::Coupon | ProfileKind::Spread => validate_rate(entry.value)?,
                ProfileKind::Gearing if entry.value.abs() > Decimal::from(1_000_000) => {
                    return Err(CashflowError::terms("gearing", "magnitude exceeds 1e6"))
                }
                ProfileKind::Notional
                    if entry.value < Decimal::ZERO
                        || entry.value > Decimal::from(1_000_000_000_000_000_000_i64) =>
                {
                    return Err(CashflowError::terms("notional", "outside 0..1e18"))
                }
                _ => (),
            }
        }
        for pair in entries.windows(2) {
            if pair[0].end != pair[1].start {
                return Err(CashflowError::terms(
                    "profile",
                    "unsorted, overlapping or gapped intervals",
                ));
            }
        }
        let first = entries
            .first()
            .ok_or_else(|| CashflowError::terms("profile", "empty"))?;
        let last = entries
            .last()
            .ok_or_else(|| CashflowError::terms("profile", "empty"))?;
        validate_span(first.start, last.end)?;
        Ok(Self { kind, entries })
    }
    /// Economic role validated at construction.
    pub fn kind(&self) -> ProfileKind {
        self.kind
    }
    /// Original validated intervals.
    pub fn entries(&self) -> &[DatedValue] {
        &self.entries
    }
    /// Coverage interval. It is never extended to hide missing terms.
    pub fn coverage(&self) -> (Date, Date) {
        (
            self.entries[0].start,
            self.entries[self.entries.len() - 1].end,
        )
    }
    /// Select a boundary value explicitly; requests outside coverage fail.
    pub fn value_at(&self, date: Date, side: BoundarySide) -> Result<Decimal, CashflowError> {
        let position = self.entries.partition_point(|entry| match side {
            BoundarySide::After => entry.end <= date,
            BoundarySide::Before => entry.end < date,
        });
        let entry = self
            .entries
            .get(position)
            .ok_or_else(|| CashflowError::terms("profile_date", "outside profile coverage"))?;
        let contains = match side {
            BoundarySide::After => entry.start <= date && date < entry.end,
            BoundarySide::Before => entry.start < date && date <= entry.end,
        };
        if !contains {
            return Err(CashflowError::terms(
                "profile_date",
                "outside profile coverage",
            ));
        }
        Ok(entry.value)
    }
    /// Principal at a step, including full redemption at the end of coverage.
    /// A rate profile cannot accidentally be treated as a principal schedule.
    pub fn principal_change(&self, date: Date) -> Result<PrincipalChange, CashflowError> {
        if self.kind != ProfileKind::Notional {
            return Err(CashflowError::terms(
                "principal",
                "requires a Notional profile",
            ));
        }
        let before = self.value_at(date, BoundarySide::Before)?;
        let after = if date == self.coverage().1 {
            Decimal::ZERO
        } else {
            self.value_at(date, BoundarySide::After)?
        };
        let repayment = before
            .checked_sub(after)
            .ok_or(CashflowError::Arithmetic("principal difference"))?;
        Ok(PrincipalChange {
            before,
            after,
            repayment,
        })
    }
}
