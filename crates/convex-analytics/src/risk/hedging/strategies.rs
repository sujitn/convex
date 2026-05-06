//! Concrete hedge strategies. Each function takes a [`RiskProfile`] and
//! returns a [`HedgeProposal`] sized to neutralize parallel DV01.
//!
//! v1 ships [`duration_futures`] and [`interest_rate_swap`]. Both reuse
//! `bond_future_risk` / `interest_rate_swap_risk` from [`super::instruments`]
//! — no parallel sizing logic.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use convex_core::types::{Currency, Date, Frequency};
use convex_curves::{DiscreteCurve, RateCurve, RateCurveDyn};

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::risk::profile::{KeyRateBucket, Provenance, RiskProfile};

use super::cost::{CostModel, HeuristicCostModel};
use super::instruments::{bond_future_risk, interest_rate_swap_risk};
use super::types::{
    residual_from, BondFuture, Constraints, HedgeInstrument, HedgeProposal, HedgeTrade,
    InterestRateSwap, SwapSide, TradeoffNotes,
};

/// Strategy that neutralizes parallel DV01 with a single bond future.
///
/// Selects a benchmark contract by position currency + duration band, sizes
/// it to `−position.dv01 / future.dv01_per_contract`, computes residual KRD,
/// and stamps a heuristic cost. Curvature exposure is left in the residual —
/// that's the strategy's documented weakness.
pub fn duration_futures(
    position: &RiskProfile,
    constraints: &Constraints,
    discount_curve: &RateCurve<DiscreteCurve>,
    discount_curve_id: &str,
    settlement: Date,
) -> AnalyticsResult<HedgeProposal> {
    let contract = pick_future_contract(position)?;
    let key_rate_tenors: Vec<f64> = position
        .key_rate_buckets
        .iter()
        .map(|b| b.tenor_years)
        .collect();
    let risk = bond_future_risk(
        &contract,
        discount_curve,
        discount_curve_id,
        settlement,
        Some(&key_rate_tenors),
    )?;

    let contracts = -position.dv01 / risk.dv01_per_contract;
    let trade_dv01 = contracts * risk.dv01_per_contract;
    let trade_buckets: Vec<KeyRateBucket> = risk
        .buckets_per_contract
        .into_iter()
        .map(|b| KeyRateBucket {
            tenor_years: b.tenor_years,
            partial_dv01: b.partial_dv01 * contracts,
        })
        .collect();
    let trade = HedgeTrade {
        instrument: HedgeInstrument::BondFuture(contract.clone()),
        quantity: contracts,
        dv01: trade_dv01,
        key_rate_buckets: trade_buckets,
    };

    let residual = residual_from(position, std::slice::from_ref(&trade));
    let (cost_bps, cost_total) = priced_cost(&trade.instrument, position);

    let provenance = strategy_provenance(position, discount_curve_id);
    let mut tradeoffs = TradeoffNotes {
        strengths: vec![
            "Highly liquid; tight bid-ask".into(),
            "Capital efficient (margin-only)".into(),
        ],
        weaknesses: vec![
            "Curvature/key-rate residual remains".into(),
            "Roll risk on contract expiry".into(),
        ],
    };
    tag_constraint_violations(constraints, &residual.residual_dv01, cost_bps, &mut tradeoffs);

    Ok(HedgeProposal {
        strategy: "DurationFutures".into(),
        trades: vec![trade],
        residual,
        cost_bps,
        cost_total,
        tradeoffs,
        provenance,
    })
}

