//! Concrete hedge strategies. Each function takes a [`RiskProfile`] and
//! returns a [`HedgeProposal`] sized to neutralize at least the position's
//! parallel DV01.
//!
//! Ships [`duration_futures`] (single contract), [`barbell_futures`] (two
//! contracts solving for parallel + dominant-bucket KRD), and
//! [`interest_rate_swap`] (tenor-matched IRS). All three reuse
//! `bond_future_risk` / `interest_rate_swap_risk` from [`super::instruments`]
//! — no parallel sizing logic.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use convex_core::types::{Currency, Date, Frequency};
use convex_curves::{DiscreteCurve, RateCurve, RateCurveDyn};

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::risk::profile::{KeyRateBucket, Provenance, RiskProfile};

use super::cost::{CostModel, HeuristicCostModel};
use super::instruments::{
    bond_future_risk, cash_bond_risk, interest_rate_swap_risk, BondFutureRisk,
};
use super::types::{
    residual_from, BondFuture, CashBondLeg, Constraints, HedgeInstrument, HedgeProposal,
    HedgeTrade, InterestRateSwap, SwapSide, TradeoffNotes,
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
    let (cost_bps, cost_total) = proposal_cost(std::slice::from_ref(&trade), position);

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
    tag_constraint_violations(
        constraints,
        &residual.residual_dv01,
        cost_bps,
        &mut tradeoffs,
    );

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

/// Strategy that uses two bond futures (a barbell pair) to neutralize
/// parallel DV01 and the position's dominant key-rate bucket simultaneously.
///
/// Useful for positions whose curvature is concentrated at a tenor that no
/// single benchmark contract matches well — e.g., a 7-year position hedged
/// with FV+TY rather than TY alone, or a 20-year position with TY+US.
///
/// Math: solve a 2x2 system where contract A and contract B's per-contract
/// (DV01, KRD-at-target) zero out (-position.dv01, -position.krd_at_target)
/// via Cramer's rule. Singular system (proportional risk profiles) → error.
pub fn barbell_futures(
    position: &RiskProfile,
    constraints: &Constraints,
    discount_curve: &RateCurve<DiscreteCurve>,
    discount_curve_id: &str,
    settlement: Date,
) -> AnalyticsResult<HedgeProposal> {
    if position.key_rate_buckets.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "BarbellFutures requires a key-rate ladder on the position".into(),
        ));
    }

    // 1. Dominant target bucket — largest |partial_dv01| in the ladder.
    let target = position
        .key_rate_buckets
        .iter()
        .max_by(|a, b| {
            a.partial_dv01
                .abs()
                .partial_cmp(&b.partial_dv01.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .expect("non-empty by check above");

    // 2. Pick a bracketing contract pair for the position's currency + duration.
    let (lo, hi) = pick_barbell_pair(position)?;

    // 3. Per-contract risks on the position's tenor ladder.
    let key_rate_tenors: Vec<f64> = position
        .key_rate_buckets
        .iter()
        .map(|b| b.tenor_years)
        .collect();
    let lo_risk = bond_future_risk(
        &lo,
        discount_curve,
        discount_curve_id,
        settlement,
        Some(&key_rate_tenors),
    )?;
    let hi_risk = bond_future_risk(
        &hi,
        discount_curve,
        discount_curve_id,
        settlement,
        Some(&key_rate_tenors),
    )?;

    // 4. Per-contract KRD at the target tenor.
    let lo_k = krd_at(&lo_risk.buckets_per_contract, target.tenor_years);
    let hi_k = krd_at(&hi_risk.buckets_per_contract, target.tenor_years);
    let lo_d = lo_risk.dv01_per_contract;
    let hi_d = hi_risk.dv01_per_contract;

    // 5. Cramer's rule on:
    //    n_lo * lo_d + n_hi * hi_d = −position.dv01
    //    n_lo * lo_k + n_hi * hi_k = −target.partial_dv01
    let det = lo_d * hi_k - hi_d * lo_k;
    if det.abs() < 1e-9 {
        return Err(AnalyticsError::CalculationFailed(format!(
            "BarbellFutures: contract pair {} + {} has near-singular risk matrix at {}Y",
            lo.contract_code, hi.contract_code, target.tenor_years
        )));
    }
    let d = position.dv01;
    let k = target.partial_dv01;
    let n_lo = (hi_d * k - d * hi_k) / det;
    let n_hi = (d * lo_k - lo_d * k) / det;

    // 6. Build the two trade legs.
    let trade_lo = scale_future_trade(&lo, &lo_risk, n_lo);
    let trade_hi = scale_future_trade(&hi, &hi_risk, n_hi);
    let trades = vec![trade_lo, trade_hi];
    let residual = residual_from(position, &trades);
    let (cost_bps, cost_total) = proposal_cost(&trades, position);

    let provenance = strategy_provenance(position, discount_curve_id);
    let mut tradeoffs = TradeoffNotes {
        strengths: vec![
            format!(
                "Two-leg barbell ({} + {}); neutralizes {}Y key-rate as well as parallel DV01",
                lo.contract_code, hi.contract_code, target.tenor_years
            ),
            "Smaller curvature residual than a single-tenor hedge when KRD is spread".into(),
        ],
        weaknesses: vec![
            "Two contracts → twice the bid-ask, twice the roll".into(),
            "Picks one target bucket; off-target buckets are not pinned".into(),
        ],
    };
    tag_constraint_violations(
        constraints,
        &residual.residual_dv01,
        cost_bps,
        &mut tradeoffs,
    );

    Ok(HedgeProposal {
        strategy: "BarbellFutures".into(),
        trades,
        residual,
        cost_bps,
        cost_total,
        tradeoffs,
        provenance,
    })
}

/// Pick a contract pair that brackets the position's effective duration.
/// Returns `(short-tenor contract, long-tenor contract)`.
fn pick_barbell_pair(position: &RiskProfile) -> AnalyticsResult<(BondFuture, BondFuture)> {
    // Bracket the position's modified duration with the two adjacent
    // benchmark contracts. Tenors line up at 2 / 5 / 10 / 30; the breakpoints
    // (5 and 10) put each duration into the pair whose underlying tenors
    // span it.
    let pair = match (position.currency, position.modified_duration_years) {
        (Currency::USD, d) if d <= 5.0 => (("TU", 2.0), ("FV", 5.0)),
        (Currency::USD, d) if d <= 10.0 => (("FV", 5.0), ("TY", 10.0)),
        (Currency::USD, _) => (("TY", 10.0), ("US", 30.0)),
        (Currency::EUR, _) => (("OE", 5.0), ("RX", 10.0)),
        other => {
            return Err(AnalyticsError::InvalidInput(format!(
                "BarbellFutures: no contract pair for {:?}",
                other.0
            )))
        }
    };
    Ok((
        BondFuture {
            contract_code: pair.0 .0.into(),
            underlying_tenor_years: pair.0 .1,
            conversion_factor: 1.0,
            contract_size_face: contract_size_for(position.currency, pair.0 .0),
            currency: position.currency,
        },
        BondFuture {
            contract_code: pair.1 .0.into(),
            underlying_tenor_years: pair.1 .1,
            conversion_factor: 1.0,
            contract_size_face: contract_size_for(position.currency, pair.1 .0),
            currency: position.currency,
        },
    ))
}

/// Per-bucket DV01 lookup at a tenor (within 1e-9 tolerance). Zero if absent.
fn krd_at(buckets: &[KeyRateBucket], tenor_years: f64) -> f64 {
    buckets
        .iter()
        .find(|b| (b.tenor_years - tenor_years).abs() < 1e-9)
        .map(|b| b.partial_dv01)
        .unwrap_or(0.0)
}

/// Build a `HedgeTrade` for a futures contract scaled by `quantity`.
fn scale_future_trade(spec: &BondFuture, risk: &BondFutureRisk, quantity: f64) -> HedgeTrade {
    HedgeTrade {
        instrument: HedgeInstrument::BondFuture(spec.clone()),
        quantity,
        dv01: risk.dv01_per_contract * quantity,
        key_rate_buckets: risk
            .buckets_per_contract
            .iter()
            .map(|b| KeyRateBucket {
                tenor_years: b.tenor_years,
                partial_dv01: b.partial_dv01 * quantity,
            })
            .collect(),
    }
}

/// Strategy that neutralizes DV01 by shorting a duration-matched on-the-run
/// sovereign bond (UST / Bund / Gilt). Most-asked-for hedge on real desks
/// when the trader wants to keep curve exposure but neutralize rates risk
/// without futures roll or bilateral swap docs.
///
/// Sizes the cash-bond face to `−position.dv01 / cash_bond_dv01_per_unit_face`
/// after probing risk on a unit face, then re-pricing with the final face for
/// the trade record.
pub fn cash_bond_pair(
    position: &RiskProfile,
    constraints: &Constraints,
    discount_curve: &RateCurve<DiscreteCurve>,
    discount_curve_id: &str,
    settlement: Date,
) -> AnalyticsResult<HedgeProposal> {
    let tenor_years = pick_swap_tenor(position);
    let coupon = otr_par_coupon(discount_curve, settlement, tenor_years)?;

    // Probe with a unit face to learn DV01 per unit face; scale linearly.
    let unit_leg = CashBondLeg {
        tenor_years,
        coupon_rate_decimal: coupon,
        currency: position.currency,
        face_amount: Decimal::ONE,
    };
    let key_rate_tenors: Vec<f64> = position
        .key_rate_buckets
        .iter()
        .map(|b| b.tenor_years)
        .collect();
    let unit_risk = cash_bond_risk(
        &unit_leg,
        discount_curve,
        discount_curve_id,
        settlement,
        Some(&key_rate_tenors),
    )?;
    if unit_risk.dv01.abs() < 1e-12 {
        return Err(AnalyticsError::CalculationFailed(
            "cash bond unit DV01 is zero".into(),
        ));
    }
    let face_f64 = -position.dv01 / unit_risk.dv01;
    let face_amount = Decimal::from_f64_retain(face_f64).ok_or_else(|| {
        AnalyticsError::CalculationFailed(format!(
            "cash bond sizing produced non-finite face: {face_f64}"
        ))
    })?;

    let final_leg = CashBondLeg {
        face_amount,
        ..unit_leg
    };
    let trade = HedgeTrade {
        instrument: HedgeInstrument::CashBond(final_leg),
        quantity: face_f64.signum(),
        dv01: unit_risk.dv01 * face_f64,
        key_rate_buckets: unit_risk
            .buckets
            .into_iter()
            .map(|b| KeyRateBucket {
                tenor_years: b.tenor_years,
                partial_dv01: b.partial_dv01 * face_f64,
            })
            .collect(),
    };
    let residual = residual_from(position, std::slice::from_ref(&trade));
    let (cost_bps, cost_total) = proposal_cost(std::slice::from_ref(&trade), position);

    let provenance = strategy_provenance(position, discount_curve_id);
    let mut tradeoffs = TradeoffNotes {
        strengths: vec![
            "Cash bond — no roll, no margin, no CTD basis".into(),
            "Tenor-matched on-the-run; tracks curve cleanly".into(),
        ],
        weaknesses: vec![
            "Wider bid-ask than a listed future".into(),
            "Funding cost (repo) on the short leg; not modelled in v1".into(),
        ],
    };
    tag_constraint_violations(
        constraints,
        &residual.residual_dv01,
        cost_bps,
        &mut tradeoffs,
    );

    Ok(HedgeProposal {
        strategy: "CashBondPair".into(),
        trades: vec![trade],
        residual,
        cost_bps,
        cost_total,
        tradeoffs,
        provenance,
    })
}

/// Par coupon for a synthetic on-the-run sovereign at `tenor_years`. Reads
/// the par swap rate against the discount curve as a proxy — fine for
/// near-flat curves and good enough for the Z-flat pricing the strategy
/// does next.
fn otr_par_coupon(
    discount_curve: &RateCurve<DiscreteCurve>,
    settlement: Date,
    tenor_years: f64,
) -> AnalyticsResult<f64> {
    let t_maturity = curve_tenor_to(discount_curve, settlement) + tenor_years;
    RateCurveDyn::par_swap_rate(discount_curve, t_maturity, 2)
        .map_err(|e| AnalyticsError::CurveError(e.to_string()))
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
    let (cost_bps, cost_total) = proposal_cost(std::slice::from_ref(&trade), position);

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
    tag_constraint_violations(
        constraints,
        &residual.residual_dv01,
        cost_bps,
        &mut tradeoffs,
    );

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
) -> AnalyticsResult<(
    Frequency,
    convex_core::daycounts::DayCountConvention,
    &'static str,
)> {
    // Post-LIBOR OIS fixed-leg conventions (ISDA 2021 Definitions): annual
    // fixed payments. Day count is Act/360 for USD/EUR, Act/365F for GBP.
    use convex_core::daycounts::DayCountConvention;
    match currency {
        Currency::USD => Ok((Frequency::Annual, DayCountConvention::Act360, "SOFR")),
        Currency::GBP => Ok((Frequency::Annual, DayCountConvention::Act365Fixed, "SONIA")),
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

/// Unified cost calculation across single-leg and multi-leg proposals.
///
/// Cost in currency = Σ over legs of (leg notional × `HeuristicCostModel`
/// bps / 10_000). For futures, leg notional = `|quantity| × contract size`;
/// for swaps, the notional comes from the spec. The reported `cost_bps` is
/// the total dollar cost expressed as bps of position market value, so
/// proposals are comparable on a per-position basis even when their hedge
/// notionals differ.
fn proposal_cost(trades: &[HedgeTrade], position: &RiskProfile) -> (f64, Decimal) {
    let model = HeuristicCostModel;
    let mv = position.market_value.to_f64().unwrap_or(0.0).abs();
    let mut total = 0.0_f64;
    for trade in trades {
        let leg_notional = match &trade.instrument {
            HedgeInstrument::BondFuture(spec) => {
                trade.quantity.abs() * spec.contract_size_face.to_f64().unwrap_or(0.0)
            }
            HedgeInstrument::InterestRateSwap(spec) => spec.notional.to_f64().unwrap_or(0.0).abs(),
            HedgeInstrument::CashBond(spec) => spec.face_amount.to_f64().unwrap_or(0.0).abs(),
        };
        total += leg_notional * model.cost_bps(&trade.instrument) / 10_000.0;
    }
    let cost_bps = if mv > 1e-9 {
        total / mv * 10_000.0
    } else {
        0.0
    };
    (
        cost_bps,
        Decimal::from_f64_retain(total).unwrap_or(Decimal::ZERO),
    )
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
            notes.weaknesses.push(format!(
                "Cost {cost_bps:.2} bp exceeds max_cost_bps = {max_cost:.2}"
            ));
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
        let p =
            duration_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
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
        let p =
            interest_rate_swap(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        match &p.trades[0].instrument {
            HedgeInstrument::InterestRateSwap(s) => {
                assert_eq!(s.side, SwapSide::PayFixed);
                assert_eq!(s.tenor_years, 10.0);
            }
            other => panic!("expected swap, got {other:?}"),
        }
    }

    #[test]
    fn both_strategies_leave_comparable_curvature_residual() {
        // Both proposals neutralize parallel DV01 but neither perfectly
        // matches the position bond's cashflow shape, so each leaves some
        // curvature residual. With ISDA-correct USD SOFR conventions
        // (Annual fixed leg vs the SA position bond) the order between the
        // two residuals depends on the position's day-count / frequency, so
        // we only assert both are non-trivial and within an order of
        // magnitude of each other.
        let (pos, curve) = long_10y_corporate();
        let f =
            duration_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        let s =
            interest_rate_swap(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        let fl1 = f.residual.residual_krd_l1_norm;
        let sl1 = s.residual.residual_krd_l1_norm;
        assert!(fl1 > 0.0 && sl1 > 0.0, "expected both residuals nonzero");
        let ratio = (fl1 / sl1).max(sl1 / fl1);
        assert!(
            ratio < 10.0,
            "residuals diverged: futures L1 = {fl1}, swap L1 = {sl1}"
        );
    }

    #[test]
    fn provenance_carries_curve_and_cost_model() {
        let (pos, curve) = long_10y_corporate();
        let p = duration_futures(
            &pos,
            &Constraints::default(),
            &curve,
            "usd_sofr",
            d(2026, 1, 15),
        )
        .unwrap();
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
        assert!(p
            .tradeoffs
            .weaknesses
            .iter()
            .any(|w| w.contains("max_cost_bps")));
    }

    #[test]
    fn barbell_futures_neutralizes_dv01_and_target_bucket() {
        let (pos, curve) = long_10y_corporate();
        let p =
            barbell_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        // Parallel DV01 neutralised within 0.1%.
        let resid_pct = p.residual.residual_dv01.abs() / pos.dv01.abs();
        assert!(
            resid_pct < 0.001,
            "barbell residual DV01 = {} on position DV01 {} ({:.3} %)",
            p.residual.residual_dv01,
            pos.dv01,
            resid_pct * 100.0
        );
        // Two legs.
        assert_eq!(p.trades.len(), 2);
        // Both legs are bond futures.
        for trade in &p.trades {
            assert!(matches!(trade.instrument, HedgeInstrument::BondFuture(_)));
        }
        // 10Y bullet → bracket pair is FV (5Y) + TY (10Y).
        let codes: Vec<&str> = p
            .trades
            .iter()
            .filter_map(|t| match &t.instrument {
                HedgeInstrument::BondFuture(f) => Some(f.contract_code.as_str()),
                _ => None,
            })
            .collect();
        assert!(codes.contains(&"FV"));
        assert!(codes.contains(&"TY"));
        // Dominant 10Y bucket is targeted → its residual is near zero.
        let ten_y = p
            .residual
            .residual_buckets
            .iter()
            .find(|b| (b.tenor_years - 10.0).abs() < 1e-9)
            .unwrap();
        assert!(
            ten_y.partial_dv01.abs() < 1.0,
            "10Y residual should be ~0 (target bucket); got {}",
            ten_y.partial_dv01
        );
    }

    #[test]
    fn barbell_futures_errors_on_empty_ladder() {
        let (mut pos, curve) = long_10y_corporate();
        pos.key_rate_buckets.clear();
        let err = barbell_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15));
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn cash_bond_pair_neutralizes_dv01() {
        let (pos, curve) = long_10y_corporate();
        let p = cash_bond_pair(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        let resid_pct = p.residual.residual_dv01.abs() / pos.dv01.abs();
        assert!(
            resid_pct < 0.001,
            "residual DV01 {} should be <0.1% of position DV01 {}; got {resid_pct}",
            p.residual.residual_dv01,
            pos.dv01
        );
        assert_eq!(p.trades.len(), 1);
        match &p.trades[0].instrument {
            HedgeInstrument::CashBond(c) => {
                assert_eq!(c.currency, Currency::USD);
                assert_eq!(c.tenor_years, 10.0);
                // Long bond hedged by SHORT cash bond → negative face_amount.
                let face = c.face_amount.to_f64().unwrap_or(0.0);
                assert!(
                    face < 0.0,
                    "long bond → short cash bond hedge; got face {face}"
                );
            }
            other => panic!("expected CashBond, got {other:?}"),
        }
    }

    #[test]
    fn cash_bond_pair_provenance_carries_curve_and_cost_model() {
        let (pos, curve) = long_10y_corporate();
        let p = cash_bond_pair(
            &pos,
            &Constraints::default(),
            &curve,
            "usd_sofr",
            d(2026, 1, 15),
        )
        .unwrap();
        assert_eq!(p.provenance.cost_model, "heuristic_v1");
        assert!(p.provenance.curves_used.contains(&"usd_sofr".to_string()));
    }

    #[test]
    fn nan_duration_does_not_panic_in_pick_swap_tenor() {
        let mut p = long_10y_corporate().0;
        p.modified_duration_years = f64::NAN;
        // Just assert no panic; result is the >25 fallback.
        assert_eq!(pick_swap_tenor(&p), 30.0);
    }
}
