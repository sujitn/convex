//! Hedge strategies. Each function sizes legs to neutralize the position's
//! parallel DV01 and returns a [`HedgeProposal`].

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use convex_core::types::{Compounding, Currency, Date, Frequency};
use convex_curves::{DiscreteCurve, RateCurve, RateCurveDyn};

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::risk::profile::{KeyRateBucket, Provenance, RiskProfile};

use super::cost::{cost_bps, COST_MODEL_NAME};
use super::ctd::{approximate_cme_cf, Deliverable};
use super::instruments::{
    bond_future_risk, cash_bond_risk, interest_rate_swap_risk, BondFutureRisk,
};
use super::types::{
    residual_from, BondFuture, CashBondLeg, Constraints, HedgeInstrument, HedgeProposal,
    HedgeTrade, InterestRateSwap, SwapSide, TradeoffNotes,
};

/// Single benchmark bond future, sized to neutralize parallel DV01. Leaves
/// curvature in the residual (that's the named weakness).
pub fn duration_futures(
    position: &RiskProfile,
    constraints: &Constraints,
    discount_curve: &RateCurve<DiscreteCurve>,
    discount_curve_id: &str,
    settlement: Date,
) -> AnalyticsResult<HedgeProposal> {
    let contract = pick_future_contract(position, discount_curve, settlement)?;
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
    tag_constraint_violations(constraints, &residual, cost_bps, &mut tradeoffs);

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

/// Two bond futures bracketing the position's duration. Solves a 2x2 for
/// parallel DV01 and the dominant key-rate bucket simultaneously, so a 7Y
/// position is hedged with FV+TY rather than just TY. Errors if the two
/// contracts' (DV01, KRD-at-target) are colinear.
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

    let (lo, hi) = pick_barbell_pair(position, discount_curve, settlement)?;
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

    // Cramer on:  n_lo·lo_d + n_hi·hi_d = −position.dv01
    //             n_lo·lo_k + n_hi·hi_k = −target.partial_dv01
    let lo_k = krd_at(&lo_risk.buckets_per_contract, target.tenor_years);
    let hi_k = krd_at(&hi_risk.buckets_per_contract, target.tenor_years);
    let lo_d = lo_risk.dv01_per_contract;
    let hi_d = hi_risk.dv01_per_contract;
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
    tag_constraint_violations(constraints, &residual, cost_bps, &mut tradeoffs);

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

/// Two benchmark futures bracketing the position's modified duration. Returns
/// `(short-tenor, long-tenor)` so callers can rely on the order in `det` math.
fn pick_barbell_pair(
    position: &RiskProfile,
    discount_curve: &RateCurve<DiscreteCurve>,
    settlement: Date,
) -> AnalyticsResult<(BondFuture, BondFuture)> {
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
        make_default_future(
            pair.0 .0,
            pair.0 .1,
            position.currency,
            discount_curve,
            settlement,
        )?,
        make_default_future(
            pair.1 .0,
            pair.1 .1,
            position.currency,
            discount_curve,
            settlement,
        )?,
    ))
}

fn krd_at(buckets: &[KeyRateBucket], tenor_years: f64) -> f64 {
    buckets
        .iter()
        .find(|b| (b.tenor_years - tenor_years).abs() < 1e-9)
        .map(|b| b.partial_dv01)
        .unwrap_or(0.0)
}

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

/// Short a duration-matched OTR sovereign sized to neutralize DV01. Coupon
/// is read off the discount curve's par-swap rate (real govt curve deferred).
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
    tag_constraint_violations(constraints, &residual, cost_bps, &mut tradeoffs);

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

fn otr_par_coupon(
    discount_curve: &RateCurve<DiscreteCurve>,
    settlement: Date,
    tenor_years: f64,
) -> AnalyticsResult<f64> {
    let t_maturity = curve_tenor_to(discount_curve, settlement) + tenor_years;
    RateCurveDyn::par_swap_rate(discount_curve, t_maturity, 2)
        .map_err(|e| AnalyticsError::CurveError(e.to_string()))
}

