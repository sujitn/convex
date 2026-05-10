//! Bond / curve construction and YAS-result conversion shared by analytics and pricing modules.

use convex_analytics::yas::YASResult;
use convex_bonds::conventions::{ConventionKey, ConventionRegistry};
use convex_bonds::pricing::{StandardYieldEngine, YieldEngine};
use convex_bonds::traits::Bond;
use convex_bonds::types::{
    CompoundingMethod, DayType, ExDivAccruedMethod, ExDividendRules, SettlementRules,
    YieldCalculationRules, YieldConvention,
};
use convex_bonds::{prelude::BondIdentifiers, FixedRateBond, FixedRateBondBuilder};
use convex_core::calendars::BusinessDayConvention;
use convex_core::types::Date;
use convex_curves::{
    DiscountCurve, DiscountCurveBuilder, InterpolationMethod, ZeroCurve, ZeroCurveBuilder,
};

use crate::convert::{
    decimal_to_f64, f64_to_decimal, format_compounding, format_instrument_type, format_market_name,
    format_yield_convention, log, parse_compounding, parse_currency, parse_date, parse_day_count,
    parse_frequency, parse_instrument_type, parse_market, parse_yield_convention,
};
use crate::dto::{AnalysisResult, BondParams, CurvePoint};

pub(crate) fn create_bond(params: &BondParams) -> Result<FixedRateBond, String> {
    let issue_date = parse_date(&params.issue_date)?;
    let maturity_date = parse_date(&params.maturity_date)?;

    // Convert coupon rate from percentage to decimal (e.g., 5.0% -> 0.05)
    let coupon = f64_to_decimal(params.coupon_rate / 100.0);
    let face = f64_to_decimal(params.face_value.unwrap_or(100.0));
    let frequency = parse_frequency(params.frequency.unwrap_or(2));
    let day_count = parse_day_count(params.day_count.as_deref().unwrap_or("30/360"));
    let currency = parse_currency(params.currency.as_deref().unwrap_or("USD"));

    let first_coupon = params
        .first_coupon_date
        .as_ref()
        .and_then(|s| parse_date(s).ok());

    // Create empty identifiers (WASM users don't need bond identifiers)
    let identifiers = BondIdentifiers::new();

    let mut builder = FixedRateBondBuilder::new()
        .identifiers(identifiers)
        .issue_date(issue_date)
        .maturity(maturity_date)
        .coupon_rate(coupon)
        .face_value(face)
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
        let rate = f64_to_decimal(point.rate / 100.0);
        builder = builder.add_rate(date, rate);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to create curve: {:?}", e))
}

/// Create a DiscountCurve for OAS calculations (implements the Curve trait).
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
        let rate = point.rate / 100.0;
        let t = reference_date.days_between(&date) as f64 / 365.0;

        if t <= 0.0 {
            continue;
        }

        let df = (-rate * t).exp();
        pillars.push((t, df));
    }

    pillars.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    for (t, df) in pillars {
        builder = builder.add_pillar(t, df);
    }

    builder
        .with_extrapolation()
        .build()
        .map_err(|e| format!("Failed to create discount curve: {:?}", e))
}

/// Get yield calculation rules from parameters, using registry if market/type specified.
pub(crate) fn get_yield_rules(params: &BondParams) -> YieldCalculationRules {
    if let (Some(market_str), Some(inst_str)) = (&params.market, &params.instrument_type) {
        let market = parse_market(market_str);
        let instrument_type = parse_instrument_type(inst_str);

        let registry = ConventionRegistry::global();
        let key = ConventionKey::new(market, instrument_type);

        if let Some(rules) = registry.rules(&key) {
            return rules.clone();
        }

        return registry.default_rules_for_market(market);
    }

    if let Some(market_str) = &params.market {
        let market = parse_market(market_str);
        let registry = ConventionRegistry::global();
        return registry.default_rules_for_market(market);
    }

    let convention = params
        .yield_convention
        .as_ref()
        .map(|s| parse_yield_convention(s))
        .unwrap_or(YieldConvention::StreetConvention);

    let compounding = params
        .compounding
        .as_ref()
        .map(|s| parse_compounding(s))
        .unwrap_or(CompoundingMethod::Periodic { frequency: 2 });

    let day_count = parse_day_count(params.day_count.as_deref().unwrap_or("30/360"));

    let settlement_rules = SettlementRules {
        days: params.settlement_days.unwrap_or(1),
        use_business_days: params.use_business_days.unwrap_or(true),
        ..Default::default()
    };

    let ex_dividend_rules = params.ex_dividend_days.map(|days| ExDividendRules {
        days,
        day_type: DayType::BusinessDays,
        accrued_method: ExDivAccruedMethod::NegativeAccrued,
    });

    YieldCalculationRules {
        convention,
        compounding,
        accrual_day_count: day_count,
        discount_day_count: day_count,
        settlement_rules,
        ex_dividend_rules,
        ..Default::default()
    }
}

