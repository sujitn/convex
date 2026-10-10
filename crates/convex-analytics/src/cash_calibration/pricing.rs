//! Original settlement/cash quotations and explicitly declared basis models.

use super::*;
use crate::bond_contract::{discounted_projection, price_fixed_government, ContractPrice};
use convex_bonds::contracts::{BondContract, BondCoupon, BondProjection};
use convex_core::{
    cashflows::{BoundarySide, CashRounding},
    daycounts::DayCountConvention,
};
use convex_math::solvers::{brent, SolverConfig};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use std::cell::RefCell;

pub(super) struct PreparedBond {
    pub bond: BondContract,
    pub projection: BondProjection,
    pub observed: CashQuoteConversion,
    pub par_cash: Option<(BondProjection, BondProjection)>,
    pub icma: Option<super::icma::PreparedIcmaHelper>,
}
pub(super) fn number(v: Decimal) -> CashCalibrationResult<f64> {
    v.to_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid("cash", "finite Decimal conversion required"))
}
fn price_cash(
    bond: &BondContract,
    projection: &BondProjection,
    mut df: impl FnMut(Date) -> CashCalibrationResult<f64>,
) -> CashCalibrationResult<f64> {
    let mut pv = 0.0;
    for cash in projection.cashflows()? {
        pv = finite(pv + number(cash.amount)? * df(cash.date)?, "cash price")?;
    }
    finite(
        pv / number(bond.terms().face_value)? * 100.0,
        "price_per100",
    )
}
fn yield_settings(
    c: CashYieldConvention,
    projection: &BondProjection,
    settlement: Date,
) -> CashCalibrationResult<()> {
    if !c.lower_yield.is_finite()
        || !c.upper_yield.is_finite()
        || c.lower_yield >= c.upper_yield
        || c.lower_yield < -10.0
        || c.upper_yield > 10.0
        || c.max_iterations == 0
        || c.max_iterations > 256
        || !c.price_tolerance.is_finite()
        || c.price_tolerance <= 0.0
        || c.price_tolerance > 1.0
    {
        return Err(invalid(
            "yield convention",
            "finite closed rate bracket, positive price tolerance and bounded iterations required",
        ));
    }
    for cash in projection.cashflows()? {
        if cash.amount < Decimal::ZERO {
            return Err(invalid(
                "yield cash",
                "negative cash requires a non-monotone inverse model",
            ));
        }
        let tau = if cash.date == settlement {
            0.0
        } else {
            year_fraction(settlement, cash.date, c.day_count)?
        };
        discount_from_rate(c.lower_yield, tau, c.compounding)?;
        discount_from_rate(c.upper_yield, tau, c.compounding)?;
    }
    Ok(())
}
fn yield_price(
    p: &PreparedBond,
    settlement: Date,
    c: CashYieldConvention,
    y: f64,
) -> CashCalibrationResult<f64> {
    price_cash(&p.bond, &p.projection, |date| {
        let tau = if date == settlement {
            0.0
        } else {
            year_fraction(settlement, date, c.day_count)?
        };
        Ok(discount_from_rate(y, tau, c.compounding)?)
    })
}
fn inverse_yield(
    p: &PreparedBond,
    h: &CashBondHelper,
    c: CashYieldConvention,
    dirty: f64,
) -> CashCalibrationResult<f64> {
    let error = RefCell::new(None);
    let root = brent(
        |y| match yield_price(p, h.settlement, c, y) {
            Ok(v) => v - dirty,
            Err(e) => {
                *error.borrow_mut() = Some(e);
                f64::NAN
            }
        },
        c.lower_yield,
        c.upper_yield,
        &SolverConfig {
            tolerance: c.price_tolerance,
            max_iterations: c.max_iterations,
        },
    );
    if let Some(e) = error.into_inner() {
        return Err(e);
    }
    let root = root?;
    if (yield_price(p, h.settlement, c, root.root)? - dirty).abs() > c.price_tolerance {
        return Err(invalid(
            "yield inverse",
            "final original dirty-price residual exceeds declared bound",
        ));
    }
    Ok(root.root)
}
fn bill(
    p: &PreparedBond,
    h: &CashBondHelper,
    dc: DayCountConvention,
) -> CashCalibrationResult<(Date, f64)> {
    let flows: Vec<_> = p
        .projection
        .cashflows()?
        .into_iter()
        .filter(|c| !c.amount.is_zero())
        .collect();
    if flows.len() != 1
        || flows[0].date <= h.settlement
        || flows[0].amount != p.bond.terms().face_value
        || !p.projection.accrued.is_zero()
        || p.projection.coupons.iter().any(|c| !c.amount.is_zero())
    {
        return Err(invalid(
            "bill",
            "one future par principal, zero coupons and zero accrued required",
        ));
    }
    Ok((
        flows[0].date,
        year_fraction(h.settlement, flows[0].date, dc)?,
    ))
}

