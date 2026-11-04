//! Contract-aware, pure futures and cash hedge economics on resolved inputs.
//!
//! Every result remains indicative. Input admission, model valuation and actual
//! settlement are distinct; these functions never authorize an order or post cash.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod bond;
mod cash;
mod graph;
mod rates;
mod settlement;

pub use bond::*;
pub use cash::*;
pub use graph::*;
pub use rates::*;
pub use settlement::*;

use convex_bonds::contracts::{BondContract, BondContractError};
use convex_core::{
    calendars::Calendar,
    cashflows::CashflowError,
    contracts::{BusinessDate, ContractError, ExactDecimal, Money},
    futures::*,
    Currency, Date,
};
use convex_curves::contracts::{BuiltCurve, CurveContractError};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::{Deserialize, Serialize};

use crate::risk::HedgeExecutionStatus;

/// Explicit domain failures; unsupported/missing inputs never become a price.
#[derive(Debug, thiserror::Error)]
pub enum FuturesError {
    /// Invalid or missing economic input.
    #[error("{field}: {reason}")]
    Invalid {
        /// Original input field.
        field: &'static str,
        /// Economic explanation.
        reason: String,
    },
    /// Original checked reference/unit failure.
    #[error(transparent)]
    Reference(#[from] ContractError),
    /// Original history/schedule/arithmetic failure.
    #[error(transparent)]
    Cashflow(#[from] CashflowError),
    /// Original bond contract failure.
    #[error(transparent)]
    Bond(#[from] BondContractError),
    /// Original immutable curve query failure.
    #[error(transparent)]
    Curve(#[from] CurveContractError),
}

/// Pure valuation result or an explicit failure.
pub type FuturesResult<T> = Result<T, FuturesError>;

fn invalid(field: &'static str, reason: impl Into<String>) -> FuturesError {
    FuturesError::Invalid {
        field,
        reason: reason.into(),
    }
}
fn arithmetic<T>(value: Option<T>, field: &'static str) -> FuturesResult<T> {
    value.ok_or_else(|| invalid(field, "checked arithmetic overflow or division by zero"))
}
fn number(value: Decimal) -> FuturesResult<f64> {
    value
        .to_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid("number", "nonfinite Decimal conversion"))
}
fn decimal(value: f64) -> FuturesResult<Decimal> {
    if !value.is_finite() {
        return Err(invalid("number", "nonfinite model output"));
    }
    let result = Decimal::from_f64_retain(value)
        .ok_or_else(|| invalid("number", "Decimal conversion overflow"))?;
    if value != 0.0 && result.is_zero() {
        return Err(invalid("number", "nonzero value underflows Decimal"));
    }
    Ok(result)
}
fn money(value: Decimal, currency: Currency) -> Money {
    Money::new(ExactDecimal::new(value), currency)
}
fn date(context: EligibilityContext) -> Date {
    Date::from(context.as_of.datetime().date_naive())
}
fn count(contracts: i64, zero_allowed: bool) -> FuturesResult<Decimal> {
    if (!zero_allowed && contracts == 0) || contracts.unsigned_abs() > 1_000_000_000 {
        return Err(invalid(
            "contracts",
            "bounded signed integer contracts required",
        ));
    }
    Ok(Decimal::from(contracts))
}
fn positive(value: Decimal, field: &'static str) -> FuturesResult<()> {
    if value <= Decimal::ZERO || value > Decimal::from(1_000_000_000_000_000i64) {
        return Err(invalid(field, "expected positive bounded amount"));
    }
    Ok(())
}
fn rate(value: Decimal, field: &'static str) -> FuturesResult<()> {
    if value.abs() > Decimal::ONE {
        return Err(invalid(field, "decimal rate outside [-1,1]"));
    }
    Ok(())
}

/// Actual supplied calendar and its immutable reference revision.
#[derive(Clone, Copy)]
pub struct FuturesCalendar<'a> {
    /// Actual source/calendar version; checked against the contracted pin.
    pub stamp: &'a ReferenceStamp,
    /// Caller-resolved calendar, without lookup or default market selection.
    pub calendar: &'a dyn Calendar,
}
impl FuturesCalendar<'_> {
    fn check(&self, expected: &ResourcePin, report: &mut FuturesInputReport) -> FuturesResult<()> {
        if self.stamp.id != expected.id || self.stamp.version != expected.version {
            return Err(invalid(
                "calendar",
                "exact contracted calendar revision required",
            ));
        }
        report.add(self.stamp)
    }
}

/// One actual checked security; the caller binds its full terms to the exact pin.
#[derive(Clone, Copy)]
pub struct FuturesBond<'a> {
    /// Actual internal security identity and immutable economic version.
    pub instrument: &'a InstrumentPin,
    /// Original security input lineage, retaining historical/synthetic quality.
    pub stamp: &'a ReferenceStamp,
    /// Complete checked supplied cashflow contract, never a tenor/coupon proxy.
    pub bond: &'a BondContract,
}
impl FuturesBond<'_> {
    fn check(&self, currency: Currency, report: &mut FuturesInputReport) -> FuturesResult<()> {
        if self.stamp.id.as_str() != self.instrument.id.as_str()
            || self.stamp.version != self.instrument.version
            || self.bond.terms().currency != currency
        {
            return Err(invalid(
                "security",
                "exact actual security version/currency required",
            ));
        }
        self.bond.require_plain_valuation()?;
        report.add(self.stamp)
    }
}