/// Tenor-matched vanilla IRS sized to neutralize DV01. Pay-fixed for long
/// positions, receive-fixed for short.
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

    // Risk scales linearly with notional, so price a unit swap and scale.
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
    tag_constraint_violations(constraints, &residual, cost_bps, &mut tradeoffs);

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

/// N-leg key-rate hedge: solve an N×N system to neutralize each of the
/// position's KRD buckets simultaneously, not just parallel DV01.
///
/// For currency `c` we pick a benchmark ladder of N futures spanning the
/// curve (USD: TU/FV/TY/US, EUR: SCH/OE/RX/BUXL). Let `K_ij = krd_j(t_i)`
/// be the j-th future's partial DV01 at the i-th tenor and `b_i = -position.krd(t_i)`.
/// We solve `K · n = b` via LU; `n_j` is the contract count for future j.
/// The parallel-DV01 constraint is implicit (sum of `K_ij` over `i` ≈ `dv01_j`
/// for each leg).
///
/// Errors if the position has fewer KRD tenors than ladder legs (under-determined),
/// or if the system is near-singular.
pub fn key_rate_futures(
    position: &RiskProfile,
    constraints: &Constraints,
    discount_curve: &RateCurve<DiscreteCurve>,
    discount_curve_id: &str,
    settlement: Date,
) -> AnalyticsResult<HedgeProposal> {
    let ladder = pick_key_rate_ladder(position.currency, discount_curve, settlement)?;
    let n = ladder.len();

    if position.key_rate_buckets.len() < n {
        return Err(AnalyticsError::InvalidInput(format!(
            "KeyRateFutures requires at least {n} KRD buckets on the position (got {}); \
             pass `key_rate_tenors` covering the ladder",
            position.key_rate_buckets.len()
        )));
    }

    // Pick the N largest |KRD| tenors as targets so the algorithm zeroes the
    // most material exposures even when the ladder is wider than the position.
    let target_tenors = pick_target_tenors(position, n);
    let target_buckets: Vec<KeyRateBucket> = target_tenors
        .iter()
        .map(|&t| KeyRateBucket {
            tenor_years: t,
            partial_dv01: krd_at(&position.key_rate_buckets, t),
        })
        .collect();

    // Build each future's KRD profile against the same target tenors.
    let mut leg_risks: Vec<BondFutureRisk> = Vec::with_capacity(n);
    for spec in &ladder {
        leg_risks.push(bond_future_risk(
            spec,
            discount_curve,
            discount_curve_id,
            settlement,
            Some(&target_tenors),
        )?);
    }

    let contracts = solve_key_rate_system(&leg_risks, &target_buckets, &ladder)?;

    let trades: Vec<HedgeTrade> = ladder
        .iter()
        .zip(leg_risks.iter())
        .zip(contracts.iter())
        .map(|((spec, risk), &qty)| scale_future_trade(spec, risk, qty))
        .collect();

    let residual = residual_from(position, &trades);
    let (cost_bps, cost_total) = proposal_cost(&trades, position);
    let provenance = strategy_provenance(position, discount_curve_id);

    let leg_codes: Vec<&str> = ladder.iter().map(|f| f.contract_code.as_str()).collect();
    let mut tradeoffs = TradeoffNotes {
        strengths: vec![
            format!(
                "{n}-leg ladder ({}) — solves N×N to pin every targeted KRD",
                leg_codes.join("+")
            ),
            "Smallest curvature residual of the futures-only strategies".into(),
        ],
        weaknesses: vec![
            format!("{n} contracts → {n}× the bid-ask and roll cost"),
            "Sensitive to the ladder's KRD matrix conditioning; may swing big with tiny tenor moves".into(),
        ],
    };
    tag_constraint_violations(constraints, &residual, cost_bps, &mut tradeoffs);

    Ok(HedgeProposal {
        strategy: "KeyRateFutures".into(),
        trades,
        residual,
        cost_bps,
        cost_total,
        tradeoffs,
        provenance,
    })
}

