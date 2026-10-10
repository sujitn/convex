//! Strict original economic inputs and actual borrowed resources.

use super::*;
use crate::{bond_contract::ContractPrice, credit_risk::BondCreditIdentity};
use convex_bonds::contracts::{BondCalendarIds, BondCalendars, BondTerms};
use convex_core::{
    cashflows::PaymentDateEntitlement, contracts::Version, daycounts::DayCountConvention,
};
use convex_math::{optimization::FitDiagnostics, solvers::SolverResult};
use convex_rates::{CalendarBinding, ReferencePin};
use serde::{Deserialize, Serialize};

/// Explicit dated-cash yield; no market-specific street convention is inferred.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashYieldConvention {
    /// Basis from settlement to each actual entitled cash date. ICMA requires a
    /// separate market pack and is rejected by this date-based quotation route.
    pub day_count: DayCountConvention,
    /// Compounding applied to those exact dated-cash fractions.
    pub compounding: convex_core::types::Compounding,
    /// Closed decimal yield bracket for the model-price inverse.
    pub lower_yield: f64,
    /// Closed upper decimal yield bound.
    pub upper_yield: f64,
    /// Bounded inverse root budget,1..256.
    pub max_iterations: u32,
    /// Absolute dirty-price-per100 residual bound for the inverse.
    pub price_tolerance: f64,
}

/// Native quote meaning; price versus yield versus coupon is never implicit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum CashQuoteKind {
    /// Clean currency price per100 ORIGINAL face.
    CleanPrice,
    /// Dirty currency price per100 ORIGINAL face.
    DirtyPrice,
    /// Yield discounting each actual entitled coupon/principal cash.
    CashflowYield {
        /// Complete explicitly supplied dated quotation.
        convention: CashYieldConvention,
    },
    /// Contextual yield over the full original nominal mesh, with no inferred
    /// day count, calendar revision, source policy or inverse controls.
    ContextualIcmaYield {
        /// Complete independently supplied sources, convention and work limits.
        input: Box<IcmaCashYield>,
    },
    /// Simple bank-discount quote on one par principal payment, using actual dates.
    BankDiscountYield {
        /// Bill quotation basis, distinct from curve time basis.
        day_count: DayCountConvention,
    },
    /// Uniform coupon rate of the supplied full par bond, whose CLEAN price is100.
    /// This is an actual cash helper; it is never inserted as a zero-rate node.
    ParCouponRate,
}

/// Original contextual ICMA cash-helper observation and model-inverse policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaCashYield {
    /// Exact security, original quote, policy and independent calendar revisions.
    pub sources: crate::workflow::IcmaQuotationSources,
    /// Full anchored nominal periods belonging to this security revision.
    pub reference_context: crate::workflow::IcmaReferenceContext,
    /// Explicit original frequency, compounding and preparation work ceiling.
    pub convention: crate::workflow::IcmaYieldConvention,
    /// Original closed model-price inverse and separate native price tolerance.
    pub inverse: crate::workflow::BondInverseSettings,
    /// Original-face quantity for quotation diagnostics; never used to fit a curve.
    pub position: crate::quote_risk::RiskPosition,
    /// Positive total cash visits reserved across observation and every model
    /// price/inverse in one build, at most MAX_ICMA_HELPER_CASH_EVALUATIONS.
    /// Each inverse reserves its unchanged iterations+8 complete cash passes.
    pub max_cash_evaluations: u64,
}