/// Calculate yield using StandardYieldEngine with convention rules.
/// Returns yield as a decimal (e.g., 0.05 for 5%).
pub(crate) fn calculate_convention_yield(
    bond: &FixedRateBond,
    settlement: Date,
    clean_price: f64,
    rules: &YieldCalculationRules,
) -> Option<f64> {
    let cash_flows = bond.cash_flows(settlement);

    if cash_flows.is_empty() {
        log("Convention yield: No cash flows");
        return None;
    }

    let accrued = bond.accrued_interest(settlement);

    let engine = StandardYieldEngine::default();
    let clean_price_dec = f64_to_decimal(clean_price);

    log(&format!(
        "Convention yield calc: compounding={:?}, convention={:?}, cash_flows={}, price={}, accrued={}",
        rules.compounding, rules.convention, cash_flows.len(), clean_price, decimal_to_f64(accrued)
    ));

    match engine.yield_from_price(&cash_flows, clean_price_dec, accrued, settlement, rules) {
        Ok(result) => {
            log(&format!(
                "Convention yield result: {:.6} ({:.4}%)",
                result.yield_value,
                result.yield_value * 100.0
            ));
            Some(result.yield_value)
        }
        Err(e) => {
            log(&format!("Convention yield error: {:?}", e));
            None
        }
    }
}

/// Convert a YAS-engine result into the wire-format AnalysisResult, including convention info.
pub(crate) fn convert_yas_result(
    result: &YASResult,
    bond: &FixedRateBond,
    settlement: Date,
    rules: &YieldCalculationRules,
    bond_params: &BondParams,
) -> AnalysisResult {
    let (days_to_mat, years_to_mat) = match bond.maturity() {
        Some(maturity) => {
            let days = settlement.days_between(&maturity);
            (days, days as f64 / 365.0)
        }
        None => (0, 0.0),
    };

    let clean_price = decimal_to_f64(result.invoice.clean_price);
    let accrued = decimal_to_f64(result.invoice.accrued_interest);
    let dirty_price = decimal_to_f64(result.invoice.dirty_price);

    let market_display = bond_params
        .market
        .as_ref()
        .map(|s| format_market_name(parse_market(s)));
    let instrument_display = bond_params
        .instrument_type
        .as_ref()
        .map(|s| format_instrument_type(parse_instrument_type(s)));

    let is_ex_dividend = if let Some(ref ex_rules) = rules.ex_dividend_rules {
        if let Some(next_coupon) = bond.next_coupon_date(settlement) {
            let days_to_coupon = settlement.days_between(&next_coupon);
            days_to_coupon <= ex_rules.days as i64
        } else {
            false
        }
    } else {
        false
    };

    AnalysisResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(accrued),

        ytm: Some(decimal_to_f64(result.ytm)),
        current_yield: Some(decimal_to_f64(result.current_yield)),
        simple_yield: Some(decimal_to_f64(result.simple_yield)),
        money_market_yield: result.money_market_yield.map(decimal_to_f64),

        ytc: None,
        ytw: None,
        workout_date: None,
        workout_price: None,

        modified_duration: Some(decimal_to_f64(result.modified_duration())),
        macaulay_duration: Some(decimal_to_f64(result.risk.macaulay_duration.years())),
        convexity: Some(decimal_to_f64(result.convexity())),
        dv01: Some(decimal_to_f64(result.dv01())),

        g_spread: Some(decimal_to_f64(result.g_spread.as_bps())),
        benchmark_spread: Some(decimal_to_f64(result.benchmark_spread.as_bps())),
        benchmark_tenor: Some(result.benchmark_tenor.clone()),
        z_spread: Some(decimal_to_f64(result.z_spread.as_bps())),
        asw_spread: result
            .asw_spread
            .as_ref()
            .map(|s| decimal_to_f64(s.as_bps())),
        oas: None,

        effective_duration: None,
        effective_convexity: None,
        option_value: None,

        days_to_maturity: Some(days_to_mat),
        years_to_maturity: Some(years_to_mat),
        is_callable: None,

        market: market_display,
        instrument_type: instrument_display,
        yield_convention: Some(format_yield_convention(rules.convention)),
        compounding_method: Some(format_compounding(rules.compounding)),
        settlement_days: Some(rules.settlement_rules.days),
        ex_dividend_days: rules.ex_dividend_rules.as_ref().map(|r| r.days),
        is_ex_dividend: Some(is_ex_dividend),

        error: None,
    }
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
            call_schedule: None,
            volatility: None,
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
        assert_eq!(decimal_to_f64(bond.coupon_rate()), 0.05);
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