pub(super) fn term_bounds(t: &convex_bonds::contracts::BondTerms) -> CashCalibrationResult<()> {
    let max = convex_core::cashflows::MAX_SCHEDULE_PERIODS;
    if [
        &t.profiles.coupon,
        &t.profiles.spread,
        &t.profiles.gearing,
        &t.profiles.notional,
    ]
    .iter()
    .any(|p| p.len() > max)
        || t.principal_prices.len() > max
        || !t.exercise.is_empty()
    {
        return Err(invalid(
            "cash terms",
            "bounded plain supplied profiles/principal required; optional helpers unsupported",
        ));
    }
    Ok(())
}
pub(crate) fn prepare_cash_contract(
    terms: &convex_bonds::contracts::BondTerms,
    trade_date: Date,
    settlement: Date,
    resources: &CashHelperResources<'_>,
    reference: Date,
) -> CashCalibrationResult<BondContract> {
    let (ids, calendars) = resources.bond()?;
    if ids != &terms.calendars {
        return Err(invalid(
            "calendars",
            "exact supplied identities/versions differ",
        ));
    }
    if !(1..=9999).contains(&trade_date.year()) || trade_date > reference || settlement < reference
    {
        return Err(invalid(
            "cash helper",
            "bounded identity, trade/settlement dates and positive original tolerance required",
        ));
    }
    let expected = convex_core::cashflows::settlement_date(
        trade_date,
        terms.settlement_lag,
        terms.settlement_adjustment,
        calendars.payment,
    )?;
    if expected != settlement {
        return Err(invalid(
            "settlement",
            "does not match original trade/lag/calendar/adjustment",
        ));
    }
    term_bounds(terms)?;
    let bond = terms.clone().build_with_calendars(calendars)?;
    bond.require_plain_valuation()?;
    if !matches!(bond.terms().coupon, BondCoupon::Fixed) {
        return Err(invalid("cash coupon", "supplied fixed/stepped/zero coupon cash required; floating/optional helpers unsupported"));
    }
    Ok(bond)
}
pub(super) fn admit(
    h: &CashBondHelper,
    resources: &CashHelperResources<'_>,
    reference: Date,
) -> CashCalibrationResult<PreparedBond> {
    resources.bond()?;
    h.quote.validate()?;
    if !label(&h.id) || !h.tolerance.is_finite() || h.tolerance <= 0.0 || h.tolerance > 1.0 {
        return Err(invalid(
            "cash helper",
            "bounded identity and positive original tolerance required",
        ));
    }
    let bond = prepare_cash_contract(&h.terms, h.trade_date, h.settlement, resources, reference)?;
    let projection = bond.fixed_projection(h.settlement, h.on_payment)?;
    let flows = projection.cashflows()?;
    if flows.is_empty()
        || !flows
            .iter()
            .any(|c| c.date > h.settlement && c.amount > Decimal::ZERO)
        || flows.iter().any(|c| c.date > h.pillar)
        || h.pillar <= reference
    {
        return Err(invalid(
            "cash pillar",
            "future informative cash and full actual delayed payment horizon required",
        ));
    }
    let accrued = number(projection.accrued)? / number(bond.terms().face_value)? * 100.0;
    let observed = CashQuoteConversion {
        settlement: h.settlement,
        quote_kind: h.quote_kind.clone(),
        quote: h.quote,
        accrued_per100: accrued,
        clean_per100: 0.0,
        dirty_per100: 0.0,
        icma: None,
        icma_work: None,
    };
    let icma = match &h.quote_kind {
        CashQuoteKind::ContextualIcmaYield { input } => Some(super::icma::PreparedIcmaHelper::new(
            &bond, h, input, resources,
        )?),
        _ => None,
    };
    Ok(PreparedBond {
        bond,
        projection,
        observed,
        par_cash: None,
        icma,
    })
}
pub(super) fn observe(
    prepared: &mut PreparedBond,
    h: &CashBondHelper,
    resources: &CashHelperResources<'_>,
) -> CashCalibrationResult<()> {
    let (_, calendars) = resources.bond()?;
    let accrued = prepared.observed.accrued_per100;
    let dirty = match &h.quote_kind {
        CashQuoteKind::CleanPrice | CashQuoteKind::DirtyPrice => {
            if h.quote.unit != QuoteUnit::PricePer100 {
                return Err(invalid("quote.unit", "clean/dirty requires PricePer100"));
            }
            if h.quote_kind == CashQuoteKind::CleanPrice {
                h.quote.value + accrued
            } else {
                h.quote.value
            }
        }
        CashQuoteKind::CashflowYield { convention } => {
            yield_settings(*convention, &prepared.projection, h.settlement)?;
            let y = h.quote.decimal_rate()?;
            if y < convention.lower_yield || y > convention.upper_yield {
                return Err(invalid("yield quote", "outside declared inverse bracket"));
            }
            yield_price(prepared, h.settlement, *convention, y)?
        }
        CashQuoteKind::ContextualIcmaYield { .. } => {
            let quotation = prepared
                .icma
                .as_ref()
                .ok_or_else(|| invalid("ICMA helper", "unprepared original context"))?
                .observed(h)?;
            let crate::workflow::WorkoutValue::Price(value) = &quotation.quotation else {
                return Err(invalid("ICMA helper", "original observed price required"));
            };
            let dirty = value.dirty_per100;
            prepared.observed.icma = Some(Box::new(quotation));
            prepared.observed.icma_work = prepared.icma.as_ref().map(|i| i.work());
            dirty
        }
        CashQuoteKind::BankDiscountYield { day_count } => {
            let (_, tau) = bill(prepared, h, *day_count)?;
            100.0 * (1.0 - h.quote.decimal_rate()? * tau)
        }
        CashQuoteKind::ParCouponRate => {
            let y = h.quote.decimal_rate()?;
            if h.terms.rounding != CashRounding::Unrounded
                || h.terms
                    .profiles
                    .notional
                    .iter()
                    .any(|n| n.value != h.terms.face_value)
                || h.terms.redemption_multiplier != Decimal::ONE
                || !h.terms.principal_prices.is_empty()
                || h.terms.negative_coupons != convex_core::cashflows::NegativeCouponPolicy::Allow
                || h.terms.profiles.coupon.iter().any(|c| {
                    c.value.to_string().parse::<f64>().ok().is_none_or(|v| {
                        v != y
                            && h.quote
                                .model_rate_quote(v)
                                .ok()
                                .is_none_or(|q| q.value != h.quote.value)
                    })
                })
            {
                return Err(invalid(
                    "par coupon",
                    "original uniform quoted coupon, unrounded unclipped nominal bullet required",
                ));
            }
            let mut zero = h.terms.clone();
            let mut unit = h.terms.clone();
            for c in &mut zero.profiles.coupon {
                c.value = Decimal::ZERO;
            }
            for c in &mut unit.profiles.coupon {
                c.value = Decimal::ONE;
            }
            let zero = zero
                .build_with_calendars(calendars)?
                .fixed_projection(h.settlement, h.on_payment)?;
            let unit = unit
                .build_with_calendars(calendars)?
                .fixed_projection(h.settlement, h.on_payment)?;
            prepared.par_cash = Some((zero, unit));
            100.0 + accrued
        }
    };
    if !dirty.is_finite()
        || dirty <= 0.0
        || !((dirty - accrued).is_finite())
        || dirty - accrued < 0.0
    {
        return Err(invalid(
            "cash quote",
            "positive dirty and nonnegative clean original price required",
        ));
    }
    prepared.observed.dirty_per100 = dirty;
    prepared.observed.clean_per100 = dirty - accrued;
    Ok(())
}
pub(super) fn prepare(
    h: &CashBondHelper,
    resources: &CashHelperResources<'_>,
    reference: Date,
) -> CashCalibrationResult<PreparedBond> {
    let mut prepared = admit(h, resources, reference)?;
    observe(&mut prepared, h, resources)?;
    Ok(prepared)
}