/// Complete cash security and its original immutable quotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashBondHelper {
    /// Stable helper identity, unique within its curve.
    pub id: String,
    /// Actual supplied quote revision; ingestion assigns bumped versions.
    pub quote_version: Version,
    /// Last relevant fitted cash date; must cover every actual consumed payment.
    pub pillar: Date,
    /// Complete original stepped coupon/principal/redemption/ex-date economics.
    #[serde(serialize_with = "serialize_cash_terms")]
    pub terms: BondTerms,
    /// Actual trade date used to check the stated settlement rule.
    pub trade_date: Date,
    /// Actual purchase/quote settlement, independently checked against terms.
    pub settlement: Date,
    /// Explicit pre/post settlement-payment state.
    pub on_payment: PaymentDateEntitlement,
    /// Actual legal mapping, mandatory for issuer fits and absent for government.
    pub credit_identity: Option<BondCreditIdentity>,
    /// Meaning of the native input.
    pub quote_kind: CashQuoteKind,
    /// Original price/rate unit and value.
    pub quote: MarketQuote,
    /// Absolute ORIGINAL native quote tolerance.
    pub tolerance: f64,
}

// The workspace enables Decimal serde-float for existing host DTOs. The new
// full cash helper preserves its supplied contractual Decimal fields as strings
// instead of allowing an intermediate JSON Value to round away economic steps.
// This adapter is private to the checked fixed-cash route; no host schema changes.
pub(crate) fn serialize_cash_terms<S: serde::Serializer>(
    terms: &BondTerms,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    super::pricing::term_bounds(terms).map_err(serde::ser::Error::custom)?;
    if !matches!(terms.coupon, convex_bonds::contracts::BondCoupon::Fixed) {
        return Err(serde::ser::Error::custom(
            "cash helper requires fixed supplied coupon economics",
        ));
    }
    let mut value = serde_json::to_value(terms).map_err(serde::ser::Error::custom)?;
    value["face_value"] = serde_json::Value::String(terms.face_value.to_string());
    value["redemption_multiplier"] =
        serde_json::Value::String(terms.redemption_multiplier.to_string());
    for (name, profile) in [
        ("coupon", &terms.profiles.coupon),
        ("spread", &terms.profiles.spread),
        ("gearing", &terms.profiles.gearing),
        ("notional", &terms.profiles.notional),
    ] {
        for (i, entry) in profile.iter().enumerate() {
            value["profiles"][name][i]["value"] =
                serde_json::Value::String(entry.value.to_string());
        }
    }
    for (i, price) in terms.principal_prices.iter().enumerate() {
        value["principal_prices"][i]["multiplier"] =
            serde_json::Value::String(price.multiplier.to_string());
        value["principal_prices"][i]["amount"] = price
            .amount
            .map(|v| serde_json::Value::String(v.to_string()))
            .unwrap_or(serde_json::Value::Null);
    }
    value.serialize(serializer)
}
impl CashBondHelper {
    /// Replace the original observation while retaining all economics. A par
    /// coupon quotation also replaces its uniform original coupon profile.
    /// The caller must assign a new quote/output version and dependent pins.
    pub fn with_quote(&self, quote: MarketQuote) -> CashCalibrationResult<Self> {
        quote.validate()?;
        super::pricing::term_bounds(&self.terms)?;
        let mut next = self.clone();
        if self.quote_kind == CashQuoteKind::ParCouponRate {
            let coupon = rust_decimal::Decimal::try_from(quote.decimal_rate()?)
                .map_err(|_| invalid("coupon", "unrepresentable rate"))?;
            for entry in &mut next.terms.profiles.coupon {
                entry.value = coupon;
            }
        }
        next.quote = quote;
        Ok(next)
    }
}

/// Actual repo loan: start principal and end principal plus simple interest.
/// Haircuts/reinvestment are separately supplied carry economics, never quotes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RepoLoanHelper {
    /// Original helper identity.
    pub id: String,
    /// Immutable actual repo quote version.
    pub quote_version: Version,
    /// Exact collateral definition; must match Repo role.
    pub collateral_id: String,
    /// Actual independently pinned business calendar.
    pub calendar: ReferencePin,
    /// Actual value date, on or after curve reference date.
    pub start_date: Date,
    /// Actual maturity/cash date, strictly after start.
    pub end_date: Date,
    /// Original loan interest basis.
    pub day_count: DayCountConvention,
    /// Original annualized native rate.
    pub quote: MarketQuote,
    /// Absolute native rate tolerance.
    pub tolerance: f64,
}

