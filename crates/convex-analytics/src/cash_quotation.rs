//! Full supplied cash quotations and actual-security government spreads.
//! Conversion, entitlement and inverses reuse the original native pricers.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{cash_calibration as c, quote_risk as q, workflow as w};
use convex_core::{contracts::ModelMoney, Date};
use convex_rates::ReferencePin;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

/// Original conversion or inverse failure, without an accepted partial quote.
#[derive(Debug, thiserror::Error)]
pub enum CashQuotationError {
    /// Original full helper, calendar, quote and settlement admission.
    #[error(transparent)]
    Cash(#[from] c::CashCalibrationError),
    /// Original product, inverse, position or explicit selection failure.
    #[error(transparent)]
    Workflow(#[from] w::WorkflowError),
}
/// Pure quotation without lookup, posting or execution authority.
pub type CashQuotationResult<T> = Result<T, CashQuotationError>;
fn invalid(field: &'static str, reason: &'static str) -> CashQuotationError {
    w::WorkflowError::Invalid {
        field,
        reason: reason.into(),
    }
    .into()
}
fn finite(value: f64, field: &'static str) -> CashQuotationResult<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid(field, "nonfinite quotation arithmetic"))
    }
}
fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

/// Complete original quotation and actual same-cash yield postcondition.
#[derive(Debug, Clone, Serialize)]
pub struct CashSecurityQuotation {
    /// Original helper economics, quote units/revision and calendar identities.
    pub source: c::CashBondHelper,
    /// Original observed clean/dirty/accrued conversion, before inverse tolerance.
    pub conversion: c::CashQuoteConversion,
    /// Full native inverse, cash, final residual, controls and actual work.
    pub yield_inverse: w::BondInverse,
}
/// Quote full fixed/stepped/amortizing/bill cash. No Street/workout yield,
/// floating projection or optional valuation convention is inferred.
pub fn quote_cash_security(
    helper: &c::CashBondHelper,
    resources: &c::CashHelperResources<'_>,
    reference_date: Date,
    convention: w::DatedYieldConvention,
    settings: w::BondInverseSettings,
) -> CashQuotationResult<CashSecurityQuotation> {
    let conversion = c::convert_cash_quote(helper, resources, reference_date)?;
    let c::CashHelperResources::Bond { calendars, .. } = resources else {
        return Err(invalid(
            "resources",
            "full independently supplied bond calendars required",
        ));
    };
    let contract = helper
        .terms
        .clone()
        .build_with_calendars(calendars)
        .map_err(w::WorkflowError::from)?;
    let cash = w::PreparedBondCash::fixed(&contract, helper.settlement, helper.on_payment)?;
    let yield_inverse = w::solve_bond_yield(
        &cash,
        convention,
        w::BondPriceTarget::Dirty(conversion.dirty_per100),
        settings,
    )?;
    Ok(CashSecurityQuotation {
        source: helper.clone(),
        conversion,
        yield_inverse,
    })
}
/// Signed unrounded settlement consideration, separate from cash posting.
#[derive(Debug, Clone, Serialize)]
pub struct CashSecurityInvoice {
    /// Complete computed quotation; no cached caller-created price is accepted.
    pub quotation: CashSecurityQuotation,
    /// Original signed face/reference size, with exact native quantity units.
    pub position: q::RiskPosition,
    /// Original dirty quote times signed face, scaled once. Positive notional
    /// means positive consideration; this is not an outgoing ledger cash entry.
    pub consideration: ModelMoney,
}
/// Use the ORIGINAL observed dirty price, not approximate inverse repricing.
/// No ledger rounding, receipt, fee, settlement authorization or posting occurs.
pub fn cash_security_invoice(
    helper: &c::CashBondHelper,
    resources: &c::CashHelperResources<'_>,
    reference_date: Date,
    convention: w::DatedYieldConvention,
    settings: w::BondInverseSettings,
    position: &q::RiskPosition,
) -> CashQuotationResult<CashSecurityInvoice> {
    if position.currency != helper.terms.currency
        || position.quantity_unit != q::RiskQuantityUnit::Notional
        || position.reference_quantity.decimal() != helper.terms.face_value
    {
        return Err(invalid(
            "position",
            "exact currency, Notional units and original face reference required",
        ));
    }
    let scale = position.scale().map_err(w::WorkflowError::from)?;
    let quotation = quote_cash_security(helper, resources, reference_date, convention, settings)?;
    let face = helper
        .terms
        .face_value
        .to_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| invalid("face", "finite positive original face required"))?;
    let value = finite(
        quotation.conversion.dirty_per100 * face / 100.0 * scale,
        "consideration",
    )?;
    if scale != 0.0 && value == 0.0 {
        return Err(invalid("consideration", "nonzero position cash underflow"));
    }
    let consideration =
        q::risk_reference_pv(value, helper.terms.currency).map_err(w::WorkflowError::from)?;
    Ok(CashSecurityInvoice {
        quotation,
        position: position.clone(),
        consideration,
    })
}