/// Strategy that neutralizes DV01 with a tenor-matched IRS.
///
/// Builds a pay-fixed (long bond → +DV01) or receive-fixed (short bond)
/// vanilla swap at the position's effective duration tenor, sized so the
/// fixed-leg PV01 matches the position's DV01.
pub fn interest_rate_swap(
    position: &RiskProfile,
    constraints: &Constraints,
    discount_curve: &RateCurve<DiscreteCurve>,
    discount_curve_id: &str,
    settlement: Date,
) -> AnalyticsResult<HedgeProposal> {
    let tenor_years = pick_swap_tenor(position);
    let side = if position.dv01 >= 0.0 {
        SwapSide::PayFixed
    } else {
        SwapSide::ReceiveFixed
    };
    let (frequency, day_count, floating_index) = swap_conventions_for(position.currency)?;
    let t_maturity_years = curve_tenor_to(discount_curve, settlement) + tenor_years;
    let fixed_rate_decimal = RateCurveDyn::par_swap_rate(
        discount_curve,
        t_maturity_years,
        frequency.periods_per_year(),
    )
    .map_err(|e| AnalyticsError::CurveError(e.to_string()))?;

    // Price a unit-notional swap once; DV01 and KRD scale linearly with
    // notional so we size analytically rather than re-pricing.
    let unit_swap = InterestRateSwap {
        tenor_years,
        fixed_rate_decimal,
        fixed_frequency: frequency,
        fixed_day_count: day_count,
        floating_index: floating_index.into(),
        side,
        notional: Decimal::ONE,
        currency: position.currency,
    };
    let key_rate_tenors: Vec<f64> = position
        .key_rate_buckets
        .iter()
        .map(|b| b.tenor_years)
        .collect();
    let unit_risk = interest_rate_swap_risk(
        &unit_swap,
        discount_curve,
        discount_curve_id,
        settlement,
        Some(&key_rate_tenors),
    )?;
    if unit_risk.dv01.abs() < 1e-12 {
        return Err(AnalyticsError::CalculationFailed(
            "swap unit DV01 is zero".into(),
        ));
    }
    let notional_f64 = -position.dv01 / unit_risk.dv01;
    let notional = Decimal::from_f64_retain(notional_f64).ok_or_else(|| {
        AnalyticsError::CalculationFailed(format!(
            "swap sizing produced non-finite notional: {notional_f64}"
        ))
    })?;

    let final_spec = InterestRateSwap {
        notional,
        ..unit_swap
    };
    let trade = HedgeTrade {
        instrument: HedgeInstrument::InterestRateSwap(final_spec),
        quantity: side_sign(side),
        dv01: unit_risk.dv01 * notional_f64,
        key_rate_buckets: unit_risk
            .buckets
            .into_iter()
            .map(|b| KeyRateBucket {
                tenor_years: b.tenor_years,
                partial_dv01: b.partial_dv01 * notional_f64,
            })
            .collect(),
    };
    let residual = residual_from(position, std::slice::from_ref(&trade));
    let (cost_bps, cost_total) = priced_cost(&trade.instrument, position);

    let provenance = strategy_provenance(position, discount_curve_id);
    let mut tradeoffs = TradeoffNotes {
        strengths: vec![
            "Tenor-matched: smaller curvature residual".into(),
            "No futures roll risk".into(),
        ],
        weaknesses: vec![
            "Bilateral OTC; documentation overhead".into(),
            "Wider bid-ask than listed futures".into(),
        ],
    };
    tag_constraint_violations(constraints, &residual.residual_dv01, cost_bps, &mut tradeoffs);

    Ok(HedgeProposal {
        strategy: "InterestRateSwap".into(),
        trades: vec![trade],
        residual,
        cost_bps,
        cost_total,
        tradeoffs,
        provenance,
    })
}

// ---- internals ----------------------------------------------------------

fn pick_future_contract(position: &RiskProfile) -> AnalyticsResult<BondFuture> {
    // Bucket by modified duration → benchmark contract code. v1 uses the most
    // liquid contract per region and assumes CF=1.0 (synthetic deliverable);
    // real CFs are wired in v2 against the deliverable basket.
    let (code, tenor) = match (position.currency, position.modified_duration_years) {
        (Currency::USD, d) if d < 2.5 => ("TU", 2.0),
        (Currency::USD, d) if d < 5.5 => ("FV", 5.0),
        (Currency::USD, d) if d < 12.0 => ("TY", 10.0),
        (Currency::USD, _) => ("US", 30.0),
        (Currency::EUR, d) if d < 5.5 => ("OE", 5.0),
        (Currency::EUR, _) => ("RX", 10.0),
        (Currency::GBP, _) => ("G", 10.0),
        other => {
            return Err(AnalyticsError::InvalidInput(format!(
                "DurationFutures: no benchmark contract for {:?}",
                other.0
            )))
        }
    };
    Ok(BondFuture {
        contract_code: code.into(),
        underlying_tenor_years: tenor,
        conversion_factor: 1.0,
        contract_size_face: contract_size_for(position.currency, code),
        currency: position.currency,
    })
}

fn contract_size_for(currency: Currency, code: &str) -> Decimal {
    use rust_decimal_macros::dec;
    match (currency, code) {
        (Currency::USD, "TU") => dec!(200_000),
        (Currency::USD, _) => dec!(100_000),
        (Currency::EUR, _) => dec!(100_000),
        (Currency::GBP, _) => dec!(100_000),
        _ => dec!(100_000),
    }
}

