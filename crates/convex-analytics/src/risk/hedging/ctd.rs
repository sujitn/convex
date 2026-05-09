//! Cheapest-to-deliver (CTD) selection and conversion factors for bond futures.
//!
//! Implements the textbook delivery basket model:
//! - [`Deliverable`] is one bond eligible for delivery into a futures contract.
//! - [`approximate_cme_cf`] is a numerical approximation of the CME conversion
//!   factor: clean price of the deliverable at a flat 6% YTM, divided by 100.
//!   The exact CME formula rounds maturity to the nearest 3-month (TY/US) or
//!   1-month (TU/FV) chunk and accrues differently — this approximation gets
//!   within ~0.5% for typical deliverables. Pass an explicit
//!   `conversion_factor` on the [`Deliverable`] when CME-exact numbers matter.
//! - [`fair_futures_price`] derives a no-arb futures price from the basket as
//!   the lowest implied per-deliverable forward (the cheapest seller dictates
//!   the market futures level).
//! - [`select_ctd_by_net_basis`] picks the cheapest-to-deliver from a basket
//!   by minimizing net basis: NB = (P_spot − F × CF) − Carry, where
//!   Carry = (coupons collected between settle and delivery) − (financing
//!   cost on P_spot at the supplied repo rate). Min net basis ≡ max implied
//!   repo ≡ cheapest-to-deliver.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use convex_bonds::instruments::FixedRateBond;
use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_bonds::types::BondIdentifiers;
use convex_core::types::{Currency, Date};
use convex_curves::RateCurveDyn;

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::functions::clean_price_from_yield;
use crate::spreads::ZSpreadCalculator;

/// One bond eligible for delivery into a futures contract. Currency and the
/// frequency/day-count conventions come from the parent [`super::types::BondFuture`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Deliverable {
    /// Identifier (CUSIP/ISIN), optional — used for diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Coupon rate (decimal, `0.0475` = 4.75%).
    pub coupon_rate_decimal: f64,
    /// Maturity date.
    pub maturity: Date,
    /// CME conversion factor (≈1.0 for a 6% deliverable). Use
    /// [`approximate_cme_cf`] to compute, or supply the exchange's published
    /// value directly.
    pub conversion_factor: f64,
}

/// CTD selection result. `index` keys back into the basket vector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CtdSelection {
    /// Position into the basket of the chosen cheapest-to-deliver bond.
    pub index: usize,
    /// Net basis at the input futures price (per 100 face).
    pub net_basis_per_100: f64,
    /// Implied repo (decimal) for delivering this bond — the rate that makes
    /// the chosen deliverable's net basis zero.
    pub implied_repo_decimal: f64,
}

/// Build a cash bond from a deliverable spec for a given currency. Uses the
/// currency's sovereign preset (USD = UST, GBP = Gilt, EUR = Bund) to set
/// frequency and day count.
pub fn deliverable_to_bond(
    deliverable: &Deliverable,
    currency: Currency,
    settlement: Date,
) -> AnalyticsResult<FixedRateBond> {
    if !deliverable.coupon_rate_decimal.is_finite() {
        return Err(AnalyticsError::InvalidInput(format!(
            "Deliverable coupon_rate_decimal not finite ({})",
            deliverable.coupon_rate_decimal
        )));
    }
    if deliverable.maturity <= settlement {
        return Err(AnalyticsError::InvalidInput(format!(
            "Deliverable maturity ({}) must be after settlement ({})",
            deliverable.maturity, settlement
        )));
    }
    let coupon = Decimal::from_f64_retain(deliverable.coupon_rate_decimal).ok_or_else(|| {
        AnalyticsError::InvalidInput(format!(
            "Deliverable coupon_rate_decimal not representable ({})",
            deliverable.coupon_rate_decimal
        ))
    })?;
    let mut builder = FixedRateBond::builder()
        .identifiers(BondIdentifiers::new())
        .coupon_rate(coupon)
        .face_value(dec!(100))
        .maturity(deliverable.maturity)
        .issue_date(settlement);
    builder = match currency {
        Currency::USD => builder.us_treasury(),
        Currency::GBP => builder.uk_gilt(),
        Currency::EUR => builder.german_bund(),
        other => {
            return Err(AnalyticsError::InvalidInput(format!(
                "Deliverable: no sovereign preset for currency {other:?}"
            )))
        }
    };
    builder
        .build()
        .map_err(|e| AnalyticsError::BondError(format!("deliverable build: {e}")))
}

