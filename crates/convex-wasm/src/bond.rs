//! Bond / curve construction and YAS-result conversion shared by analytics and pricing modules.

use convex_bonds::conventions::{ConventionKey, ConventionRegistry};
use convex_bonds::pricing::{
    dated_cash::{dated_cash_yield_risk, DatedCashYieldRisk},
    StandardYieldEngine, YieldEngine,
};
use convex_bonds::traits::Bond;
use convex_bonds::types::{
    CompoundingMethod, DayType, ExDivAccruedMethod, ExDividendRules, SettlementRules,
    YieldCalculationRules, YieldConvention,
};
use convex_bonds::{prelude::BondIdentifiers, FixedRateBond, FixedRateBondBuilder};
use convex_core::calendars::BusinessDayConvention;
use convex_core::types::{Compounding, Date};
use convex_curves::{
    DiscountCurve, DiscountCurveBuilder, InterpolationMethod, ZeroCurve, ZeroCurveBuilder,
};
use convex_math::portable;

use crate::convert::{
    decimal_to_f64, f64_to_decimal, finite, parse_compounding, parse_currency, parse_date,
    parse_day_count, parse_frequency, parse_instrument_type, parse_market, parse_yield_convention,
    percentage_to_decimal, positive,
};
use crate::dto::{BondParams, CurvePoint};