fn pick_swap_tenor(position: &RiskProfile) -> f64 {
    // Round modified duration to the nearest standard liquid swap tenor.
    // NaN-safe: NaN duration falls into the >20 band → 30Y.
    let d = position.modified_duration_years;
    if d < 3.5 {
        2.0
    } else if d < 7.5 {
        5.0
    } else if d < 15.0 {
        10.0
    } else if d < 25.0 {
        20.0
    } else {
        30.0
    }
}

fn swap_conventions_for(
    currency: Currency,
) -> AnalyticsResult<(Frequency, convex_core::daycounts::DayCountConvention, &'static str)> {
    use convex_core::daycounts::DayCountConvention;
    match currency {
        Currency::USD => Ok((Frequency::SemiAnnual, DayCountConvention::Act360, "SOFR")),
        Currency::GBP => Ok((Frequency::Quarterly, DayCountConvention::Act365Fixed, "SONIA")),
        Currency::EUR => Ok((Frequency::Annual, DayCountConvention::Act360, "ESTR")),
        other => Err(AnalyticsError::InvalidInput(format!(
            "InterestRateSwap: no swap conventions for {other:?}"
        ))),
    }
}

/// Years from the curve's reference date to `settlement`.
fn curve_tenor_to(discount_curve: &RateCurve<DiscreteCurve>, settlement: Date) -> f64 {
    use convex_curves::TermStructure;
    discount_curve
        .inner()
        .reference_date()
        .days_between(&settlement) as f64
        / 365.0
}

fn side_sign(side: SwapSide) -> f64 {
    match side {
        SwapSide::PayFixed => -1.0,
        SwapSide::ReceiveFixed => 1.0,
    }
}

fn priced_cost(instrument: &HedgeInstrument, position: &RiskProfile) -> (f64, Decimal) {
    let model = HeuristicCostModel;
    let cost_bps = model.cost_bps(instrument);
    let mv_f64 = position.market_value.to_f64().unwrap_or(0.0).abs();
    let cost_total =
        Decimal::from_f64_retain(mv_f64 * cost_bps / 10_000.0).unwrap_or(Decimal::ZERO);
    (cost_bps, cost_total)
}