/// Numerical approximation of the CME conversion factor: clean price of the
/// deliverable at a flat 6% YTM, divided by 100. The exact CME formula
/// rounds the maturity-at-first-delivery to the nearest 3-month (TY/US) or
/// 1-month (TU/FV) chunk and accrues coupons differently; this approximation
/// is accurate to ~0.5% for typical deliverables. Supply an explicit
/// `conversion_factor` on [`Deliverable`] when CME-exact numbers matter.
pub fn approximate_cme_cf(
    deliverable: &Deliverable,
    currency: Currency,
    first_delivery: Date,
) -> AnalyticsResult<f64> {
    let bond = deliverable_to_bond(deliverable, currency, first_delivery)?;
    let clean = clean_price_from_yield(&bond, first_delivery, 0.06, bond.frequency())?;
    Ok(clean / 100.0)
}

/// Compute the no-arb fair futures price from a basket: the minimum of the
/// per-deliverable implied forward `F_i = (P_spot_i − coupons_i) × (1 + r·T) / CF_i`.
/// This is the futures level at which the cheapest deliverable's net basis
/// is zero — the standard textbook result.
pub fn fair_futures_price(
    basket: &[Deliverable],
    currency: Currency,
    curve: &dyn RateCurveDyn,
    settlement: Date,
    delivery: Date,
    repo_rate_decimal: f64,
) -> AnalyticsResult<f64> {
    if basket.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "fair_futures_price: empty deliverable basket".into(),
        ));
    }
    if !repo_rate_decimal.is_finite() {
        return Err(AnalyticsError::InvalidInput(format!(
            "repo rate not finite ({repo_rate_decimal})"
        )));
    }
    if delivery <= settlement {
        return Err(AnalyticsError::InvalidInput(format!(
            "delivery date ({delivery}) must be after settlement ({settlement})"
        )));
    }
    let t = settlement.days_between(&delivery) as f64 / 360.0;
    let mut min_f: Option<f64> = None;
    for (i, d) in basket.iter().enumerate() {
        let (spot, coupons) = spot_and_coupons(d, currency, curve, settlement, delivery)?;
        if !(d.conversion_factor.is_finite() && d.conversion_factor > 0.0) {
            return Err(AnalyticsError::InvalidInput(format!(
                "Deliverable[{i}]: conversion_factor must be finite and > 0 (got {})",
                d.conversion_factor
            )));
        }
        let f_i = (spot - coupons) * (1.0 + repo_rate_decimal * t) / d.conversion_factor;
        min_f = Some(match min_f {
            None => f_i,
            Some(prev) if f_i < prev => f_i,
            Some(prev) => prev,
        });
    }
    min_f.ok_or_else(|| AnalyticsError::CalculationFailed("fair_futures_price: no result".into()))
}