/// Exact immutable curve input and actual source quality. Numeric reference
/// versions use their decimal string in CurveKey; no latest lookup is performed.
#[derive(Clone, Copy)]
pub struct FuturesCurve<'a> {
    /// Complete immutable checked curve and construction/query conventions.
    pub curve: &'a BuiltCurve,
    /// Same curve ID and numeric version, plus data availability/quality.
    pub stamp: &'a ReferenceStamp,
}
impl FuturesCurve<'_> {
    fn check(&self, report: &mut FuturesInputReport) -> FuturesResult<()> {
        let key = &self.curve.recipe().key;
        if key.id != self.stamp.id.as_str() || key.version != self.stamp.version.get().to_string() {
            return Err(invalid(
                "curve",
                "exact curve/source identity and numeric revision required",
            ));
        }
        report.add(self.stamp)
    }
}

/// Consumed actual input stamps, deterministic data blocks and execution status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FuturesInputReport {
    /// Economic instant and information cutoff used throughout the result.
    pub context: EligibilityContext,
    /// All actually consumed immutable resource revisions, bounded to 1,024.
    pub stamps: Vec<ReferenceStamp>,
    /// Necessary source prerequisites, never an execution certificate.
    pub assessment: HedgeInputEligibility,
    /// All pure outputs remain indicative, including those with eligible inputs.
    pub execution_status: HedgeExecutionStatus,
}
impl FuturesInputReport {
    fn new(context: EligibilityContext, assessment: HedgeInputEligibility) -> Self {
        Self {
            context,
            stamps: Vec::new(),
            assessment,
            execution_status: HedgeExecutionStatus::IndicativeOnly,
        }
    }
    fn add(&mut self, stamp: &ReferenceStamp) -> FuturesResult<()> {
        stamp.validate()?;
        // A later-known or future-effective source cannot contribute even to
        // historical model valuation. Stale/archived data remains labelled.
        if stamp.available_at > self.context.known_at || stamp.source_at > self.context.as_of {
            return Err(invalid(
                "input_time",
                "future-effective or later-known source refused",
            ));
        }
        if let Some(old) = self
            .stamps
            .iter()
            .find(|s| s.id == stamp.id && s.version == stamp.version)
        {
            if old != stamp {
                return Err(invalid(
                    "input_version",
                    "one revision has conflicting source facts",
                ));
            }
            return Ok(());
        }
        if self.stamps.len() >= 1024 {
            return Err(invalid("inputs", "at most1024 consumed stamps"));
        }
        let reason = match stamp.quality {
            ReferenceQuality::Exchange | ReferenceQuality::Provider => None,
            ReferenceQuality::Synthetic => Some(HedgeInputBlock::Synthetic),
            ReferenceQuality::Indicative => Some(HedgeInputBlock::Indicative),
            ReferenceQuality::Historical => Some(HedgeInputBlock::Historical),
        };
        if let Some(reason) = reason {
            self.block(stamp, reason);
        }
        if stamp.valid_until <= self.context.as_of {
            self.block(stamp, HedgeInputBlock::Stale);
        }
        self.stamps.push(stamp.clone());
        Ok(())
    }
    fn block(&mut self, stamp: &ReferenceStamp, reason: HedgeInputBlock) {
        self.assessment.issues.push(HedgeInputIssue {
            resource: stamp.id.clone(),
            reason,
        });
    }
    fn finish(&mut self) {
        self.stamps
            .sort_by(|a, b| (&a.id, a.version).cmp(&(&b.id, b.version)));
        self.assessment.issues.sort();
        self.assessment.issues.dedup();
        self.assessment.eligible = self.assessment.issues.is_empty();
    }
}

fn contract_report(
    contract: &FuturesContract,
    basket: Option<&DeliveryBasket>,
    context: EligibilityContext,
) -> FuturesResult<FuturesInputReport> {
    let assessment = assess_futures_reference(contract, basket, context)?;
    let mut report = FuturesInputReport::new(context, assessment);
    report.add(&contract.stamp)?;
    if let Some(basket) = basket {
        report.add(&basket.stamp)?;
    }
    Ok(report)
}