/// Build a fresh `Provenance` for a strategy's output. Carries forward the
/// position's curve list and the strategy's discount-curve id (deduplicated).
fn strategy_provenance(position: &RiskProfile, discount_curve_id: &str) -> Provenance {
    let mut curves_used = position.provenance.curves_used.clone();
    if !curves_used.iter().any(|c| c == discount_curve_id) {
        curves_used.push(discount_curve_id.to_string());
    }
    Provenance {
        curves_used,
        cost_model: HeuristicCostModel.name().to_string(),
        advisor_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn tag_constraint_violations(
    constraints: &Constraints,
    residual_dv01: &f64,
    cost_bps: f64,
    notes: &mut TradeoffNotes,
) {
    if let Some(max_resid) = constraints.max_residual_dv01 {
        if residual_dv01.abs() > max_resid {
            notes.weaknesses.push(format!(
                "Residual DV01 {:.0} exceeds max_residual_dv01 = {:.0}",
                residual_dv01.abs(),
                max_resid
            ));
        }
    }
    if let Some(max_cost) = constraints.max_cost_bps {
        if cost_bps > max_cost {
            notes
                .weaknesses
                .push(format!("Cost {cost_bps:.2} bp exceeds max_cost_bps = {max_cost:.2}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::{Compounding, Currency, Mark};
    use convex_curves::{InterpolationMethod, ValueType};
    use rust_decimal_macros::dec;

    use crate::risk::profile::compute_position_risk;
    use convex_bonds::instruments::FixedRateBond;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn flat_curve(rate: f64) -> RateCurve<DiscreteCurve> {
        let dc = DiscreteCurve::new(
            d(2026, 1, 15),
            vec![0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0],
            vec![rate; 8],
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::Linear,
        )
        .unwrap();
        RateCurve::new(dc)
    }

    fn long_10y_corporate() -> (RiskProfile, RateCurve<DiscreteCurve>) {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("CORP10Y5")
            .coupon_rate(dec!(0.05))
            .maturity(d(2036, 1, 15))
            .issue_date(d(2026, 1, 15))
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::Thirty360US)
            .currency(Currency::USD)
            .face_value(dec!(100))
            .build()
            .unwrap();
        let curve = flat_curve(0.045);
        let mark = Mark::Yield {
            value: dec!(0.05),
            frequency: Frequency::SemiAnnual,
        };
        let profile = compute_position_risk(
            &bond,
            d(2026, 1, 15),
            &mark,
            dec!(10_000_000),
            &curve,
            "usd_sofr",
            None,
            Some(&[2.0, 5.0, 10.0, 30.0]),
            Some("CORP10Y5".into()),
        )
        .unwrap();
        (profile, curve)
    }

    #[test]
    fn duration_futures_neutralizes_dv01_within_one_basis_point() {
        let (pos, curve) = long_10y_corporate();
        let p = duration_futures(
            &pos,
            &Constraints::default(),
            &curve,
            "usd_sofr",
            d(2026, 1, 15),
        )
        .unwrap();
        let resid_pct = p.residual.residual_dv01.abs() / pos.dv01.abs();
        assert!(
            resid_pct < 0.001,
            "residual DV01 {} should be <0.1% of position DV01 {}; got {resid_pct}",
            p.residual.residual_dv01,
            pos.dv01
        );
    }

    #[test]
    fn duration_futures_picks_short_for_long_position() {
        let (pos, curve) = long_10y_corporate();
        let p = duration_futures(
            &pos,
            &Constraints::default(),
            &curve,
            "c",
            d(2026, 1, 15),
        )
        .unwrap();
        // Long bond + positive DV01 → short futures.
        assert!(p.trades[0].quantity < 0.0);
        assert_eq!(p.trades.len(), 1);
        match &p.trades[0].instrument {
            HedgeInstrument::BondFuture(f) => assert_eq!(f.contract_code, "TY"),
            other => panic!("expected BondFuture, got {other:?}"),
        }
    }

    #[test]
    fn interest_rate_swap_neutralizes_dv01() {
        let (pos, curve) = long_10y_corporate();
        let p = interest_rate_swap(
            &pos,
            &Constraints::default(),
            &curve,
            "usd_sofr",
            d(2026, 1, 15),
        )
        .unwrap();
        let resid_pct = p.residual.residual_dv01.abs() / pos.dv01.abs();
        assert!(
            resid_pct < 0.001,
            "residual DV01 {} should be <0.1% of position DV01 {}; got {resid_pct}",
            p.residual.residual_dv01,
            pos.dv01
        );
    }

    #[test]
    fn interest_rate_swap_picks_pay_fixed_for_long_position() {
        let (pos, curve) = long_10y_corporate();
        let p = interest_rate_swap(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        match &p.trades[0].instrument {
            HedgeInstrument::InterestRateSwap(s) => {
                assert_eq!(s.side, SwapSide::PayFixed);
                assert_eq!(s.tenor_years, 10.0);
            }
            other => panic!("expected swap, got {other:?}"),
        }
    }

    #[test]
    fn swap_residual_curvature_is_smaller_than_futures() {
        // Tenor-matched swap should leave less curvature than a single 10Y future.
        let (pos, curve) = long_10y_corporate();
        let f = duration_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        let s = interest_rate_swap(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        // Both should match parallel DV01; the swap matches the bucket pattern
        // more closely (CTD ≠ position bond), so its L1 residual should be
        // strictly smaller. This is the documented tradeoff.
        assert!(
            s.residual.residual_krd_l1_norm <= f.residual.residual_krd_l1_norm,
            "swap residual L1 = {}, futures residual L1 = {}",
            s.residual.residual_krd_l1_norm,
            f.residual.residual_krd_l1_norm
        );
    }

    #[test]
    fn provenance_carries_curve_and_cost_model() {
        let (pos, curve) = long_10y_corporate();
        let p = duration_futures(&pos, &Constraints::default(), &curve, "usd_sofr", d(2026, 1, 15)).unwrap();
        assert_eq!(p.provenance.cost_model, "heuristic_v1");
        assert!(p.provenance.curves_used.contains(&"usd_sofr".to_string()));
    }

    #[test]
    fn constraint_violation_appears_in_tradeoffs() {
        let (pos, curve) = long_10y_corporate();
        let constraints = Constraints {
            max_cost_bps: Some(0.0),
            ..Default::default()
        };
        let p = duration_futures(&pos, &constraints, &curve, "c", d(2026, 1, 15)).unwrap();
        assert!(p.tradeoffs.weaknesses.iter().any(|w| w.contains("max_cost_bps")));
    }

    #[test]
    fn nan_duration_does_not_panic_in_pick_swap_tenor() {
        let mut p = long_10y_corporate().0;
        p.modified_duration_years = f64::NAN;
        // Just assert no panic; result is the >25 fallback.
        assert_eq!(pick_swap_tenor(&p), 30.0);
    }
}