/// Pick the cheapest-to-deliver from a basket by minimizing net basis.
///
/// `futures_price_per_100` may be a market quote; pass [`fair_futures_price`]
/// when no live price is available.
pub fn select_ctd_by_net_basis(
    basket: &[Deliverable],
    currency: Currency,
    curve: &dyn RateCurveDyn,
    settlement: Date,
    delivery: Date,
    repo_rate_decimal: f64,
    futures_price_per_100: f64,
) -> AnalyticsResult<CtdSelection> {
    if basket.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "select_ctd_by_net_basis: empty deliverable basket".into(),
        ));
    }
    if !repo_rate_decimal.is_finite() {
        return Err(AnalyticsError::InvalidInput(format!(
            "repo rate not finite ({repo_rate_decimal})"
        )));
    }
    if !futures_price_per_100.is_finite() || futures_price_per_100 <= 0.0 {
        return Err(AnalyticsError::InvalidInput(format!(
            "futures price must be finite and strictly positive (got {futures_price_per_100})"
        )));
    }
    if delivery <= settlement {
        return Err(AnalyticsError::InvalidInput(format!(
            "delivery date ({delivery}) must be after settlement ({settlement})"
        )));
    }
    let t = settlement.days_between(&delivery) as f64 / 360.0;
    let mut best: Option<CtdSelection> = None;
    for (i, d) in basket.iter().enumerate() {
        if !(d.conversion_factor.is_finite() && d.conversion_factor > 0.0) {
            return Err(AnalyticsError::InvalidInput(format!(
                "Deliverable[{i}]: conversion_factor must be finite and > 0 (got {})",
                d.conversion_factor
            )));
        }
        let (spot, coupons) = spot_and_coupons(d, currency, curve, settlement, delivery)?;
        // Carry = coupons collected − financing cost on the spot purchase.
        // (Coupons are not re-financed for the residual period — small for
        // quarterly contracts; documented approximation.)
        let carry = coupons - spot * repo_rate_decimal * t;
        let net_basis = (spot - futures_price_per_100 * d.conversion_factor) - carry;
        // Implied repo solves NB = 0:
        //   r* = (coupons + F·CF − P_spot) / (P_spot · T)
        let implied_repo = if spot * t > 0.0 {
            (coupons + futures_price_per_100 * d.conversion_factor - spot) / (spot * t)
        } else {
            0.0
        };
        let candidate = CtdSelection {
            index: i,
            net_basis_per_100: net_basis,
            implied_repo_decimal: implied_repo,
        };
        match &best {
            None => best = Some(candidate),
            Some(prev) if candidate.net_basis_per_100 < prev.net_basis_per_100 => {
                best = Some(candidate);
            }
            _ => {}
        }
    }
    best.ok_or_else(|| AnalyticsError::CalculationFailed("CTD selection: no candidate".into()))
}

/// Combined CTD entry: prices the basket once, computes F (from input or
/// no-arb fair forward), selects min-net-basis CTD. Returns the selection
/// plus the futures price used. This is the path `bond_future_risk` takes
/// to avoid double-pricing the basket (one pass for `fair_futures_price`,
/// another for `select_ctd_by_net_basis`).
pub fn select_ctd_with_market_or_fair_price(
    basket: &[Deliverable],
    currency: Currency,
    curve: &dyn RateCurveDyn,
    settlement: Date,
    delivery: Date,
    repo_rate_decimal: f64,
    market_futures_price_per_100: Option<f64>,
) -> AnalyticsResult<(CtdSelection, f64)> {
    if basket.is_empty() {
        return Err(AnalyticsError::InvalidInput(
            "select_ctd_with_market_or_fair_price: empty deliverable basket".into(),
        ));
    }
    if !repo_rate_decimal.is_finite() {
        return Err(AnalyticsError::InvalidInput(format!(
            "repo rate not finite ({repo_rate_decimal})"
        )));
    }
    if delivery <= settlement {
        return Err(AnalyticsError::InvalidInput(format!(
            "delivery date ({delivery}) must be after settlement ({settlement})"
        )));
    }
    if let Some(f) = market_futures_price_per_100 {
        if !f.is_finite() || f <= 0.0 {
            return Err(AnalyticsError::InvalidInput(format!(
                "market futures price must be finite and > 0 (got {f})"
            )));
        }
    }
    let t = settlement.days_between(&delivery) as f64 / 360.0;

    // Price each deliverable once. Validate CFs while we're here.
    let mut carries: Vec<(f64, f64)> = Vec::with_capacity(basket.len());
    for (i, d) in basket.iter().enumerate() {
        if !(d.conversion_factor.is_finite() && d.conversion_factor > 0.0) {
            return Err(AnalyticsError::InvalidInput(format!(
                "Deliverable[{i}]: conversion_factor must be finite and > 0 (got {})",
                d.conversion_factor
            )));
        }
        carries.push(spot_and_coupons(d, currency, curve, settlement, delivery)?);
    }

    // Per-deliverable implied forward F_i = (spot − coupons) × (1+r·T) / CF.
    let f_used = match market_futures_price_per_100 {
        Some(f) => f,
        None => {
            let mut min_f: Option<f64> = None;
            for (i, d) in basket.iter().enumerate() {
                let (spot, coupons) = carries[i];
                let f_i = (spot - coupons) * (1.0 + repo_rate_decimal * t) / d.conversion_factor;
                min_f = Some(match min_f {
                    None => f_i,
                    Some(prev) if f_i < prev => f_i,
                    Some(prev) => prev,
                });
            }
            min_f.ok_or_else(|| {
                AnalyticsError::CalculationFailed("fair futures price: no result".into())
            })?
        }
    };

    // Pick min net basis at f_used.
    let mut best: Option<CtdSelection> = None;
    for (i, d) in basket.iter().enumerate() {
        let (spot, coupons) = carries[i];
        let carry = coupons - spot * repo_rate_decimal * t;
        let net_basis = (spot - f_used * d.conversion_factor) - carry;
        let implied_repo = if spot * t > 0.0 {
            (coupons + f_used * d.conversion_factor - spot) / (spot * t)
        } else {
            0.0
        };
        let candidate = CtdSelection {
            index: i,
            net_basis_per_100: net_basis,
            implied_repo_decimal: implied_repo,
        };
        match &best {
            None => best = Some(candidate),
            Some(prev) if candidate.net_basis_per_100 < prev.net_basis_per_100 => {
                best = Some(candidate);
            }
            _ => {}
        }
    }
    best.map(|sel| (sel, f_used))
        .ok_or_else(|| AnalyticsError::CalculationFailed("CTD selection: no candidate".into()))
}