/// Caller-resolved government reference classification and complete actual quote.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GovernmentSecurityQuote {
    /// Exact security identity/reference revision; identity must equal helper.id.
    pub security: ReferencePin,
    /// Actual benchmark membership; no issuer/provider inference is performed.
    pub benchmark_id: String,
    /// Full security terms, quote revision, actual settlement and entitlement.
    pub helper: c::CashBondHelper,
}
/// Independently bound actual calendars, without a repository handle.
pub struct GovernmentSecurityInputs<'a> {
    /// Immutable reference and original quote.
    pub quote: &'a GovernmentSecurityQuote,
    /// Actual calendars matching the security's own original identities.
    pub resources: &'a c::CashHelperResources<'a>,
}
/// Explicit security selection; no flat endpoint or nominal-tenor fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum GovernmentSelection {
    /// One supplied security at the exact target contractual maturity date.
    ExactMaturity,
    /// Adjacent securities linearly weighted by actual contractual maturity days.
    /// Only the closed supplied maturity interval is admitted.
    LinearMaturity,
}
/// Common dated quotation and original inverse controls for every security.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GovernmentSpreadSettings {
    /// Exact benchmark classification expected on every supplied security.
    pub benchmark_id: String,
    /// Every observed price is requoted under this same dated-cash convention.
    pub convention: w::DatedYieldConvention,
    /// Original inverse controls; failures propagate without relaxed retries.
    pub inverse: w::BondInverseSettings,
    /// Explicit actual-maturity selection/interpolation policy.
    pub selection: GovernmentSelection,
}
/// Computed benchmark retaining independent reference and quote revisions.
#[derive(Debug, Clone, Serialize)]
pub struct GovernmentBenchmarkQuotation {
    /// Original security reference pin.
    pub security: ReferencePin,
    /// Full original helper and conversion/inverse diagnostics.
    pub quotation: CashSecurityQuotation,
}
/// Source indices and actual calendar-day interpolation weights.
#[derive(Debug, Clone, Serialize)]
pub struct GovernmentSelectionResult {
    /// Lower/exact source index in the original-order output array.
    pub lower_index: usize,
    /// Upper source; absent for an exact maturity match.
    pub upper_index: Option<usize>,
    /// Lower security contribution.
    pub lower_weight: f64,
    /// Upper contribution, zero for an exact match.
    pub upper_weight: f64,
    /// Actual target contractual maturity, independent of delayed helper pillar.
    pub target_maturity: Date,
    /// Actual settlement-to-maturity calendar days used for selection.
    pub target_remaining_days: i64,
}
/// Fractional actual-security G spread; a government zero curve cannot substitute.
#[derive(Debug, Clone, Serialize)]
pub struct GovernmentSpreadQuotation {
    /// Actual supplied information origin.
    pub reference_date: Date,
    /// Common quotation, numerical and selection policy actually used.
    pub settings: GovernmentSpreadSettings,
    /// Full target observed quotation and inverse.
    pub target: CashSecurityQuotation,
    /// All supplied benchmarks in ORIGINAL source order, without silent dropping.
    pub benchmarks: Vec<GovernmentBenchmarkQuotation>,
    /// Actual selected indices/dates/weights.
    pub selected: GovernmentSelectionResult,
    /// Selected/interpolated decimal yield in the common output convention.
    pub benchmark_decimal_yield: f64,
    /// Target minus benchmark annual decimal yield, without bp rounding.
    pub decimal_spread: f64,
    /// The same difference times10,000, preserving fractional basis points.
    pub basis_points: f64,
}
/// Full securities under a supplied common dated-cash convention. Every quote
/// must pass native admission/inversion, including unselected securities.
pub fn government_spread(
    target: &c::CashBondHelper,
    target_resources: &c::CashHelperResources<'_>,
    securities: &[GovernmentSecurityInputs<'_>],
    reference_date: Date,
    settings: &GovernmentSpreadSettings,
) -> CashQuotationResult<GovernmentSpreadQuotation> {
    if !label(&settings.benchmark_id) {
        return Err(invalid(
            "benchmark",
            "bounded exact benchmark identity required",
        ));
    }
    if securities.is_empty() || securities.len() > 128 {
        return Err(invalid("benchmarks", "1..128 actual securities required"));
    }
    if settings.selection == GovernmentSelection::ExactMaturity && securities.len() != 1 {
        return Err(invalid(
            "benchmarks",
            "exact selection requires one supplied security",
        ));
    }
    let target_maturity = target.terms.schedule.end_date;
    let days = target.settlement.days_between(&target_maturity);
    if days <= 0 {
        return Err(invalid(
            "maturity",
            "future actual contractual target maturity required",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut maturities = std::collections::BTreeSet::new();
    for input in securities {
        let quote = input.quote;
        if quote.security.id.as_str() != quote.helper.id
            || quote.benchmark_id != settings.benchmark_id
            || quote.helper.credit_identity.is_some()
            || quote.helper.terms.currency != target.terms.currency
            || quote.helper.settlement != target.settlement
            || quote.helper.on_payment != target.on_payment
            || quote.helper.terms.schedule.end_date <= target.settlement
            || !ids.insert(quote.security.id.as_str())
            || !maturities.insert(quote.helper.terms.schedule.end_date)
        {
            return Err(invalid("benchmarks", "unique original securities/maturities, benchmark, currency, settlement and entitlement required"));
        }
    }
    let target = quote_cash_security(
        target,
        target_resources,
        reference_date,
        settings.convention,
        settings.inverse,
    )?;
    let benchmarks = securities
        .iter()
        .map(|input| {
            Ok(GovernmentBenchmarkQuotation {
                security: input.quote.security.clone(),
                quotation: quote_cash_security(
                    &input.quote.helper,
                    input.resources,
                    reference_date,
                    settings.convention,
                    settings.inverse,
                )?,
            })
        })
        .collect::<CashQuotationResult<Vec<_>>>()?;
    let exact = benchmarks
        .iter()
        .position(|b| b.quotation.source.terms.schedule.end_date == target_maturity);
    let (lower, upper, upper_weight) = if let Some(index) = exact {
        (index, None, 0.0)
    } else if settings.selection == GovernmentSelection::ExactMaturity {
        return Err(invalid(
            "maturity",
            "selected security must have the exact target maturity",
        ));
    } else {
        let mut ordered: Vec<_> = (0..benchmarks.len()).collect();
        ordered.sort_by_key(|i| benchmarks[*i].quotation.source.terms.schedule.end_date);
        let pair = ordered
            .windows(2)
            .find(|p| {
                benchmarks[p[0]].quotation.source.terms.schedule.end_date < target_maturity
                    && target_maturity < benchmarks[p[1]].quotation.source.terms.schedule.end_date
            })
            .ok_or_else(|| {
                invalid(
                    "maturity",
                    "target outside the closed supplied benchmark interval",
                )
            })?;
        let low = benchmarks[pair[0]].quotation.source.terms.schedule.end_date;
        let high = benchmarks[pair[1]].quotation.source.terms.schedule.end_date;
        (
            pair[0],
            Some(pair[1]),
            low.days_between(&target_maturity) as f64 / low.days_between(&high) as f64,
        )
    };
    let low_yield = benchmarks[lower].quotation.yield_inverse.decimal_parameter;
    let high_yield = upper.map_or(low_yield, |i| {
        benchmarks[i].quotation.yield_inverse.decimal_parameter
    });
    let benchmark_decimal_yield = finite(
        low_yield + upper_weight * (high_yield - low_yield),
        "government yield",
    )?;
    let decimal_spread = finite(
        target.yield_inverse.decimal_parameter - benchmark_decimal_yield,
        "G spread",
    )?;
    Ok(GovernmentSpreadQuotation {
        reference_date,
        settings: settings.clone(),
        target,
        benchmarks,
        selected: GovernmentSelectionResult {
            lower_index: lower,
            upper_index: upper,
            lower_weight: 1.0 - upper_weight,
            upper_weight,
            target_maturity,
            target_remaining_days: days,
        },
        benchmark_decimal_yield,
        decimal_spread,
        basis_points: finite(decimal_spread * 10_000.0, "G spread bp")?,
    })
}