/// Closed helper families with no ignored inapplicable payoff fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum CashCalibrationHelper {
    /// Full actual bond or bill cash and quote.
    Bond {
        /// Complete original security.
        input: Box<CashBondHelper>,
    },
    /// Full actual dated repo loan.
    Repo {
        /// Complete original loan.
        input: RepoLoanHelper,
    },
}
impl CashCalibrationHelper {
    /// Original source identity.
    pub fn id(&self) -> &str {
        match self {
            Self::Bond { input } => &input.id,
            Self::Repo { input } => &input.id,
        }
    }
    /// Last relevant input cash date.
    pub fn pillar(&self) -> Date {
        match self {
            Self::Bond { input } => input.pillar,
            Self::Repo { input } => input.end_date,
        }
    }
    /// Original native quote.
    pub fn quote(&self) -> MarketQuote {
        match self {
            Self::Bond { input } => input.quote,
            Self::Repo { input } => input.quote,
        }
    }
    /// Original native-unit tolerance.
    pub fn tolerance(&self) -> f64 {
        match self {
            Self::Bond { input } => input.tolerance,
            Self::Repo { input } => input.tolerance,
        }
    }
    pub(super) fn solver_tolerance(&self) -> CashCalibrationResult<f64> {
        if let Self::Bond { input: helper } = self {
            if let CashQuoteKind::ContextualIcmaYield { input } = &helper.quote_kind {
                // The outer solve must not discard the explicitly requested
                // inner annual-yield precision. Convert it through the original
                // quote-unit owner; the final ORIGINAL tolerance stays unchanged.
                return Ok(self.tolerance().min(
                    helper
                        .quote
                        .model_rate_quote(input.inverse.parameter_tolerance)?
                        .value,
                ));
            }
        }
        Ok(self.tolerance())
    }
}

/// Exact semantic reference to a named rate curve; roles cannot substitute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum BondReferenceRole {
    /// Same-currency collateral discounting.
    Discount,
    /// Actual named government benchmark.
    Government {
        /// Exact internal benchmark identity.
        benchmark_id: String,
    },
}

/// Explicit residual pricing model, distinct from CDS hazard or a raw Z quote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum BondBasisModel {
    /// Discount actual promised cash by reference DF ratios and the dated
    /// continuous-zero residual basis factor. No credit claim is implied.
    ReferenceZeroSpread {
        /// Exact independently supplied reference version.
        reference: CurveKey,
        /// Exact consumed reference role.
        role: BondReferenceRole,
    },
    /// Conditional surviving promised cash plus immediate recovery of current
    /// par principal at default. Residual basis affects PROMISED cash only;
    /// recovery uses the independently supplied discount/survival integration.
    /// No recovered coupon/accrual is assumed. Actual CDS legal mapping is required.
    CreditParRecovery {
        /// Exact collateral discount version.
        discount: CurveKey,
        /// Exact full CDS-calibrated/supplied survival version.
        survival: CurveKey,
        /// Actual mapped seniority.
        seniority: String,
        /// Explicit recovery fraction, identical to credit role.
        recovery: f64,
    },
}