/// Currency-specific liquid futures ladder.
fn pick_key_rate_ladder(
    currency: Currency,
    discount_curve: &RateCurve<DiscreteCurve>,
    settlement: Date,
) -> AnalyticsResult<Vec<BondFuture>> {
    let pairs: &[(&str, f64)] = match currency {
        Currency::USD => &[("TU", 2.0), ("FV", 5.0), ("TY", 10.0), ("US", 30.0)],
        Currency::EUR => &[("SCH", 2.0), ("OE", 5.0), ("RX", 10.0), ("BUXL", 30.0)],
        other => {
            return Err(AnalyticsError::InvalidInput(format!(
                "KeyRateFutures: no key-rate ladder defined for {other:?}"
            )))
        }
    };
    pairs
        .iter()
        .map(|(code, tenor)| {
            make_default_future(code, *tenor, currency, discount_curve, settlement)
        })
        .collect()
}

/// Pick N tenors closest to the ladder while preferring the position's
/// largest |KRD| buckets — keeps the algorithm aligned with the most material
/// exposures even when the position has more buckets than ladder legs.
fn pick_target_tenors(position: &RiskProfile, n: usize) -> Vec<f64> {
    let mut buckets = position.key_rate_buckets.clone();
    buckets.sort_by(|a, b| {
        b.partial_dv01
            .abs()
            .partial_cmp(&a.partial_dv01.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut tenors: Vec<f64> = buckets.iter().take(n).map(|b| b.tenor_years).collect();
    tenors.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    tenors
}

/// Solve `K · n = -k_pos` where `K[i,j] = leg_risks[j].krd(t_i)` and
/// `k_pos[i] = position.krd(t_i)`. Returns `n` (one contract count per leg).
fn solve_key_rate_system(
    leg_risks: &[BondFutureRisk],
    target_buckets: &[KeyRateBucket],
    ladder: &[BondFuture],
) -> AnalyticsResult<Vec<f64>> {
    use convex_math::linear_algebra::solve_linear_system;
    use nalgebra::{DMatrix, DVector};

    let n = leg_risks.len();
    let mut matrix = DMatrix::<f64>::zeros(n, n);
    let mut rhs = DVector::<f64>::zeros(n);
    for (i, target) in target_buckets.iter().enumerate() {
        for (j, leg) in leg_risks.iter().enumerate() {
            matrix[(i, j)] = krd_at(&leg.buckets_per_contract, target.tenor_years);
        }
        rhs[i] = -target.partial_dv01;
    }
    let solution = solve_linear_system(&matrix, &rhs).map_err(|e| {
        let codes: Vec<&str> = ladder.iter().map(|f| f.contract_code.as_str()).collect();
        AnalyticsError::CalculationFailed(format!(
            "KeyRateFutures: {n}×{n} system over ({}) is singular: {e}",
            codes.join(", ")
        ))
    })?;
    Ok(solution.iter().copied().collect())
}

// ---- internals ----------------------------------------------------------

fn pick_future_contract(
    position: &RiskProfile,
    discount_curve: &RateCurve<DiscreteCurve>,
    settlement: Date,
) -> AnalyticsResult<BondFuture> {
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
    make_default_future(code, tenor, position.currency, discount_curve, settlement)
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

/// Repo proxy: 3-month simple-compounded zero rate on the discount curve.
/// Used as the financing rate for net-basis CTD carry. The curve must
/// quote at 0.25y — propagates the curve error otherwise rather than
/// silently substituting a hardcoded constant.
fn repo_rate(curve: &RateCurve<DiscreteCurve>) -> AnalyticsResult<f64> {
    curve
        .zero_rate_at_tenor(0.25, Compounding::Simple)
        .map_err(|e| AnalyticsError::CurveError(format!("repo proxy at 3M: {e}")))
}

/// Builds a [`BondFuture`] with one synthetic at-par deliverable. CME
/// convention is "remaining maturity at first delivery", so the deliverable
/// matures `underlying_tenor_years` after the first delivery date (not after
/// settlement). Coupon is the par-swap rate at the underlying tenor; CF is
/// from [`approximate_cme_cf`]; repo from the curve front-end; first
/// delivery 3 months out. Callers with real basket data should construct
/// [`BondFuture`] with their own `deliverable_basket`.
fn make_default_future(
    contract_code: &str,
    underlying_tenor_years: f64,
    currency: Currency,
    discount_curve: &RateCurve<DiscreteCurve>,
    settlement: Date,
) -> AnalyticsResult<BondFuture> {
    const DELIVERY_MONTHS: u32 = 3;
    let tenor_months = (underlying_tenor_years * 12.0).round() as i32;
    if tenor_months <= 0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "make_default_future({contract_code}): underlying tenor must round to \
             >0 months (got {underlying_tenor_years})"
        )));
    }
    let first_delivery = settlement
        .add_months(DELIVERY_MONTHS as i32)
        .map_err(|e| AnalyticsError::InvalidInput(format!("first delivery: {e}")))?;
    let maturity = first_delivery
        .add_months(tenor_months)
        .map_err(|e| AnalyticsError::InvalidInput(format!("future deliverable maturity: {e}")))?;
    let coupon = otr_par_coupon(discount_curve, settlement, underlying_tenor_years)?;
    let mut deliverable = Deliverable {
        name: Some(format!("{contract_code}_{underlying_tenor_years:.1}Y")),
        coupon_rate_decimal: coupon,
        maturity,
        conversion_factor: 1.0,
    };
    deliverable.conversion_factor = approximate_cme_cf(&deliverable, currency, first_delivery)?;
    Ok(BondFuture {
        contract_code: contract_code.into(),
        underlying_tenor_years,
        deliverable_basket: vec![deliverable],
        delivery_months: DELIVERY_MONTHS,
        repo_rate_decimal: repo_rate(discount_curve)?,
        futures_price: None,
        contract_size_face: contract_size_for(currency, contract_code),
        currency,
    })
}

