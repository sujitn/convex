//! Actual-security government quotations using the original checked ICMA owner.
use super::*;
use convex_core::types::Compounding;

/// Complete independently supplied contextual policy, reference and quantity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaSecurityContext {
    /// Actual security's full original nominal mesh and independent revision.
    pub reference_context: w::IcmaReferenceContext,
    /// Its original normal frequency, compounding and reference-work controls.
    pub convention: w::IcmaYieldConvention,
    /// Independently resolved policy revision, checked against the convention.
    pub quotation_policy: ReferencePin,
    /// Actual independently resolved payment/reset/observation/exercise versions.
    pub calendars: w::IcmaCalendarPins,
    /// Exact original-face signed notional and currency; no quantity is inferred.
    pub position: q::RiskPosition,
}
/// Actual target observed quotation, without fabricated input yield or price.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaCashSecurityQuote {
    /// Original target reference, whose identity must equal the helper identity.
    pub security: ReferencePin,
    /// Full original terms, settlement, price meaning, units and quote revision.
    pub helper: c::CashBondHelper,
    /// Independently supplied exact contextual inputs for this security.
    pub icma: IcmaSecurityContext,
}
/// Complete original government classification and its own contextual inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaGovernmentSecurityQuote {
    /// Actual unchanged government-security economics, classification and quote.
    pub quote: GovernmentSecurityQuote,
    /// This source's exact context; another security's nominal mesh cannot substitute.
    pub icma: IcmaSecurityContext,
}
/// Actual calendars bound independently for every government security.
pub struct IcmaGovernmentSecurityInputs<'a> {
    /// Original immutable source, including its full independently versioned mesh.
    pub quote: &'a IcmaGovernmentSecurityQuote,
    /// Actual resolved calendars matching this source's identities and versions.
    pub resources: &'a c::CashHelperResources<'a>,
}
/// Common output representation, without equating normal coupon frequencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaGovernmentSpreadSettings {
    /// Exact benchmark classification required on every supplied security.
    pub benchmark_id: String,
    /// Actual common annual-decimal quotation policy required on every source.
    pub quotation_policy: ReferencePin,
    /// Common compounding applied to each source's independently computed ICMA time.
    pub compounding: Compounding,
    /// Original closed-domain same-pricer inverse controls for every security.
    pub inverse: w::BondInverseSettings,
    /// Original exact/closed-linear actual contractual maturity selection.
    pub selection: GovernmentSelection,
    /// Positive total reference-entry preparation ceiling, at most16777216.
    /// Each original source's independent reference-work ceiling also applies.
    pub max_reference_evaluations: u64,
}
/// Full observed conversion and original same-pricer ICMA cash/invoice inverse.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaCashSecurityQuotation {
    /// Original terms, observed quote and independently supplied contextual sources.
    pub source: IcmaCashSecurityQuote,
    /// Original observed dirty/clean/accrued conversion; never solved repricing.
    pub conversion: c::CashQuoteConversion,
    /// Complete contextual same-pricer inverse and ORIGINAL observed consideration.
    pub quotation: w::IcmaQuotation,
}
/// Every actual government security retained in the original input order.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaGovernmentBenchmarkQuotation {
    /// Full original classification, reference/quote/policy/calendar/context pins.
    pub source: IcmaGovernmentSecurityQuote,
    /// Original conversion and complete contextual inverse, cash and signed invoice.
    pub quotation: IcmaCashSecurityQuotation,
}
/// Full actual-security contextual G-spread quotation; no curve proxy.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaGovernmentSpreadQuotation {
    /// Actual supplied information origin.
    pub reference_date: Date,
    /// Original common quotation, selection and bounded numerical settings.
    pub settings: IcmaGovernmentSpreadSettings,
    /// Original observed target conversion, contextual inverse and signed invoice.
    pub target: IcmaCashSecurityQuotation,
    /// All actual benchmark sources, including unselected securities.
    pub benchmarks: Vec<IcmaGovernmentBenchmarkQuotation>,
    /// Original source indices, actual contractual dates and calendar-day weights.
    pub selected: GovernmentSelectionResult,
    /// Common annual-decimal actual-security benchmark yield.
    pub benchmark_decimal_yield: f64,
    /// Target minus benchmark yield, in annual decimals.
    pub decimal_spread: f64,
    /// Same spread times10000; no basis-point rounding.
    pub basis_points: f64,
    /// Sum of original one-time preparation reservations across every source.
    pub reserved_reference_evaluations: u64,
}
/// Complete target reference/economics without an invented observed target price.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaGovernmentSpreadTarget {
    /// Full original target terms, reference, trade, settlement and entitlement.
    pub target: GovernmentSpreadTarget,
    /// Target's independent nominal mesh, policy, calendars and signed original face.
    pub icma: IcmaSecurityContext,
}
/// Original common quotation settings plus a separate decimal spread postcondition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct IcmaGovernmentSpreadPriceSettings {
    /// Complete actual-security quotation, selection and original inverse controls.
    pub government: IcmaGovernmentSpreadSettings,
    /// Explicit annual-decimal spread residual in (0,1e-6], independent of price.
    pub decimal_spread_tolerance: f64,
}
/// Full contextual G-spread-to-price and original same-pricer roundtrip.
#[derive(Debug, Clone, Serialize)]
pub struct IcmaGovernmentSpreadPrice {
    /// Actual supplied information origin.
    pub reference_date: Date,
    /// Complete original target reference/economics/context/quantity.
    pub target: IcmaGovernmentSpreadTarget,
    /// Separate actual spread quote, unit and revision.
    pub quote: GovernmentSpreadQuote,
    /// Original common convention, numerical and spread residual settings.
    pub settings: IcmaGovernmentSpreadPriceSettings,
    /// Every original actual benchmark and its complete contextual cash inverse.
    pub benchmarks: Vec<IcmaGovernmentBenchmarkQuotation>,
    /// Original actual-contractual-maturity selection and source order indices.
    pub selected: GovernmentSelectionResult,
    /// Common annual-decimal selected benchmark yield.
    pub benchmark_decimal_yield: f64,
    /// Benchmark yield plus original requested spread.
    pub target_decimal_yield: f64,
    /// Original quote converted once to annual decimals.
    pub decimal_spread: f64,
    /// Original spread times10000, retaining fractional basis points.
    pub basis_points: f64,
    /// Complete original target cash price and computed dirty-price consideration.
    pub valuation: w::IcmaQuotation,
    /// Same prepared target cash inverse, original work and native price residual.
    pub yield_inverse: w::IcmaQuotation,
    /// Solved target yield minus the same selected benchmark yield.
    pub roundtrip_decimal_spread: f64,
    /// Actual roundtrip spread minus original requested annual-decimal spread.
    pub decimal_spread_residual: f64,
    /// Sum of original one-time preparation reservations across every source.
    pub reserved_reference_evaluations: u64,
}