/// Convert a full original clean/dirty/yield/par observation through actual
/// settlement, entitlement and accrued. No model curve is needed or synthesized.
pub fn convert_cash_quote(
    h: &CashBondHelper,
    resources: &CashHelperResources<'_>,
    reference_date: Date,
) -> CashCalibrationResult<CashQuoteConversion> {
    Ok(prepare(h, resources, reference_date)?.observed)
}

fn check_reference_recipe(
    curve: &CurveRecipe,
    currency: convex_core::Currency,
    role: &BondReferenceRole,
) -> CashCalibrationResult<()> {
    let matches = match (role, &curve.role) {
        (
            BondReferenceRole::Discount,
            CurveRole::Discount {
                collateral_currency,
            },
        ) => *collateral_currency == currency,
        (
            BondReferenceRole::Government { benchmark_id },
            CurveRole::Government {
                benchmark_id: actual,
            },
        ) => benchmark_id == actual,
        _ => false,
    };
    if curve.currency != currency || !matches {
        return Err(invalid(
            "reference role",
            "exact currency/collateral/government benchmark differs",
        ));
    }
    Ok(())
}

pub(super) fn check_basis_metadata(
    h: &CashBondHelper,
    basis: &CurveRecipe,
    model: &BondBasisModel,
    reference: &CurveRecipe,
    credit: Option<&CurveRecipe>,
) -> CashCalibrationResult<()> {
    let CurveRole::IssuerSpread {
        entity_id,
        model_id,
    } = &basis.role
    else {
        return Err(invalid("basis role", "IssuerSpread required"));
    };
    let identity = h.credit_identity.as_ref().ok_or_else(|| {
        invalid(
            "issuer mapping",
            "actual supplied legal bond identity required",
        )
    })?;
    if basis.currency != h.terms.currency
        || basis.time_basis != DayCountConvention::Act365Fixed
        || identity.currency != h.terms.currency
        || identity.maturity != h.terms.schedule.end_date
        || identity.entity_id.as_str() != entity_id
    {
        return Err(invalid(
            "issuer mapping",
            "exact entity/currency/maturity and ACT365F basis required",
        ));
    }
    match model {
        BondBasisModel::ReferenceZeroSpread {
            reference: key,
            role,
        } => {
            if model_id != "reference_zero_spread_v1" || reference.key != *key || credit.is_some() {
                return Err(invalid(
                    "basis model",
                    "exact reference and declared reference_zero_spread_v1 model required",
                ));
            }
            check_reference_recipe(reference, h.terms.currency, role)?;
        }
        BondBasisModel::CreditParRecovery {
            discount,
            survival,
            seniority,
            recovery,
        } => {
            let credit = credit
                .ok_or_else(|| invalid("credit mapping", "actual exact credit recipe required"))?;
            if model_id != "credit_par_recovery_promised_basis_v1"
                || reference.key != *discount
                || credit.key != *survival
                || !recovery.is_finite()
                || !(0.0..=1.0).contains(recovery)
                || identity.seniority.as_str() != seniority
                || credit.currency != h.terms.currency
                || !matches!(&credit.role, CurveRole::Credit { entity_id: e, seniority: s, recovery: r } if e==entity_id && s==seniority && r==recovery)
                || reference.reference_date != basis.reference_date
                || credit.reference_date != basis.reference_date
            {
                return Err(invalid("credit mapping", "exact model/entity/seniority/currency/recovery/versions and common reference date required"));
            }
            check_reference_recipe(reference, h.terms.currency, &BondReferenceRole::Discount)?;
            for (curve, representation) in [
                (reference, CurveRepresentation::DiscountFactors),
                (credit, CurveRepresentation::SurvivalProbabilities),
            ] {
                check_recovery_grid(curve, representation)?;
            }
        }
    }
    Ok(())
}
fn check_recovery_grid(
    c: &CurveRecipe,
    representation: CurveRepresentation,
) -> CashCalibrationResult<()> {
    if c.representation != representation
        || c.time_basis != DayCountConvention::Act365Fixed
        || c.interpolation != convex_curves::InterpolationMethod::LogLinear
        || c.extrapolation != convex_curves::ExtrapolationMethod::None
    {
        return Err(invalid(
            "recovery grid",
            "direct/fitted ACT365F log-linear DF/survival with no extrapolation required",
        ));
    }
    Ok(())
}
pub(super) fn check_credit_cash(p: &PreparedBond) -> CashCalibrationResult<()> {
    if p.bond
        .principal_cashflows()
        .iter()
        .any(|c| c.effective_date != c.payment_date)
    {
        return Err(invalid("credit principal", "effective principal and cash dates must coincide; delayed/defaulted cash needs lifecycle"));
    }
    Ok(())
}