/// (spot dirty per 100, coupons collected between settle and delivery).
fn spot_and_coupons(
    deliverable: &Deliverable,
    currency: Currency,
    curve: &dyn RateCurveDyn,
    settlement: Date,
    delivery: Date,
) -> AnalyticsResult<(f64, f64)> {
    let bond = deliverable_to_bond(deliverable, currency, settlement)?;
    let spot = price_at_z_zero(&bond, curve, settlement);
    let coupons: f64 = bond
        .cash_flows(settlement)
        .into_iter()
        .filter(|cf| cf.date > settlement && cf.date <= delivery)
        .map(|cf| {
            // The maturity cash flow includes principal — for repo carry we
            // only want the coupon component. Use coupon_rate × face / freq
            // as a precise per-period coupon; cash_flows kindly tag this.
            match cf.flow_type {
                convex_bonds::traits::CashFlowType::Coupon => cf.amount.to_f64().unwrap_or(0.0),
                _ => 0.0,
            }
        })
        .sum();
    Ok((spot, coupons))
}

fn price_at_z_zero<B: Bond + FixedCouponBond>(
    bond: &B,
    curve: &dyn RateCurveDyn,
    settlement: Date,
) -> f64 {
    ZSpreadCalculator::new(curve).price_with_spread(bond, 0.0, settlement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::Compounding;
    use convex_curves::{DiscreteCurve, InterpolationMethod, RateCurve, ValueType};

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

    fn deliverable(coupon: f64, maturity: Date, cf: f64) -> Deliverable {
        Deliverable {
            name: None,
            coupon_rate_decimal: coupon,
            maturity,
            conversion_factor: cf,
        }
    }

    // ---- approximate_cme_cf -----------------------------------------------

    #[test]
    fn cf_for_six_pct_coupon_is_unity() {
        // CME standardizes CF against a 6% bond → CF = 1.0 by construction
        // (within numerical tolerance of the approximation).
        let d10 = deliverable(0.06, d(2036, 1, 15), 1.0);
        let cf = approximate_cme_cf(&d10, Currency::USD, d(2026, 1, 15)).unwrap();
        assert_relative_eq!(cf, 1.0, epsilon = 0.01);
    }

    #[test]
    fn cf_for_below_six_pct_coupon_is_below_unity() {
        // A 4% 10Y at 6% standardized yield trades below par → CF < 1.0.
        let d10 = deliverable(0.04, d(2036, 1, 15), 0.0);
        let cf = approximate_cme_cf(&d10, Currency::USD, d(2026, 1, 15)).unwrap();
        assert!(
            cf > 0.7 && cf < 0.95,
            "CF for 4% 10Y = {cf}, expected ~0.85"
        );
    }

    #[test]
    fn cf_for_above_six_pct_coupon_is_above_unity() {
        let d10 = deliverable(0.08, d(2036, 1, 15), 0.0);
        let cf = approximate_cme_cf(&d10, Currency::USD, d(2026, 1, 15)).unwrap();
        assert!(
            cf > 1.05 && cf < 1.2,
            "CF for 8% 10Y = {cf}, expected ~1.15"
        );
    }

    // ---- fair_futures_price -----------------------------------------------

    #[test]
    fn fair_futures_price_for_single_deliverable_is_implied_forward() {
        // Single deliverable: fair F = (P_spot − coupons) × (1+rT) / CF.
        let bond = deliverable(0.05, d(2036, 1, 15), 1.0);
        let curve = flat_curve(0.05);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        let f = fair_futures_price(&[bond], Currency::USD, &curve, settle, delivery, 0.04).unwrap();
        // 5% par-ish bond → spot ~100; with 3 months at 4% repo: F ≈ 100 × (1+0.01) ≈ 101
        // less coupons collected (~zero in 3 months for a Jan-Jul payer at Jan settle).
        assert!(f > 99.0 && f < 103.0, "fair F = {f}, expected ~100-102");
    }

    #[test]
    fn fair_futures_price_uses_minimum_implied_forward() {
        // Two deliverables at different coupons; the lower-implied-forward bond
        // dictates F. The 4% sub-par bond (CF<1) typically has lower F when
        // computed against its own CF — verify the min is selected.
        let mat = d(2036, 1, 15);
        let cf_4 = approximate_cme_cf(&deliverable(0.04, mat, 0.0), Currency::USD, d(2026, 1, 15))
            .unwrap();
        let cf_8 = approximate_cme_cf(&deliverable(0.08, mat, 0.0), Currency::USD, d(2026, 1, 15))
            .unwrap();
        let basket = vec![deliverable(0.04, mat, cf_4), deliverable(0.08, mat, cf_8)];
        let curve = flat_curve(0.05);
        let f = fair_futures_price(
            &basket,
            Currency::USD,
            &curve,
            d(2026, 1, 15),
            d(2026, 4, 15),
            0.04,
        )
        .unwrap();
        // With coupon=YTM=5% the basket should be roughly indifferent;
        // numerical drift between the two bonds (CF approx + carry) should
        // keep F in a tight realistic range.
        assert!(f > 95.0 && f < 110.0, "fair F = {f}");
    }

    #[test]
    fn fair_futures_price_rejects_empty_basket() {
        let curve = flat_curve(0.04);
        let err = fair_futures_price(
            &[],
            Currency::USD,
            &curve,
            d(2026, 1, 15),
            d(2026, 4, 15),
            0.04,
        );
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    // ---- select_ctd_by_net_basis ------------------------------------------

    #[test]
    fn ctd_selection_single_deliverable_at_fair_price_has_zero_net_basis() {
        let curve = flat_curve(0.05);
        let basket = vec![deliverable(0.05, d(2036, 1, 15), 1.0)];
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        let f = fair_futures_price(&basket, Currency::USD, &curve, settle, delivery, 0.04).unwrap();
        let sel =
            select_ctd_by_net_basis(&basket, Currency::USD, &curve, settle, delivery, 0.04, f)
                .unwrap();
        assert_eq!(sel.index, 0);
        // At fair price, net basis ≈ 0 (within numerical tolerance).
        assert!(
            sel.net_basis_per_100.abs() < 0.05,
            "net basis at fair F = {}, expected ~0",
            sel.net_basis_per_100
        );
        // Implied repo ≈ input repo at fair price.
        assert_relative_eq!(sel.implied_repo_decimal, 0.04, epsilon = 1e-3);
    }

    #[test]
    fn ctd_selection_picks_minimum_net_basis_in_two_bond_basket() {
        // Construct a basket where the 4% bond is the cheaper deliverable and
        // verify it is chosen at the no-arb futures price.
        let mat = d(2036, 1, 15);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        let cf_4 = approximate_cme_cf(&deliverable(0.04, mat, 0.0), Currency::USD, settle).unwrap();
        let cf_8 = approximate_cme_cf(&deliverable(0.08, mat, 0.0), Currency::USD, settle).unwrap();
        let basket = vec![deliverable(0.04, mat, cf_4), deliverable(0.08, mat, cf_8)];
        let curve = flat_curve(0.05);
        let f = fair_futures_price(&basket, Currency::USD, &curve, settle, delivery, 0.04).unwrap();
        let sel =
            select_ctd_by_net_basis(&basket, Currency::USD, &curve, settle, delivery, 0.04, f)
                .unwrap();
        // The chosen bond's net basis should be the minimum of the two.
        let other_idx = if sel.index == 0 { 1 } else { 0 };
        let other = select_ctd_by_net_basis(
            &basket[other_idx..=other_idx],
            Currency::USD,
            &curve,
            settle,
            delivery,
            0.04,
            f,
        )
        .unwrap();
        assert!(
            sel.net_basis_per_100 <= other.net_basis_per_100 + 1e-9,
            "chosen NB {} should be <= other NB {}",
            sel.net_basis_per_100,
            other.net_basis_per_100,
        );
    }

    #[test]
    fn ctd_selection_rejects_empty_basket() {
        let curve = flat_curve(0.04);
        let err = select_ctd_by_net_basis(
            &[],
            Currency::USD,
            &curve,
            d(2026, 1, 15),
            d(2026, 4, 15),
            0.04,
            100.0,
        );
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn ctd_selection_rejects_delivery_before_settlement() {
        let curve = flat_curve(0.04);
        let basket = vec![deliverable(0.05, d(2036, 1, 15), 1.0)];
        let err = select_ctd_by_net_basis(
            &basket,
            Currency::USD,
            &curve,
            d(2026, 4, 15),
            d(2026, 1, 15), // before settle
            0.04,
            100.0,
        );
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn ctd_selection_rejects_zero_or_non_finite_cf() {
        let curve = flat_curve(0.04);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let basket = vec![deliverable(0.05, d(2036, 1, 15), bad)];
            let err = select_ctd_by_net_basis(
                &basket,
                Currency::USD,
                &curve,
                settle,
                delivery,
                0.04,
                100.0,
            );
            assert!(
                matches!(err, Err(AnalyticsError::InvalidInput(_))),
                "CF={bad} should be rejected"
            );
        }
    }

    // ---- select_ctd_with_market_or_fair_price ----------------------------

    #[test]
    fn combined_ctd_at_no_arb_price_matches_fair_plus_select() {
        // The combined entry should produce the same selection as
        // fair_futures_price → select_ctd_by_net_basis but in one basket
        // pass.
        let mat = d(2036, 1, 15);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        let cf_4 = approximate_cme_cf(&deliverable(0.04, mat, 0.0), Currency::USD, settle).unwrap();
        let cf_8 = approximate_cme_cf(&deliverable(0.08, mat, 0.0), Currency::USD, settle).unwrap();
        let basket = vec![deliverable(0.04, mat, cf_4), deliverable(0.08, mat, cf_8)];
        let curve = flat_curve(0.05);

        let f = fair_futures_price(&basket, Currency::USD, &curve, settle, delivery, 0.04).unwrap();
        let two_pass =
            select_ctd_by_net_basis(&basket, Currency::USD, &curve, settle, delivery, 0.04, f)
                .unwrap();

        let (one_pass, f_combined) = select_ctd_with_market_or_fair_price(
            &basket,
            Currency::USD,
            &curve,
            settle,
            delivery,
            0.04,
            None,
        )
        .unwrap();
        assert_eq!(one_pass.index, two_pass.index);
        assert!(
            (f_combined - f).abs() < 1e-9,
            "F mismatch: {f_combined} vs {f}"
        );
        assert!((one_pass.net_basis_per_100 - two_pass.net_basis_per_100).abs() < 1e-9);
    }

    #[test]
    fn combined_ctd_uses_market_price_when_supplied() {
        let mat = d(2036, 1, 15);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        let basket = vec![deliverable(0.05, mat, 1.0)];
        let curve = flat_curve(0.05);
        // Market F at 95 (rich futures): net basis at chosen CTD will be > 0.
        let (sel, f_used) = select_ctd_with_market_or_fair_price(
            &basket,
            Currency::USD,
            &curve,
            settle,
            delivery,
            0.04,
            Some(95.0),
        )
        .unwrap();
        assert_eq!(sel.index, 0);
        assert!((f_used - 95.0).abs() < 1e-9);
        assert!(
            sel.net_basis_per_100 > 0.0,
            "with futures at 95 (below fair), net basis should be > 0; got {}",
            sel.net_basis_per_100
        );
    }

    #[test]
    fn combined_ctd_single_deliverable_fast_path_returns_zero_net_basis_at_no_arb() {
        // Single-deliverable basket at no-arb F: selection.index = 0,
        // net_basis ≈ 0, implied_repo ≈ input repo.
        let mat = d(2036, 1, 15);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        let basket = vec![deliverable(0.05, mat, 1.0)];
        let curve = flat_curve(0.05);
        let (sel, _f) = select_ctd_with_market_or_fair_price(
            &basket,
            Currency::USD,
            &curve,
            settle,
            delivery,
            0.04,
            None,
        )
        .unwrap();
        assert_eq!(sel.index, 0);
        assert!(sel.net_basis_per_100.abs() < 0.05);
        assert_relative_eq!(sel.implied_repo_decimal, 0.04, epsilon = 1e-3);
    }

    #[test]
    fn combined_ctd_rejects_empty_basket_and_bad_inputs() {
        let curve = flat_curve(0.04);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        let basket = vec![deliverable(0.05, d(2036, 1, 15), 1.0)];

        // Empty basket.
        let err = select_ctd_with_market_or_fair_price(
            &[],
            Currency::USD,
            &curve,
            settle,
            delivery,
            0.04,
            None,
        );
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));

        // Delivery before settlement.
        let err = select_ctd_with_market_or_fair_price(
            &basket,
            Currency::USD,
            &curve,
            delivery,
            settle,
            0.04,
            None,
        );
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));

        // Non-finite or zero market futures price.
        for bad in [0.0, -100.0, f64::NAN, f64::INFINITY] {
            let err = select_ctd_with_market_or_fair_price(
                &basket,
                Currency::USD,
                &curve,
                settle,
                delivery,
                0.04,
                Some(bad),
            );
            assert!(
                matches!(err, Err(AnalyticsError::InvalidInput(_))),
                "futures_price={bad} should be rejected"
            );
        }
    }

    #[test]
    fn deliverable_to_bond_supports_us_gilt_bund() {
        let mat = d(2036, 1, 15);
        let settle = d(2026, 1, 15);
        for currency in [Currency::USD, Currency::GBP, Currency::EUR] {
            let bond = deliverable_to_bond(&deliverable(0.04, mat, 1.0), currency, settle).unwrap();
            assert_eq!(bond.currency(), currency);
            assert_eq!(bond.maturity(), Some(mat));
        }
    }

    #[test]
    fn deliverable_to_bond_rejects_unsupported_currency() {
        let err = deliverable_to_bond(
            &deliverable(0.04, d(2036, 1, 15), 1.0),
            Currency::JPY,
            d(2026, 1, 15),
        );
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
    }

    #[test]
    fn deliverable_to_bond_rejects_maturity_at_or_before_settlement() {
        let settle = d(2026, 1, 15);
        for mat in [settle, d(2025, 12, 15)] {
            let err = deliverable_to_bond(&deliverable(0.04, mat, 1.0), Currency::USD, settle);
            assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));
        }
    }
}