pub(crate) fn create_bond(params: &BondParams) -> Result<FixedRateBond, String> {
    let issue_date = parse_date(&params.issue_date)?;
    let maturity_date = parse_date(&params.maturity_date)?;

    // Convert coupon rate from percentage to decimal (e.g., 5.0% -> 0.05)
    let coupon = percentage_to_decimal(params.coupon_rate)?;
    let face = f64_to_decimal(params.face_value.unwrap_or(100.0))?;
    let frequency = parse_frequency(params.frequency.unwrap_or(2))?;
    let rules = get_yield_rules(params)?;
    let day_count = rules.accrual_day_count;
    let currency = parse_currency(params.currency.as_deref().unwrap_or("USD"))?;
    let settlement = parse_date(&params.settlement_date)?;
    if settlement < issue_date || settlement >= maturity_date {
        return Err(
            "Fixed convenience settlement must be on/after issue and before maturity".into(),
        );
    }

    let first_coupon = params
        .first_coupon_date
        .as_ref()
        .map(|s| parse_date(s))
        .transpose()?;

    // Create empty identifiers (WASM users don't need bond identifiers)
    let identifiers = BondIdentifiers::new();

    let mut builder = FixedRateBondBuilder::new()
        .identifiers(identifiers)
        .issue_date(issue_date)
        .maturity(maturity_date)
        .coupon_rate(coupon)
        .face_value(face)
        .redemption_value(face)
        .frequency(frequency)
        .day_count(day_count)
        .currency(currency)
        .business_day_convention(BusinessDayConvention::ModifiedFollowing);

    if let Some(fc) = first_coupon {
        builder = builder.first_coupon_date(fc);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to create bond: {:?}", e))
}

pub(crate) fn create_curve(
    reference_date: Date,
    points: &[CurvePoint],
) -> Result<ZeroCurve, String> {
    if points.is_empty() {
        return Err("Curve must have at least one point".to_string());
    }

    let mut builder = ZeroCurveBuilder::new()
        .reference_date(reference_date)
        .interpolation(InterpolationMethod::Linear);

    for point in points {
        let date = parse_date(&point.date)?;
        // Convert percentage to decimal (e.g., 4.5% -> 0.045)
        let rate = percentage_to_decimal(point.rate)?;
        builder = builder.add_rate(date, rate);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to create curve: {:?}", e))
}

/// Create the explicit discount curve used by the retained Z-spread convenience.
pub(crate) fn create_discount_curve(
    reference_date: Date,
    points: &[CurvePoint],
) -> Result<DiscountCurve, String> {
    if points.is_empty() {
        return Err("Curve must have at least one point".to_string());
    }

    let mut builder = DiscountCurveBuilder::new(reference_date);

    // Always add t=0 pillar with df=1.0 (spot date)
    builder = builder.add_pillar(0.0, 1.0);

    let mut pillars: Vec<(f64, f64)> = Vec::new();

    for point in points {
        let date = parse_date(&point.date)?;
        // DF(t) = exp(-r * t)
        let rate = decimal_to_f64(percentage_to_decimal(point.rate)?)?;
        let t = reference_date.days_between(&date) as f64 / 365.0;

        if t <= 0.0 {
            return Err("Discount curve points must follow the reference date".into());
        }

        let df = positive(portable::exp(-rate * t), "discount factor")?;
        pillars.push((t, df));
    }

    pillars.sort_by(|a, b| a.0.total_cmp(&b.0));
    if pillars.windows(2).any(|p| p[0].0 == p[1].0) {
        return Err("Duplicate discount curve point".into());
    }

    for (t, df) in pillars {
        builder = builder.add_pillar(t, df);
    }

    builder
        .with_extrapolation()
        .build()
        .map_err(|e| format!("Failed to create discount curve: {:?}", e))
}

/// Get yield calculation rules from parameters, using registry if market/type specified.
pub(crate) fn get_yield_rules(params: &BondParams) -> Result<YieldCalculationRules, String> {
    let market = params.market.as_deref().map(parse_market).transpose()?;
    let instrument = params
        .instrument_type
        .as_deref()
        .map(parse_instrument_type)
        .transpose()?;
    if instrument.is_some_and(|kind| {
        !matches!(
            kind,
            convex_bonds::conventions::InstrumentType::GovernmentBond
                | convex_bonds::conventions::InstrumentType::CorporateIG
                | convex_bonds::conventions::InstrumentType::CorporateHY
                | convex_bonds::conventions::InstrumentType::Municipal
                | convex_bonds::conventions::InstrumentType::Agency
                | convex_bonds::conventions::InstrumentType::Supranational
                | convex_bonds::conventions::InstrumentType::CoveredBond
        )
    }) {
        return Err("This fixed coupon convenience cannot represent the supplied instrument payoff; use complete library terms".into());
    }
    let registry = ConventionRegistry::global();
    let mut rules = if let Some(market) = market {
        if let Some(kind) = instrument {
            registry
                .rules(&ConventionKey::new(market, kind))
                .cloned()
                .ok_or("No registered convention rules for this market/instrument pair")?
        } else {
            registry.default_rules_for_market(market)
        }
    } else {
        YieldCalculationRules {
            convention: YieldConvention::StreetConvention,
            compounding: CompoundingMethod::Periodic { frequency: 2 },
            accrual_day_count: parse_day_count("30/360")?,
            discount_day_count: parse_day_count("30/360")?,
            settlement_rules: SettlementRules {
                days: 1,
                use_business_days: true,
                ..Default::default()
            },
            ..Default::default()
        }
    };
    if let Some(value) = &params.yield_convention {
        rules.convention = parse_yield_convention(value)?;
    }
    if let Some(value) = &params.compounding {
        rules.compounding = parse_compounding(value)?;
    }
    if let Some(value) = &params.day_count {
        rules.accrual_day_count = parse_day_count(value)?;
        rules.discount_day_count = rules.accrual_day_count;
    }
    if let Some(value) = params.frequency {
        rules.frequency = parse_frequency(value)?;
    }
    if let Some(value) = params.settlement_days {
        rules.settlement_rules.days = value;
    }
    if let Some(value) = params.use_business_days {
        rules.settlement_rules.use_business_days = value;
    }
    if let Some(days) = params.ex_dividend_days {
        rules.ex_dividend_rules = Some(ExDividendRules {
            days,
            day_type: if rules.settlement_rules.use_business_days {
                DayType::BusinessDays
            } else {
                DayType::CalendarDays
            },
            accrued_method: ExDivAccruedMethod::NegativeAccrued,
        });
    }
    Ok(rules)
}

/// Price/risk quotes use per100 cash; the original positive face is still validated.
pub(crate) fn create_price_bond(params: &BondParams) -> Result<FixedRateBond, String> {
    create_bond(params)?;
    let mut normalized = params.clone();
    normalized.face_value = Some(100.0);
    create_bond(&normalized)
}

/// Explicit context-free dated-cash basis; unsupported market mechanics fail.
pub(crate) fn dated_yield_compounding(
    rules: &YieldCalculationRules,
) -> Result<Compounding, String> {
    if rules
        .ex_dividend_rules
        .as_ref()
        .is_some_and(|r| r.days != 0)
    {
        return Err("Ex-dividend quotation requires full native entitlement/calendar terms".into());
    }
    match rules.compounding {
        CompoundingMethod::Periodic { frequency } if frequency > 0 => {
            Compounding::try_from_periods_per_year(frequency)
                .ok_or_else(|| "Unsupported dated yield frequency".into())
        }
        CompoundingMethod::Continuous => Ok(Compounding::Continuous),
        CompoundingMethod::Simple => Ok(Compounding::Simple),
        CompoundingMethod::ActualPeriod { .. } => {
            Err("Actual period quotation requires full contextual ICMA references".into())
        }
        _ => Err("Discount quotation requires an actual dated bill contract".into()),
    }
}

/// Solve and reprice the same cash/rate basis before exposing yield or risk.
pub(crate) fn calculate_dated_yield_quote(
    bond: &FixedRateBond,
    settlement: Date,
    clean_price: f64,
    rules: &YieldCalculationRules,
) -> Result<(f64, DatedCashYieldRisk), String> {
    positive(clean_price, "clean price")?;
    let cash = bond.try_cash_flows(settlement).map_err(|e| e.to_string())?;
    let accrued = bond.accrued_interest(settlement);
    let compounding = dated_yield_compounding(rules)?;
    // Validate native cash and contextual day count before running the retained solver.
    dated_cash_yield_risk(
        &cash,
        settlement,
        0.0,
        rules.discount_day_count,
        compounding,
    )
    .map_err(|e| e.to_string())?;
    let result = StandardYieldEngine::default()
        .yield_from_price(
            &cash,
            f64_to_decimal(clean_price)?,
            accrued,
            settlement,
            rules,
        )
        .map_err(|e| format!("Convention yield failed: {e}"))?;
    let rate = finite(result.yield_value, "dated yield")?;
    let risk = dated_cash_yield_risk(
        &cash,
        settlement,
        rate,
        rules.discount_day_count,
        compounding,
    )
    .map_err(|e| e.to_string())?;
    let target = finite(clean_price + decimal_to_f64(accrued)?, "dirty yield target")?;
    if (risk.present_value - target).abs() > 1e-8 {
        return Err("Dated yield failed the unchanged absolute per100 repricing bound1e-8".into());
    }
    Ok((rate, risk))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::CurvePoint;
    use convex_bonds::traits::FixedCouponBond;

    #[test]
    fn test_create_bond() {
        let params = BondParams {
            coupon_rate: 5.0,
            maturity_date: "2030-06-15".to_string(),
            issue_date: "2020-06-15".to_string(),
            settlement_date: "2024-06-15".to_string(),
            face_value: Some(100.0),
            frequency: Some(2),
            day_count: Some("30/360".to_string()),
            currency: Some("USD".to_string()),
            first_coupon_date: None,
            market: None,
            instrument_type: None,
            yield_convention: None,
            compounding: None,
            settlement_days: None,
            ex_dividend_days: None,
            use_business_days: None,
        };

        let bond = create_bond(&params).unwrap();
        // Coupon rate stored as decimal (0.05 for 5%)
        assert_eq!(decimal_to_f64(bond.coupon_rate()).unwrap(), 0.05);
    }

    #[test]
    fn test_create_curve() {
        let reference = Date::from_ymd(2024, 6, 15).unwrap();
        let points = vec![
            CurvePoint {
                date: "2025-06-15".to_string(),
                rate: 4.0,
            },
            CurvePoint {
                date: "2026-06-15".to_string(),
                rate: 4.5,
            },
            CurvePoint {
                date: "2029-06-15".to_string(),
                rate: 5.0,
            },
        ];

        let curve = create_curve(reference, &points).unwrap();
        assert_eq!(curve.reference_date(), reference);
    }
}