/// Closed root bracket for annualized continuous zeros or residual basis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashBootstrapSettings {
    /// Closed lower annualized parameter bound within[-10,10].
    pub lower_parameter: f64,
    /// Closed upper annualized parameter bound.
    pub upper_parameter: f64,
    /// Bounded Brent budget per parameter.
    pub max_iterations: u32,
}
/// Duplicate pillar policy; all retained global observations must reprice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CashOverlapPolicy {
    /// Reject overlapping observations explicitly.
    Reject,
    /// Retain every original helper in an identified global objective.
    RetainAllGlobal,
}
/// Full government, issuer-basis or repo recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashCurveRecipe {
    /// Complete exact role/currency/reference/method/dependencies/source contract.
    /// Global initial_zeros/lower_zero/upper_zero hold annualized basis parameters
    /// for IssuerSpreads, and continuous zeros for Government/Repo.
    pub curve: CurveRecipe,
    /// Grid dates, or Parametric coverage dates whose last is the exact original
    /// native model horizon. Dates never stand in for model parameters.
    pub pillars: Vec<Date>,
    /// Every original full helper in source order.
    pub helpers: Vec<CashCalibrationHelper>,
    /// Mandatory only for piecewise fitting.
    pub bootstrap: Option<CashBootstrapSettings>,
    /// Mandatory only for IssuerSpread curves.
    pub basis_model: Option<BondBasisModel>,
    /// Explicit overlap behavior.
    pub overlap: CashOverlapPolicy,
}
/// Actual resources matched one-for-one to source helpers.
pub enum CashHelperResources<'a> {
    /// Supplied calendars for the full security; identities must match terms.
    Bond {
        /// Exact original calendar resource IDs/versions.
        ids: &'a BondCalendarIds,
        /// Actual immutable resources.
        calendars: BondCalendars<'a>,
    },
    /// Actual independent revisions for contextual ICMA helpers. Context-free
    /// resources cannot attest or substitute these original source versions.
    IcmaBond {
        /// Exact original contractual calendar identities.
        ids: &'a BondCalendarIds,
        /// Actual immutable calendar data.
        calendars: BondCalendars<'a>,
        /// Actual resolved revisions, independent of the quotation payload.
        pins: crate::workflow::IcmaCalendarPins,
    },
    /// Actual exact loan calendar resource.
    Repo(CalendarBinding<'a>),
}
impl<'a> CashHelperResources<'a> {
    pub(crate) fn bond(&self) -> CashCalibrationResult<(&'a BondCalendarIds, &BondCalendars<'a>)> {
        match self {
            Self::Bond { ids, calendars } | Self::IcmaBond { ids, calendars, .. } => {
                Ok((ids, calendars))
            }
            Self::Repo(_) => Err(invalid("resources", "actual full bond calendars required")),
        }
    }
}
/// Borrowed complete pure build inputs.
pub struct CashCurveBuildInput<'a> {
    /// Original recipe.
    pub recipe: &'a CashCurveRecipe,
    /// Actual resources in its original helper order.
    pub resources: &'a [CashHelperResources<'a>],
}
/// Original settlement/accrued conversion; no price/rate proxy.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CashQuoteConversion {
    /// Actual settlement date.
    pub settlement: Date,
    /// Full original native quote meaning.
    pub quote_kind: CashQuoteKind,
    /// Original quote unit/value.
    pub quote: MarketQuote,
    /// Actual accrued/rebate per100 original face.
    pub accrued_per100: f64,
    /// Consistent observed clean price.
    pub clean_per100: f64,
    /// Consistent observed dirty price.
    pub dirty_per100: f64,
    /// Complete same-owner observed ICMA cash/time/price and original signed
    /// consideration. Absent for existing context-free observations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icma: Option<Box<crate::workflow::IcmaQuotation>>,
    /// Declared and reserved contextual work, separate from solver measurements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icma_work: Option<IcmaHelperWork>,
}
/// Contextual preparation and complete-cash work retained for each helper.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaHelperWork {
    /// Same checked ICMA owner's one-time reference-entry reservation.
    pub reserved_reference_evaluations: u64,
    /// Cash visits reserved so far, including prior fit trials and this result.
    pub reserved_cash_evaluations: u64,
    /// Original explicit per-helper cash ceiling for the entire build.
    pub max_cash_evaluations: u64,
}
/// Full declared issuer model valuation, separately retaining recovery cash PV.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BasisBondValuation {
    /// Full actual projected security cash, accrued and modeled clean/dirty price.
    pub price: ContractPrice,
    /// Exact residual-basis curve version.
    pub basis: CurveKey,
    /// Actual separately declared reference/credit/recovery assumptions.
    pub model: BondBasisModel,
    /// Absolute currency promised-cash settlement PV.
    pub promised_pv: f64,
    /// Absolute currency immediate par-recovery settlement PV; zero for the
    /// explicitly deterministic ReferenceZeroSpread model only.
    pub recovery_pv: f64,
}
/// Actual original helper model value.
#[derive(Debug, Clone, serde::Serialize)]
pub enum CashHelperValuation {
    /// Full cash quote conversion and original model cash/price.
    Bond {
        /// Checked original observation.
        observed: CashQuoteConversion,
        /// Actual full priced security.
        value: Box<ContractPrice>,
        /// Actual separate credit/basis diagnostics.
        basis: Option<Box<BasisBondValuation>>,
        /// Complete model-implied ICMA inverse in the original helper units,
        /// separate from the original observed quotation and consideration.
        #[serde(skip_serializing_if = "Option::is_none")]
        icma: Option<Box<crate::workflow::IcmaQuotation>>,
        /// Aggregate reservation across all original fit trials for this helper.
        #[serde(skip_serializing_if = "Option::is_none")]
        icma_work: Option<IcmaHelperWork>,
    },
    /// Actual dated loan pricing; currency principal is normalized to one.
    Repo {
        /// Original collateral role.
        collateral_id: String,
        /// Actual value date.
        start: Date,
        /// Actual cash date.
        end: Date,
        /// Actual loan year fraction.
        accrual_fraction: f64,
        /// P(end)/P(start), not a bond discount replacement.
        repo_discount_ratio: f64,
        /// Model annualized simple repo rate.
        decimal_rate: f64,
    },
}
/// Actual measured bounded solver diagnostics.
#[derive(Debug, Clone)]
pub enum CashSolverDiagnostics {
    /// Causal roots in increasing pillar order.
    Piecewise {
        /// Actual original helper identities.
        helper_ids: Vec<String>,
        /// Actual roots, iterations, residuals.
        roots: Vec<SolverResult>,
    },
    /// Identified common scaled least-squares fit.
    Global(FitDiagnostics),
    /// Exact original continuous-zero model, distinct from annualized grid nodes.
    Parametric {
        /// Solved model loadings and decay years, in their original family.
        parameters: ParametricModel,
        /// Complete measured work/rank/conditioning/original-unit residuals.
        diagnostics: FitDiagnostics,
    },
    /// Actual full-product exponential-tail fit over its exact fixed base.
    Ufr {
        /// Solved continuous UFR and positive speed, in original curve-time units.
        parameters: UfrParameters,
        /// Complete measured work/rank/conditioning/original-unit residuals.
        diagnostics: FitDiagnostics,
    },
}
/// Complete accepted immutable fit.
#[derive(Debug, Clone)]
pub struct CalibratedCashCurve {
    /// Original full input recipe.
    pub recipe: CashCurveRecipe,
    /// Full admitted curve with every original native repricing record.
    pub curve: BuiltCurve,
    /// Solved grid zeros/basis in dated order. Empty for Parametric/Ufr; exact mixed
    /// model units remain in the corresponding solver diagnostics.
    pub annualized_parameters: Vec<f64>,
    /// Original source-order actual helper valuations.
    pub repriced_helpers: Vec<CashHelperValuation>,
    /// Actual solver outcome, not an estimated work count.
    pub solver: CashSolverDiagnostics,
    /// Actual helper price evaluations including final repricing.
    pub price_evaluations: usize,
}
/// Atomic graph build result; no publication or external-state mutation.
#[derive(Debug, Clone)]
pub struct CashCurveBuildOutput {
    /// All supplied exact dependencies and accepted new curves.
    pub curves: CurveSet,
    /// Deterministic exact dependency order.
    pub build_order: Vec<CurveKey>,
    /// Full original fits and actual native residuals.
    pub calibrations: Vec<CalibratedCashCurve>,
    /// Actual helper valuations over the whole atomic batch.
    pub price_evaluations: usize,
}
