//! Full supplied cash quotations and actual-security government spreads.
//! Conversion, entitlement and inverses reuse the original native pricers.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{cash_calibration as c, quote_risk as q, workflow as w};
use convex_core::{contracts::ModelMoney, Date};
use convex_rates::ReferencePin;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

mod icma;
pub use icma::*;

/// Original conversion or inverse failure, without an accepted partial quote.
#[derive(Debug, thiserror::Error)]
pub enum CashQuotationError {
    /// Original full helper, calendar, quote and settlement admission.
    #[error(transparent)]
    Cash(#[from] c::CashCalibrationError),
    /// Original product, inverse, position or explicit selection failure.
    #[error(transparent)]
    Workflow(#[from] w::WorkflowError),
    /// Reverse spread's separately declared decimal-rate postcondition failed.
    #[error("G-spread roundtrip residual {decimal_residual} exceeds {decimal_tolerance}")]
    SpreadResidual {
        /// Actual solved spread minus requested spread, in annual decimals.
        decimal_residual: f64,
        /// Original caller-declared absolute annual-decimal bound.
        decimal_tolerance: f64,
        /// Actual target inverse iterations, without an automatic retry.
        iterations: u32,
        /// Actual complete target cash valuations.
        price_evaluations: usize,
    },
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
    let (_, calendars) = resources.bond()?;
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
    let scale = invoice_scale(&helper.terms, position)?;
    let quotation = quote_cash_security(helper, resources, reference_date, convention, settings)?;
    let consideration =
        invoice_consideration(&helper.terms, quotation.conversion.dirty_per100, scale)?;
    Ok(CashSecurityInvoice {
        quotation,
        position: position.clone(),
        consideration,
    })
}
pub(crate) fn invoice_scale(
    terms: &convex_bonds::contracts::BondTerms,
    position: &q::RiskPosition,
) -> CashQuotationResult<f64> {
    if position.currency != terms.currency
        || position.quantity_unit != q::RiskQuantityUnit::Notional
        || position.reference_quantity.decimal() != terms.face_value
    {
        return Err(invalid(
            "position",
            "exact currency, Notional units and original face reference required",
        ));
    }
    Ok(position.scale().map_err(w::WorkflowError::from)?)
}
pub(crate) fn invoice_consideration(
    terms: &convex_bonds::contracts::BondTerms,
    dirty_per100: f64,
    scale: f64,
) -> CashQuotationResult<ModelMoney> {
    let face = terms
        .face_value
        .to_f64()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| invalid("face", "finite positive original face required"))?;
    let value = finite(dirty_per100 * face / 100.0 * scale, "consideration")?;
    if scale != 0.0 && value == 0.0 {
        return Err(invalid("consideration", "nonzero position cash underflow"));
    }
    Ok(q::risk_reference_pv(value, terms.currency).map_err(w::WorkflowError::from)?)
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
    let state = GovernmentTargetState {
        currency: target.terms.currency,
        settlement: target.settlement,
        on_payment: target.on_payment,
        maturity: target.terms.schedule.end_date,
    };
    validate_government_securities(&state, securities, settings)?;
    let target = quote_cash_security(
        target,
        target_resources,
        reference_date,
        settings.convention,
        settings.inverse,
    )?;
    let benchmarks = quote_government_securities(securities, reference_date, settings)?;
    let (selected, benchmark_decimal_yield) = select_government_security(
        state.maturity,
        state.settlement,
        &benchmarks,
        settings.selection,
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
        selected,
        benchmark_decimal_yield,
        decimal_spread,
        basis_points: finite(decimal_spread * 10_000.0, "G spread bp")?,
    })
}