fn pick_swap_tenor(position: &RiskProfile) -> f64 {
    // Bucket modified duration into standard liquid swap tenors. NaN falls
    // through every comparison and lands in the final branch (30Y).
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

fn proposal_cost(trades: &[HedgeTrade], position: &RiskProfile) -> (f64, Decimal) {
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
        total += leg_notional * cost_bps(&trade.instrument) / 10_000.0;
    }
    let cost_bps_total = if mv > 1e-9 {
        total / mv * 10_000.0
    } else {
        0.0
    };
    (
        cost_bps_total,
        Decimal::from_f64_retain(total).unwrap_or(Decimal::ZERO),
    )
}

fn strategy_provenance(position: &RiskProfile, discount_curve_id: &str) -> Provenance {
    let mut curves_used = position.provenance.curves_used.clone();
    if !curves_used.iter().any(|c| c == discount_curve_id) {
        curves_used.push(discount_curve_id.to_string());
    }
    Provenance {
        curves_used,
        cost_model: COST_MODEL_NAME.to_string(),
        advisor_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn tag_constraint_violations(
    constraints: &Constraints,
    residual: &super::types::ResidualRisk,
    cost_bps: f64,
    notes: &mut TradeoffNotes,
) {
    if let Some(max_resid) = constraints.max_residual_dv01 {
        if residual.residual_dv01.abs() > max_resid {
            notes.weaknesses.push(format!(
                "Residual DV01 {:.0} exceeds max_residual_dv01 = {:.0}",
                residual.residual_dv01.abs(),
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
    for limit in &constraints.max_residual_per_bucket {
        if let Some(bucket) = residual
            .residual_buckets
            .iter()
            .find(|b| (b.tenor_years - limit.tenor_years).abs() < 1e-9)
        {
            if bucket.partial_dv01.abs() > limit.max_abs_dv01 {
                notes.weaknesses.push(format!(
                    "{:.1}Y bucket residual {:.0} exceeds max {:.0}",
                    limit.tenor_years,
                    bucket.partial_dv01.abs(),
                    limit.max_abs_dv01,
                ));
            }
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

    // ---- KeyRateFutures ---------------------------------------------------

    #[test]
    fn key_rate_futures_pins_every_target_bucket() {
        let (pos, curve) = long_10y_corporate();
        let p =
            key_rate_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        // 4-leg ladder.
        assert_eq!(p.trades.len(), 4);
        let codes: Vec<&str> = p
            .trades
            .iter()
            .filter_map(|t| match &t.instrument {
                HedgeInstrument::BondFuture(f) => Some(f.contract_code.as_str()),
                _ => None,
            })
            .collect();
        for code in ["TU", "FV", "TY", "US"] {
            assert!(codes.contains(&code), "missing leg {code} in {codes:?}");
        }
        // Every targeted KRD bucket is pinned within ~0.5% of position DV01.
        let tol = pos.dv01.abs() * 0.005;
        for tenor in [2.0, 5.0, 10.0, 30.0] {
            let r = p
                .residual
                .residual_buckets
                .iter()
                .find(|b| (b.tenor_years - tenor).abs() < 1e-9)
                .unwrap_or_else(|| panic!("missing residual bucket {tenor}Y"));
            assert!(
                r.partial_dv01.abs() < tol,
                "residual at {tenor}Y = {} should be < {tol}",
                r.partial_dv01,
            );
        }
    }

    #[test]
    fn key_rate_futures_residual_dv01_is_bounded() {
        // KRF pins each KRD bucket exactly; residual parallel DV01 is the
        // KRD-vs-parallel-DV01 leakage from triangular bumps not summing
        // to a true parallel shift. A few percent on a 10Y position.
        let (pos, curve) = long_10y_corporate();
        let p =
            key_rate_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        let resid_pct = p.residual.residual_dv01.abs() / pos.dv01.abs();
        assert!(
            resid_pct < 0.05,
            "residual DV01 = {} should be <5% of position DV01 {}; got {resid_pct:.4}",
            p.residual.residual_dv01,
            pos.dv01,
        );
    }

    #[test]
    fn key_rate_futures_l1_residual_smaller_than_duration_futures() {
        // Pinning every key tenor should leave a smaller residual KRD L1 norm
        // than a single-tenor parallel hedge — that's the whole point.
        let (pos, curve) = long_10y_corporate();
        let krf =
            key_rate_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        let df =
            duration_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15)).unwrap();
        assert!(
            krf.residual.residual_krd_l1_norm < df.residual.residual_krd_l1_norm,
            "key-rate residual L1 ({}) should be < duration-futures residual L1 ({})",
            krf.residual.residual_krd_l1_norm,
            df.residual.residual_krd_l1_norm,
        );
    }

    #[test]
    fn key_rate_futures_errors_when_position_has_too_few_buckets() {
        let (mut pos, curve) = long_10y_corporate();
        // Strip down to two buckets — under the 4-leg ladder.
        pos.key_rate_buckets.truncate(2);
        let err = key_rate_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15));
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn key_rate_futures_errors_for_unsupported_currency() {
        let (mut pos, curve) = long_10y_corporate();
        pos.currency = Currency::JPY;
        let err = key_rate_futures(&pos, &Constraints::default(), &curve, "c", d(2026, 1, 15));
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn key_rate_futures_provenance_carries_curve_and_cost_model() {
        let (pos, curve) = long_10y_corporate();
        let p = key_rate_futures(
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

    // ---- make_default_future (curve-driven coupon) -----------------------

    #[test]
    fn default_future_coupon_tracks_par_swap_rate_at_underlying_tenor() {
        let curve = flat_curve(0.03);
        let f = make_default_future("TY", 10.0, Currency::USD, &curve, d(2026, 1, 15)).unwrap();
        let coupon = f.deliverable_basket[0].coupon_rate_decimal;
        // par_swap_rate on a flat curve equals the zero rate; tolerate small
        // compounding/day-count drift.
        assert!(
            (coupon - 0.03).abs() < 0.005,
            "coupon = {coupon}, expected ~0.03"
        );
    }

    #[test]
    fn default_future_coupon_shifts_with_curve_level() {
        // Same contract on 3% vs 6% curves should produce materially
        // different coupons — the previous hardcoded behavior was flat at
        // 4.5% USD regardless of curve.
        let lo = make_default_future("TY", 10.0, Currency::USD, &flat_curve(0.03), d(2026, 1, 15))
            .unwrap()
            .deliverable_basket[0]
            .coupon_rate_decimal;
        let hi = make_default_future("TY", 10.0, Currency::USD, &flat_curve(0.06), d(2026, 1, 15))
            .unwrap()
            .deliverable_basket[0]
            .coupon_rate_decimal;
        assert!(
            hi - lo > 0.025,
            "high-rate coupon ({hi}) should exceed low-rate coupon ({lo}) by ≥ 250bps"
        );
    }

    // ---- KeyRateBucketLimit ----------------------------------------------

    #[test]
    fn duration_futures_tags_bucket_violation_when_too_loose() {
        // DurationFutures leaves curvature on; a tight per-bucket constraint
        // at off-target tenors should produce a weakness tag.
        let (pos, curve) = long_10y_corporate();
        let constraints = Constraints {
            // 10Y bucket is the dominant exposure; setting a tiny limit at
            // 2Y/5Y/30Y should flag the residuals at those off-target tenors.
            max_residual_per_bucket: vec![
                super::super::types::KeyRateBucketLimit {
                    tenor_years: 2.0,
                    max_abs_dv01: 1.0,
                },
                super::super::types::KeyRateBucketLimit {
                    tenor_years: 30.0,
                    max_abs_dv01: 1.0,
                },
            ],
            ..Default::default()
        };
        let p = duration_futures(&pos, &constraints, &curve, "c", d(2026, 1, 15)).unwrap();
        let weakness_text = p.tradeoffs.weaknesses.join(" | ");
        assert!(
            weakness_text.contains("bucket residual") && weakness_text.contains("exceeds max"),
            "expected bucket-violation tag in weaknesses, got: {weakness_text}"
        );
    }

    #[test]
    fn duration_futures_no_tag_when_constraint_satisfied() {
        // Set the per-bucket limits high enough that no real residual ever
        // breaches; no bucket-violation weakness should appear.
        let (pos, curve) = long_10y_corporate();
        let constraints = Constraints {
            max_residual_per_bucket: vec![super::super::types::KeyRateBucketLimit {
                tenor_years: 2.0,
                max_abs_dv01: 1e9,
            }],
            ..Default::default()
        };
        let p = duration_futures(&pos, &constraints, &curve, "c", d(2026, 1, 15)).unwrap();
        let weakness_text = p.tradeoffs.weaknesses.join(" | ");
        assert!(
            !weakness_text.contains("bucket residual"),
            "unexpected bucket-violation tag, got: {weakness_text}"
        );
    }

    #[test]
    fn constraints_default_max_residual_per_bucket_is_empty_when_omitted() {
        // Wire-compat: old payloads without max_residual_per_bucket should
        // still parse, with the new field defaulting to an empty list.
        let c: Constraints = serde_json::from_str(r#"{"max_residual_dv01": 1000}"#).unwrap();
        assert_eq!(c.max_residual_dv01, Some(1000.0));
        assert!(c.max_residual_per_bucket.is_empty());
    }

    #[test]
    fn default_future_repo_tracks_curve_front_end() {
        // 3% flat curve → repo ≈ 3% (3M zero rate, simple compounding).
        let curve = flat_curve(0.03);
        let f = make_default_future("TY", 10.0, Currency::USD, &curve, d(2026, 1, 15)).unwrap();
        assert!(
            (f.repo_rate_decimal - 0.03).abs() < 0.005,
            "repo on 3% curve = {}, expected ~0.03",
            f.repo_rate_decimal
        );
    }

    #[test]
    fn default_future_repo_shifts_with_curve_level() {
        // Same contract on 2% vs 6% curves → repo shifts ~400bps.
        let lo = make_default_future("TY", 10.0, Currency::USD, &flat_curve(0.02), d(2026, 1, 15))
            .unwrap()
            .repo_rate_decimal;
        let hi = make_default_future("TY", 10.0, Currency::USD, &flat_curve(0.06), d(2026, 1, 15))
            .unwrap()
            .repo_rate_decimal;
        assert!(
            hi - lo > 0.03,
            "high-rate repo ({hi}) should exceed low-rate repo ({lo}) by ≥ 300bps"
        );
    }

    #[test]
    fn ctd_selection_on_steep_curve_picks_a_deliverable() {
        // Build a steeply upward-sloping curve and a hand-built TY-style
        // basket with two deliverables of materially different coupons. CTD
        // selection should successfully pick one of them (basket-skew
        // exercises). Index need not always be ≠ 0; the point is that the
        // CTD machinery completes a real selection rather than degenerating.
        use crate::risk::hedging::ctd::{select_ctd, Deliverable};
        let tenors = vec![0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0];
        let rates = vec![0.02, 0.022, 0.025, 0.03, 0.04, 0.05, 0.058, 0.06];
        let dc = DiscreteCurve::new(
            d(2026, 1, 15),
            tenors,
            rates,
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::Linear,
        )
        .unwrap();
        let curve = RateCurve::new(dc);
        let basket = vec![
            Deliverable {
                name: Some("TY_low_cpn".into()),
                coupon_rate_decimal: 0.04,
                maturity: d(2034, 1, 15),
                conversion_factor: 0.875,
            },
            Deliverable {
                name: Some("TY_high_cpn".into()),
                coupon_rate_decimal: 0.06,
                maturity: d(2036, 1, 15),
                conversion_factor: 1.0,
            },
        ];
        let (sel, _f_used) = select_ctd(
            &basket,
            Currency::USD,
            &curve,
            d(2026, 1, 15),
            d(2026, 4, 15),
            0.022,
            None,
        )
        .unwrap();
        assert!(sel.index < 2);
        // At fair F (no-arb), the chosen CTD's net basis is ≈0 and the
        // implied repo equals the input repo — sanity-check the math.
        assert!(sel.net_basis_per_100.abs() < 0.05);
        assert!((sel.implied_repo_decimal - 0.022).abs() < 1e-3);
    }

    #[test]
    fn default_future_at_six_pct_curve_has_unit_conversion_factor() {
        // CME standardized CF = clean price at 6% YTM ÷ 100. When the curve
        // (and therefore the par-swap-rate coupon) is ≈6%, the deliverable
        // trades near par and CF ≈ 1.0. par_swap_rate uses the curve's
        // discount factors with semi-annual frequency — slightly different
        // compounding from the 6% YTM used inside approximate_cme_cf — so a
        // 1.5% tolerance accommodates that drift.
        let curve = flat_curve(0.06);
        let f = make_default_future("TY", 10.0, Currency::USD, &curve, d(2026, 1, 15)).unwrap();
        let cf = f.deliverable_basket[0].conversion_factor;
        assert!(
            (cf - 1.0).abs() < 0.015,
            "CF on 6% curve = {cf}, expected ≈ 1.0 (within 1.5%)"
        );
    }
}