// Prepared state is private. All original source/context admission and cash times
// complete before ANY government quotation inverse or accepted subset.
struct PreparedSecurity {
    source: IcmaCashSecurityQuote,
    conversion: c::CashQuoteConversion,
    cash: w::PreparedIcmaQuotation,
}
fn check_common(
    context: &IcmaSecurityContext,
    settings: &IcmaGovernmentSpreadSettings,
) -> CashQuotationResult<()> {
    if context.convention.compounding != settings.compounding
        || context.convention.quotation_policy != settings.quotation_policy
        || context.quotation_policy != settings.quotation_policy
    {
        return Err(invalid(
            "ICMA government convention",
            "actual common annual-decimal policy and compounding required on every original source",
        ));
    }
    Ok(())
}
fn prepare_context(
    contract: &convex_bonds::contracts::BondContract,
    security: &ReferencePin,
    quote_revision: ReferencePin,
    context: &IcmaSecurityContext,
    settlement: Date,
    on_payment: convex_core::cashflows::PaymentDateEntitlement,
    resources: &c::CashHelperResources<'_>,
) -> CashQuotationResult<w::PreparedIcmaQuotation> {
    let (_, calendars) = resources.bond()?;
    let sources = w::IcmaQuotationSources {
        security: security.clone(),
        quote_revision,
        quotation_policy: context.quotation_policy.clone(),
        calendars: context.calendars.clone(),
    };
    Ok(w::PreparedIcmaQuotation::new(
        &w::IcmaQuotationInputs {
            contract,
            sources: &sources,
            reference_context: &context.reference_context,
            convention: &context.convention,
            position: &context.position,
            exercise_calendar: (&contract.terms().calendars.exercise, calendars.exercise),
            purchase_calendar: (&contract.terms().calendars.payment, calendars.payment),
        },
        &w::IcmaCashSelection::Plain {
            settlement,
            on_payment,
        },
    )?)
}
fn prepare_security(
    source: &IcmaCashSecurityQuote,
    resources: &c::CashHelperResources<'_>,
    reference: Date,
    settings: &IcmaGovernmentSpreadSettings,
) -> CashQuotationResult<PreparedSecurity> {
    check_common(&source.icma, settings)?;
    if source.security.id.as_str() != source.helper.id {
        return Err(invalid(
            "ICMA security",
            "exact original reference and observed helper identity required",
        ));
    }
    let h = &source.helper;
    let conversion = c::convert_cash_quote(h, resources, reference)?;
    let contract =
        c::prepare_cash_contract(&h.terms, h.trade_date, h.settlement, resources, reference)?;
    let cash = prepare_context(
        &contract,
        &source.security,
        ReferencePin {
            id: source.security.id.clone(),
            version: h.quote_version,
        },
        &source.icma,
        h.settlement,
        h.on_payment,
        resources,
    )?;
    Ok(PreparedSecurity {
        source: source.clone(),
        conversion,
        cash,
    })
}
fn reserve(
    used: &mut u64,
    cash: &w::PreparedIcmaQuotation,
    settings: &IcmaGovernmentSpreadSettings,
) -> CashQuotationResult<()> {
    *used = used
        .checked_add(cash.reserved_reference_evaluations())
        .ok_or_else(|| invalid("ICMA work", "reference-entry reservation overflow"))?;
    if settings.max_reference_evaluations == 0
        || settings.max_reference_evaluations > w::MAX_ICMA_REFERENCE_EVALUATIONS
        || *used > settings.max_reference_evaluations
    {
        return Err(
            w::WorkflowError::from(convex_core::cashflows::CashflowError::BoundExceeded(
                "ICMA government reference evaluations",
            ))
            .into(),
        );
    }
    Ok(())
}
fn prepare_benchmarks(
    target: &GovernmentTargetState,
    inputs: &[IcmaGovernmentSecurityInputs<'_>],
    reference: Date,
    settings: &IcmaGovernmentSpreadSettings,
    used: &mut u64,
) -> CashQuotationResult<Vec<PreparedSecurity>> {
    validate_government_quotes(
        target,
        inputs.iter().map(|s| &s.quote.quote),
        &settings.benchmark_id,
        settings.selection,
    )?;
    inputs
        .iter()
        .map(|s| {
            let source = IcmaCashSecurityQuote {
                security: s.quote.quote.security.clone(),
                helper: s.quote.quote.helper.clone(),
                icma: s.quote.icma.clone(),
            };
            let prepared = prepare_security(&source, s.resources, reference, settings)?;
            reserve(used, &prepared.cash, settings)?;
            Ok(prepared)
        })
        .collect()
}
fn inverse(
    prepared: &PreparedSecurity,
    settings: &IcmaGovernmentSpreadSettings,
) -> CashQuotationResult<IcmaCashSecurityQuotation> {
    Ok(IcmaCashSecurityQuotation {
        source: prepared.source.clone(),
        conversion: prepared.conversion.clone(),
        quotation: prepared.cash.quote_yield(
            w::BondPriceTarget::Dirty(prepared.conversion.dirty_per100),
            settings.inverse,
        )?,
    })
}
fn quote_benchmarks(
    prepared: &[PreparedSecurity],
    inputs: &[IcmaGovernmentSecurityInputs<'_>],
    settings: &IcmaGovernmentSpreadSettings,
) -> CashQuotationResult<Vec<IcmaGovernmentBenchmarkQuotation>> {
    prepared
        .iter()
        .zip(inputs)
        .map(|(p, s)| {
            Ok(IcmaGovernmentBenchmarkQuotation {
                source: s.quote.clone(),
                quotation: inverse(p, settings)?,
            })
        })
        .collect()
}
fn yield_inverse(quote: &w::IcmaQuotation) -> CashQuotationResult<&w::BondInverse> {
    match &quote.quotation {
        w::WorkoutValue::Yield(inverse) => Ok(inverse),
        _ => Err(invalid(
            "ICMA quotation",
            "original native same-pricer inverse required",
        )),
    }
}
fn select(
    target: &GovernmentTargetState,
    benchmarks: &[IcmaGovernmentBenchmarkQuotation],
    selection: GovernmentSelection,
) -> CashQuotationResult<(GovernmentSelectionResult, f64)> {
    let yields = benchmarks
        .iter()
        .map(|b| {
            Ok((
                b.source.quote.helper.terms.schedule.end_date,
                yield_inverse(&b.quotation.quotation)?.decimal_parameter,
            ))
        })
        .collect::<CashQuotationResult<Vec<_>>>()?;
    select_government_yields(target.maturity, target.settlement, &yields, selection)
}
/// Quote original observed target and EVERY actual benchmark under checked ICMA
/// context and a supplied common annual-decimal policy; no sampled curve fallback.
pub fn icma_government_spread(
    target: &IcmaCashSecurityQuote,
    target_resources: &c::CashHelperResources<'_>,
    securities: &[IcmaGovernmentSecurityInputs<'_>],
    reference_date: Date,
    settings: &IcmaGovernmentSpreadSettings,
) -> CashQuotationResult<IcmaGovernmentSpreadQuotation> {
    let state = GovernmentTargetState {
        currency: target.helper.terms.currency,
        settlement: target.helper.settlement,
        on_payment: target.helper.on_payment,
        maturity: target.helper.terms.schedule.end_date,
    };
    let target = prepare_security(target, target_resources, reference_date, settings)?;
    let mut used = 0;
    reserve(&mut used, &target.cash, settings)?;
    let prepared = prepare_benchmarks(&state, securities, reference_date, settings, &mut used)?;
    let target = inverse(&target, settings)?;
    let benchmarks = quote_benchmarks(&prepared, securities, settings)?;
    let (selected, benchmark_decimal_yield) = select(&state, &benchmarks, settings.selection)?;
    let decimal_spread = finite(
        yield_inverse(&target.quotation)?.decimal_parameter - benchmark_decimal_yield,
        "G spread",
    )?;
    Ok(IcmaGovernmentSpreadQuotation {
        reference_date,
        settings: settings.clone(),
        target,
        benchmarks,
        selected,
        benchmark_decimal_yield,
        decimal_spread,
        basis_points: finite(decimal_spread * 10000.0, "G spread bp")?,
        reserved_reference_evaluations: used,
    })
}
/// Price original full target cash from actual-security ICMA G-spread, then enforce
/// original native price AND separate annual-decimal spread residuals. Every source
/// is prepared before inverses; failures expose no accepted subset or invoice.
pub fn price_icma_government_spread(
    target: &IcmaGovernmentSpreadTarget,
    target_resources: &c::CashHelperResources<'_>,
    securities: &[IcmaGovernmentSecurityInputs<'_>],
    reference_date: Date,
    settings: &IcmaGovernmentSpreadPriceSettings,
    quote: &GovernmentSpreadQuote,
) -> CashQuotationResult<IcmaGovernmentSpreadPrice> {
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
    let common = &settings.government;
    check_common(&target.icma, common)?;
    let t = &target.target;
    let state = GovernmentTargetState {
        currency: t.terms.currency,
        settlement: t.settlement,
        on_payment: t.on_payment,
        maturity: t.terms.schedule.end_date,
    };
    let contract = c::prepare_cash_contract(
        &t.terms,
        t.trade_date,
        t.settlement,
        target_resources,
        reference_date,
    )?;
    let cash = prepare_context(
        &contract,
        &t.security,
        ReferencePin {
            id: t.security.id.clone(),
            version: quote.quote_version,
        },
        &target.icma,
        t.settlement,
        t.on_payment,
        target_resources,
    )?;
    let mut used = 0;
    reserve(&mut used, &cash, common)?;
    let prepared = prepare_benchmarks(&state, securities, reference_date, common, &mut used)?;
    let benchmarks = quote_benchmarks(&prepared, securities, common)?;
    let (selected, benchmark_decimal_yield) = select(&state, &benchmarks, common.selection)?;
    let target_decimal_yield = finite(benchmark_decimal_yield + decimal_spread, "target yield")?;
    let valuation = cash.quote_price(target_decimal_yield)?;
    let w::WorkoutValue::Price(value) = &valuation.quotation else {
        return Err(invalid("ICMA quotation", "original price required"));
    };
    if value.dirty_per100 <= 0.0 || value.clean_per100 < 0.0 {
        return Err(invalid(
            "cash quote",
            "positive dirty and nonnegative clean original price required",
        ));
    }
    let roundtrip = cash.quote_yield(
        w::BondPriceTarget::Dirty(value.dirty_per100),
        common.inverse,
    )?;
    let inverse = yield_inverse(&roundtrip)?;
    let roundtrip_decimal_spread = finite(
        inverse.decimal_parameter - benchmark_decimal_yield,
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
            iterations: inverse.iterations,
            price_evaluations: inverse.price_evaluations,
        });
    }
    Ok(IcmaGovernmentSpreadPrice {
        reference_date,
        target: target.clone(),
        quote: quote.clone(),
        settings: settings.clone(),
        benchmarks,
        selected,
        benchmark_decimal_yield,
        target_decimal_yield,
        decimal_spread,
        basis_points: finite(decimal_spread * 10000.0, "G spread bp")?,
        valuation,
        yield_inverse: roundtrip,
        roundtrip_decimal_spread,
        decimal_spread_residual,
        reserved_reference_evaluations: used,
    })
}