struct GovernmentTargetState {
    currency: convex_core::Currency,
    settlement: Date,
    on_payment: convex_core::cashflows::PaymentDateEntitlement,
    maturity: Date,
}
fn validate_government_securities(
    target: &GovernmentTargetState,
    securities: &[GovernmentSecurityInputs<'_>],
    settings: &GovernmentSpreadSettings,
) -> CashQuotationResult<()> {
    validate_government_quotes(
        target,
        securities.iter().map(|s| s.quote),
        &settings.benchmark_id,
        settings.selection,
    )
}
fn validate_government_quotes<'a>(
    target: &GovernmentTargetState,
    securities: impl ExactSizeIterator<Item = &'a GovernmentSecurityQuote>,
    benchmark_id: &str,
    selection: GovernmentSelection,
) -> CashQuotationResult<()> {
    if !label(benchmark_id) {
        return Err(invalid(
            "benchmark",
            "bounded exact benchmark identity required",
        ));
    }
    if securities.len() == 0 || securities.len() > 128 {
        return Err(invalid("benchmarks", "1..128 actual securities required"));
    }
    if selection == GovernmentSelection::ExactMaturity && securities.len() != 1 {
        return Err(invalid(
            "benchmarks",
            "exact selection requires one supplied security",
        ));
    }
    let target_maturity = target.maturity;
    let days = target.settlement.days_between(&target_maturity);
    if days <= 0 {
        return Err(invalid(
            "maturity",
            "future actual contractual target maturity required",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut maturities = std::collections::BTreeSet::new();
    for quote in securities {
        if quote.security.id.as_str() != quote.helper.id
            || quote.benchmark_id != benchmark_id
            || quote.helper.credit_identity.is_some()
            || quote.helper.terms.currency != target.currency
            || quote.helper.settlement != target.settlement
            || quote.helper.on_payment != target.on_payment
            || quote.helper.terms.schedule.end_date <= target.settlement
            || !ids.insert(quote.security.id.as_str())
            || !maturities.insert(quote.helper.terms.schedule.end_date)
        {
            return Err(invalid("benchmarks", "unique original securities/maturities, benchmark, currency, settlement and entitlement required"));
        }
    }
    Ok(())
}
fn quote_government_securities(
    securities: &[GovernmentSecurityInputs<'_>],
    reference_date: Date,
    settings: &GovernmentSpreadSettings,
) -> CashQuotationResult<Vec<GovernmentBenchmarkQuotation>> {
    securities
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
        .collect::<CashQuotationResult<Vec<_>>>()
}
fn select_government_security(
    target_maturity: Date,
    settlement: Date,
    benchmarks: &[GovernmentBenchmarkQuotation],
    selection: GovernmentSelection,
) -> CashQuotationResult<(GovernmentSelectionResult, f64)> {
    let yields: Vec<_> = benchmarks
        .iter()
        .map(|b| {
            (
                b.quotation.source.terms.schedule.end_date,
                b.quotation.yield_inverse.decimal_parameter,
            )
        })
        .collect();
    select_government_yields(target_maturity, settlement, &yields, selection)
}
fn select_government_yields(
    target_maturity: Date,
    settlement: Date,
    yields: &[(Date, f64)],
    selection: GovernmentSelection,
) -> CashQuotationResult<(GovernmentSelectionResult, f64)> {
    let exact = yields.iter().position(|b| b.0 == target_maturity);
    let (lower, upper, upper_weight) = if let Some(index) = exact {
        (index, None, 0.0)
    } else if selection == GovernmentSelection::ExactMaturity {
        return Err(invalid(
            "maturity",
            "selected security must have the exact target maturity",
        ));
    } else {
        let mut ordered: Vec<_> = (0..yields.len()).collect();
        ordered.sort_by_key(|i| yields[*i].0);
        let pair = ordered
            .windows(2)
            .find(|p| yields[p[0]].0 < target_maturity && target_maturity < yields[p[1]].0)
            .ok_or_else(|| {
                invalid(
                    "maturity",
                    "target outside the closed supplied benchmark interval",
                )
            })?;
        let low = yields[pair[0]].0;
        let high = yields[pair[1]].0;
        (
            pair[0],
            Some(pair[1]),
            low.days_between(&target_maturity) as f64 / low.days_between(&high) as f64,
        )
    };
    let low_yield = yields[lower].1;
    let high_yield = upper.map_or(low_yield, |i| yields[i].1);
    let benchmark_decimal_yield = finite(
        low_yield + upper_weight * (high_yield - low_yield),
        "government yield",
    )?;
    Ok((
        GovernmentSelectionResult {
            lower_index: lower,
            upper_index: upper,
            lower_weight: 1.0 - upper_weight,
            upper_weight,
            target_maturity,
            target_remaining_days: settlement.days_between(&target_maturity),
        },
        benchmark_decimal_yield,
    ))
}

/// Complete target economics without an invented observed target price.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GovernmentSpreadTarget {
    /// Actual target security identity and independent reference revision.
    pub security: ReferencePin,
    /// Full original fixed coupon/notional/principal/entitlement economics.
    #[serde(serialize_with = "c::serialize_cash_terms")]
    pub terms: convex_bonds::contracts::BondTerms,
    /// Actual trade date for the unchanged settlement-rule admission.
    pub trade_date: Date,
    /// Actual quote settlement, checked against trade/lag/calendar/adjustment.
    pub settlement: Date,
    /// Explicit ownership of unpaid settlement-date cash.
    pub on_payment: convex_core::cashflows::PaymentDateEntitlement,
}
/// Original spread observation, separate from security reference revisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GovernmentSpreadQuote {
    /// Actual immutable quote revision.
    pub quote_version: convex_core::contracts::Version,
    /// Fractional DecimalRate or BasisPoints; other native quote units fail.
    pub spread: convex_curves::contracts::MarketQuote,
}
/// Common benchmark quotation plus an explicit reverse-spread postcondition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GovernmentSpreadPriceSettings {
    /// Original unchanged convention, benchmark selection and inverse controls.
    pub government: GovernmentSpreadSettings,
    /// Absolute annual-decimal spread residual in (0,1e-6], distinct from
    /// per100 price acceptance and parameter stopping precision.
    pub decimal_spread_tolerance: f64,
}
/// Full same-pricer reverse quote, without posting or execution authority.
#[derive(Debug, Clone, Serialize)]
pub struct GovernmentSpreadPrice {
    /// Exact supplied information origin.
    pub reference_date: Date,
    /// Complete original target economics and reference revision.
    pub target: GovernmentSpreadTarget,
    /// Separate original spread value, native unit and quote revision.
    pub quote: GovernmentSpreadQuote,
    /// Original quotation and numerical policy actually used.
    pub settings: GovernmentSpreadPriceSettings,
    /// Every original benchmark and full native inverse in source order.
    pub benchmarks: Vec<GovernmentBenchmarkQuotation>,
    /// Actual contractual-maturity selection and calendar-day weights.
    pub selected: GovernmentSelectionResult,
    /// Common annual-decimal actual-security benchmark yield.
    pub benchmark_decimal_yield: f64,
    /// Benchmark yield plus requested fractional spread.
    pub target_decimal_yield: f64,
    /// Original spread converted once to annual decimals.
    pub decimal_spread: f64,
    /// Original spread times10,000 without basis-point rounding.
    pub basis_points: f64,
    /// Original complete target price/accrued/cash at the requested spread.
    pub valuation: w::BondValue,
    /// Actual signed original-face position; zero is an explicit flat position.
    pub position: q::RiskPosition,
    /// Signed unrounded consideration from the ORIGINAL computed dirty price.
    pub consideration: ModelMoney,
    /// Same complete target cash, original closed domain and inverse diagnostics.
    pub yield_inverse: w::BondInverse,
    /// Solved target yield minus the same selected benchmark yield.
    pub roundtrip_decimal_spread: f64,
    /// Actual roundtrip spread minus the original requested decimal spread.
    pub decimal_spread_residual: f64,
}
/// Price full fixed dated cash from actual-security G-spread. Every benchmark,
/// including unselected quotes, must pass the original conversion/inverse.
/// This composes dated-cash yields; no Street, zero-curve or option model is inferred.
pub fn price_government_spread(
    target: &GovernmentSpreadTarget,
    target_resources: &c::CashHelperResources<'_>,
    securities: &[GovernmentSecurityInputs<'_>],
    reference_date: Date,
    settings: &GovernmentSpreadPriceSettings,
    quote: &GovernmentSpreadQuote,
    position: &q::RiskPosition,
) -> CashQuotationResult<GovernmentSpreadPrice> {
    use convex_curves::contracts::QuoteUnit;
    if !settings.decimal_spread_tolerance.is_finite()
        || settings.decimal_spread_tolerance <= 0.0
        || settings.decimal_spread_tolerance > 1e-6
        || !matches!(
            quote.spread.unit,
            QuoteUnit::DecimalRate | QuoteUnit::BasisPoints
        )
    {
        return Err(invalid("G-spread settings", "explicit decimal residual bound in (0,1e-6] and DecimalRate/BasisPoints quote required"));
    }
    let decimal_spread = quote
        .spread
        .decimal_rate()
        .map_err(w::WorkflowError::from)?;
    if decimal_spread.abs() > 1.0 {
        return Err(invalid(
            "G spread",
            "fractional annual spread within +/-1 required",
        ));
    }
    let scale = invoice_scale(&target.terms, position)?;
    let state = GovernmentTargetState {
        currency: target.terms.currency,
        settlement: target.settlement,
        on_payment: target.on_payment,
        maturity: target.terms.schedule.end_date,
    };
    let common = &settings.government;
    validate_government_securities(&state, securities, common)?;
    let contract = c::prepare_cash_contract(
        &target.terms,
        target.trade_date,
        target.settlement,
        target_resources,
        reference_date,
    )?;
    let cash = w::PreparedBondCash::fixed(&contract, target.settlement, target.on_payment)?;
    let benchmarks = quote_government_securities(securities, reference_date, common)?;
    let (selected, benchmark_decimal_yield) = select_government_security(
        state.maturity,
        state.settlement,
        &benchmarks,
        common.selection,
    )?;
    let target_decimal_yield = finite(benchmark_decimal_yield + decimal_spread, "target yield")?;
    let valuation = cash.price_yield(target_decimal_yield, common.convention)?;
    if valuation.dirty_per100 <= 0.0 || valuation.clean_per100 < 0.0 {
        return Err(invalid(
            "cash quote",
            "positive dirty and nonnegative clean original price required",
        ));
    }
    let yield_inverse = w::solve_bond_yield(
        &cash,
        common.convention,
        w::BondPriceTarget::Dirty(valuation.dirty_per100),
        common.inverse,
    )?;
    let roundtrip_decimal_spread = finite(
        yield_inverse.decimal_parameter - benchmark_decimal_yield,
        "roundtrip G spread",
    )?;
    let decimal_spread_residual = finite(
        roundtrip_decimal_spread - decimal_spread,
        "G spread residual",
    )?;
    if decimal_spread_residual.abs() > settings.decimal_spread_tolerance {
        return Err(CashQuotationError::SpreadResidual {
            decimal_residual: decimal_spread_residual,
            decimal_tolerance: settings.decimal_spread_tolerance,
            iterations: yield_inverse.iterations,
            price_evaluations: yield_inverse.price_evaluations,
        });
    }
    let consideration = invoice_consideration(&target.terms, valuation.dirty_per100, scale)?;
    Ok(GovernmentSpreadPrice {
        reference_date,
        target: target.clone(),
        quote: quote.clone(),
        settings: settings.clone(),
        benchmarks,
        selected,
        benchmark_decimal_yield,
        target_decimal_yield,
        decimal_spread,
        basis_points: finite(decimal_spread * 10_000.0, "G spread bp")?,
        valuation,
        position: position.clone(),
        consideration,
        yield_inverse,
        roundtrip_decimal_spread,
        decimal_spread_residual,
    })
}