fn recovery(
    p: &PreparedBond,
    h: &CashBondHelper,
    discount: &BuiltCurve,
    survival: &BuiltCurve,
    r: f64,
) -> CashCalibrationResult<f64> {
    // Shared credit segment integration is exact only for the admitted grids.
    for (curve, representation) in [
        (discount, CurveRepresentation::DiscountFactors),
        (survival, CurveRepresentation::SurvivalProbabilities),
    ] {
        check_recovery_grid(curve.recipe(), representation)?;
        curve.interpolation_grid()?;
    }
    check_credit_cash(p)?;
    let start = discount.date_to_tenor(h.settlement)?;
    let end = discount.date_to_tenor(h.terms.schedule.end_date)?;
    if end <= start {
        return Err(invalid(
            "credit life",
            "positive remaining recovery horizon required",
        ));
    }
    let mut points = vec![start, end];
    for curve in [discount, survival] {
        let (knots, _) = curve.interpolation_grid()?;
        points.extend(knots.iter().copied().filter(|t| *t > start && *t < end));
    }
    let mut principal = vec![(
        start,
        number(
            p.bond
                .outstanding_principal(h.settlement, BoundarySide::After)?,
        )?,
    )];
    for n in &h.terms.profiles.notional {
        if n.start <= h.settlement {
            continue;
        }
        let time = discount.date_to_tenor(n.start)?;
        if time > start && time < end {
            points.push(time);
        }
        principal.push((time, number(n.value)?));
    }
    if points.len() > convex_credit::MAX_INTEGRATION_POINTS {
        return Err(CurveContractError::BoundExceeded("bond recovery integration grid").into());
    }
    points.sort_by(f64::total_cmp);
    points.dedup();
    let df = |t| -> CashCalibrationResult<f64> {
        Ok(discount
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Tenor(t),
            })?
            .value)
    };
    let q = |t| -> CashCalibrationResult<f64> {
        Ok(survival
            .query(&CurveQuery::SurvivalProbability {
                point: CurvePoint::Tenor(t),
            })?
            .value)
    };
    let d0 = df(start)?;
    let q0 = q(start)?;
    let mut total = 0.0;
    for w in points.windows(2) {
        let n = principal
            .iter()
            .rev()
            .find(|(t, _)| *t <= w[0])
            .map(|(_, n)| *n)
            .ok_or_else(|| invalid("principal", "missing actual outstanding profile"))?;
        let mass = convex_credit::discounted_default_mass(
            df(w[0])? / d0,
            df(w[1])? / d0,
            q(w[0])? / q0,
            q(w[1])? / q0,
        )?;
        total = finite(total + r * n * mass, "recovery PV")?;
    }
    Ok(total)
}

pub(super) fn basis_price(
    p: &PreparedBond,
    h: &CashBondHelper,
    model: &BondBasisModel,
    basis: &BuiltCurve,
    curves: &CurveSet,
) -> CashCalibrationResult<BasisBondValuation> {
    let (reference_key, credit, recovery_rate) = match model {
        BondBasisModel::ReferenceZeroSpread { reference, .. } => (reference, None, 0.0),
        BondBasisModel::CreditParRecovery {
            discount,
            survival,
            recovery,
            ..
        } => (discount, Some(curves.get(survival)?.as_ref()), *recovery),
    };
    let reference = curves.get(reference_key)?;
    check_basis_metadata(
        h,
        basis.recipe(),
        model,
        reference.recipe(),
        credit.map(BuiltCurve::recipe),
    )?;
    let recovery_pv = if let Some(credit) = credit {
        recovery(p, h, reference, credit, recovery_rate)?
    } else {
        0.0
    };
    let weight = |date| -> CashCalibrationResult<f64> {
        let df = reference
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Date(date),
            })?
            .value;
        let t = basis.date_to_tenor(date)?;
        let spread = basis.issuer_spread(CurvePoint::Date(date))?;
        let q = if let Some(credit) = credit {
            credit
                .query(&CurveQuery::SurvivalProbability {
                    point: CurvePoint::Date(date),
                })?
                .value
        } else {
            1.0
        };
        let v = finite(df * q * (-spread * t).exp(), "promised cash factor")?;
        if v <= 0.0 {
            return Err(invalid(
                "promised cash factor",
                "positive finite factor required",
            ));
        }
        Ok(v)
    };
    let price = discounted_projection(
        &p.bond,
        h.settlement,
        p.projection.clone(),
        reference_key.clone(),
        |date| {
            weight(date).map_err(|e| {
                BondContractError::Cashflow(CashflowError::InvalidTerms {
                    field: "basis factor",
                    reason: e.to_string(),
                })
            })
        },
        recovery_pv,
    )?;
    Ok(BasisBondValuation {
        promised_pv: price.present_value - recovery_pv,
        price,
        basis: basis.recipe().key.clone(),
        model: model.clone(),
        recovery_pv,
    })
}

/// Price full original fixed/stepped/amortizing issuer cash with a declared
/// residual model, exact reference/credit dependencies and separate recovery PV.
pub fn price_basis_bond(
    h: &CashBondHelper,
    resources: &CashHelperResources<'_>,
    model: &BondBasisModel,
    basis: &BuiltCurve,
    curves: &CurveSet,
) -> CashCalibrationResult<BasisBondValuation> {
    let p = prepare(h, resources, basis.recipe().reference_date)?;
    basis_price(&p, h, model, basis, curves)
}

pub(super) fn model_quote(
    p: &PreparedBond,
    h: &CashBondHelper,
    value: &ContractPrice,
    curve: &BuiltCurve,
) -> CashCalibrationResult<(MarketQuote, Option<Box<crate::workflow::IcmaQuotation>>)> {
    let mut icma = None;
    let quote =
        match &h.quote_kind {
            CashQuoteKind::CleanPrice => MarketQuote {
                value: value.clean_price,
                unit: h.quote.unit,
            },
            CashQuoteKind::DirtyPrice => MarketQuote {
                value: value.dirty_price,
                unit: h.quote.unit,
            },
            CashQuoteKind::CashflowYield { convention } => h
                .quote
                .model_rate_quote(inverse_yield(p, h, *convention, value.dirty_price)?)?,
            CashQuoteKind::ContextualIcmaYield { .. } => {
                let quotation = p
                    .icma
                    .as_ref()
                    .ok_or_else(|| invalid("ICMA helper", "unprepared original context"))?
                    .model(value.dirty_price)?;
                let crate::workflow::WorkoutValue::Yield(inverse) = &quotation.quotation else {
                    return Err(invalid("ICMA helper", "original model inverse required"));
                };
                let quote = h.quote.model_rate_quote(inverse.decimal_parameter)?;
                icma = Some(Box::new(quotation));
                quote
            }
            CashQuoteKind::BankDiscountYield { day_count } => {
                let (_, tau) = bill(p, h, *day_count)?;
                h.quote
                    .model_rate_quote((1.0 - value.dirty_price / 100.0) / tau)?
            }
            CashQuoteKind::ParCouponRate => {
                let (zero, unit) = p.par_cash.as_ref().ok_or_else(|| {
                    invalid("par", "missing complete original unit coupon exposure")
                })?;
                let df0 = curve
                    .query(&CurveQuery::DiscountFactor {
                        point: CurvePoint::Date(h.settlement),
                    })?
                    .value;
                let clean = |projection: &BondProjection| -> CashCalibrationResult<f64> {
                    Ok(price_cash(&p.bond, projection, |date| {
                        Ok(curve
                            .query(&CurveQuery::DiscountFactor {
                                point: CurvePoint::Date(date),
                            })?
                            .value
                            / df0)
                    })? - number(projection.accrued)? / number(h.terms.face_value)? * 100.0)
                };
                let z = clean(zero)?;
                let u = clean(unit)?;
                if u - z <= 0.0 {
                    return Err(invalid(
                        "par",
                        "positive original clean coupon exposure required",
                    ));
                }
                h.quote.model_rate_quote((100.0 - z) / (u - z))?
            }
        };
    quote.validate()?;
    Ok((quote, icma))
}

pub(super) fn government_price(
    p: &PreparedBond,
    h: &CashBondHelper,
    curve: &BuiltCurve,
) -> CashCalibrationResult<ContractPrice> {
    let CurveRole::Government { benchmark_id } = &curve.recipe().role else {
        return Err(invalid("government", "exact government role required"));
    };
    Ok(price_fixed_government(
        &p.bond,
        h.settlement,
        h.on_payment,
        curve,
        benchmark_id,
    )?)
}
