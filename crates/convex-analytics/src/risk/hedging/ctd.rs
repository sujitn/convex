//! Cheapest-to-deliver (CTD) selection for bond futures. Net basis =
//! `(P_spot − F·CF) − Carry`; min net basis ≡ max implied repo ≡ CTD.
//! [`select_ctd`] is the public entry. [`approximate_cme_cf`] is the
//! flat-6% YTM clean-price formula — close to but not identical to the
//! exact CME formula (which rounds maturity to a contract-specific
//! quantum); supply `Deliverable.conversion_factor` directly when you
//! need the exchange's published number.

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
#[allow(missing_docs)]
pub struct Deliverable {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub coupon_rate_decimal: f64,
    pub maturity: Date,
    pub conversion_factor: f64,
}

/// Result of a CTD selection: which deliverable, the resulting net basis,
/// and the implied repo at that net basis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[allow(missing_docs)]
pub struct CtdSelection {
    pub index: usize,
    pub net_basis_per_100: f64,
    pub implied_repo_decimal: f64,
}

/// Build a cash bond from a deliverable spec for a given currency. Uses the
/// currency's sovereign preset (USD = UST, GBP = Gilt, EUR = Bund) to set
/// frequency and day count. The synthetic issue date is back-stepped from
/// maturity in coupon periods so the schedule is grid-aligned (no stub at
/// maturity).
pub(crate) fn deliverable_to_bond(
    deliverable: &Deliverable,
    currency: Currency,
    settlement: Date,
) -> AnalyticsResult<FixedRateBond> {
    if deliverable.maturity <= settlement {
        return Err(AnalyticsError::InvalidInput(format!(
            "Deliverable maturity ({}) must be after settlement ({})",
            deliverable.maturity, settlement
        )));
    }
    // from_f64_retain returns None for NaN/Inf, so it doubles as a finiteness check.
    let coupon = Decimal::from_f64_retain(deliverable.coupon_rate_decimal).ok_or_else(|| {
        AnalyticsError::InvalidInput(format!(
            "Deliverable coupon_rate_decimal not representable ({})",
            deliverable.coupon_rate_decimal
        ))
    })?;
    let issue_date = aligned_issue_date(deliverable.maturity, currency, settlement)?;
    let mut builder = FixedRateBond::builder()
        .identifiers(BondIdentifiers::new())
        .coupon_rate(coupon)
        .face_value(dec!(100))
        .maturity(deliverable.maturity)
        .issue_date(issue_date);
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

/// Step back from `maturity` in coupon periods until we land at or before
/// `settlement`. Anchors the synthetic schedule to the maturity grid so the
/// final coupon falls exactly on maturity (no stub).
fn aligned_issue_date(
    maturity: Date,
    currency: Currency,
    settlement: Date,
) -> AnalyticsResult<Date> {
    // USD UST + UK Gilt = SemiAnnual (6M); German Bund = Annual (12M).
    let period_months: i32 = match currency {
        Currency::USD | Currency::GBP => 6,
        Currency::EUR => 12,
        other => {
            return Err(AnalyticsError::InvalidInput(format!(
                "Deliverable: no coupon period for currency {other:?}"
            )))
        }
    };
    let mut anchor = maturity;
    while anchor > settlement {
        anchor = anchor
            .add_months(-period_months)
            .map_err(|e| AnalyticsError::InvalidInput(format!("issue date alignment: {e}")))?;
    }
    Ok(anchor)
}

/// Conversion factor from clean price of the deliverable at a flat 6% YTM,
/// divided by 100. Differs from the exact CME formula because that one
/// rounds the maturity-at-first-delivery to a contract-specific quantum
/// (3 months for TY/US, 1 month for TU/FV); supply an explicit
/// `Deliverable.conversion_factor` when you need CME-exact numbers.
pub(crate) fn approximate_cme_cf(
    deliverable: &Deliverable,
    currency: Currency,
    first_delivery: Date,
) -> AnalyticsResult<f64> {
    let bond = deliverable_to_bond(deliverable, currency, first_delivery)?;
    let clean = clean_price_from_yield(&bond, first_delivery, 0.06, bond.frequency())?;
    Ok(clean / 100.0)
}

/// Pick the cheapest-to-deliver from a basket. Prices the basket once,
/// uses `market_futures_price_per_100` when supplied or computes the no-arb
/// fair forward otherwise, then selects min net basis. Returns the
/// selection plus the futures price used.
pub fn select_ctd(
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
            "select_ctd: empty deliverable basket".into(),
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
    for (i, d) in basket.iter().enumerate() {
        if !(d.conversion_factor.is_finite() && d.conversion_factor > 0.0) {
            return Err(AnalyticsError::InvalidInput(format!(
                "Deliverable[{i}]: conversion_factor must be finite and > 0 (got {})",
                d.conversion_factor
            )));
        }
        if d.maturity <= delivery {
            return Err(AnalyticsError::InvalidInput(format!(
                "Deliverable[{i}]: maturity ({}) must be after delivery ({delivery})",
                d.maturity
            )));
        }
    }

    let t = settlement.days_between(&delivery) as f64 / 360.0;
    let carries: Vec<(f64, f64)> = basket
        .iter()
        .map(|d| spot_and_coupons(d, currency, curve, settlement, delivery))
        .collect::<AnalyticsResult<_>>()?;

    // Market F or no-arb fair forward. Setting NB=0 in the formula below
    // gives F = (spot − coupons + spot·r·T) / CF (only the spot purchase is
    // financed; coupons received between settle and delivery aren't).
    let f_used = market_futures_price_per_100.unwrap_or_else(|| {
        basket
            .iter()
            .zip(&carries)
            .map(|(d, &(spot, coupons))| {
                (spot - coupons + spot * repo_rate_decimal * t) / d.conversion_factor
            })
            .fold(f64::INFINITY, f64::min)
    });

    // Net basis = (spot − F·CF) − (coupons − spot·r·T). Min wins; implied repo
    // solves NB = 0 → r* = (coupons + F·CF − spot) / (spot · T).
    let sel = basket
        .iter()
        .zip(&carries)
        .enumerate()
        .map(|(i, (d, &(spot, coupons)))| {
            let carry = coupons - spot * repo_rate_decimal * t;
            let net_basis = (spot - f_used * d.conversion_factor) - carry;
            let implied_repo = if spot * t > 0.0 {
                (coupons + f_used * d.conversion_factor - spot) / (spot * t)
            } else {
                0.0
            };
            CtdSelection {
                index: i,
                net_basis_per_100: net_basis,
                implied_repo_decimal: implied_repo,
            }
        })
        .min_by(|a, b| {
            a.net_basis_per_100
                .partial_cmp(&b.net_basis_per_100)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("basket non-empty by earlier check");

    Ok((sel, f_used))
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
    // ZSpreadCalculator returns sentinel 0.0 on internal failure (perpetual,
    // expired bond, curve query error). Reject anything non-positive or
    // non-finite — silently treating it as a $0 spot would cascade into a
    // wrong fair forward and a wrong CTD pick.
    let spot = price_at_z_zero(&bond, curve, settlement);
    if !spot.is_finite() || spot <= 0.0 {
        return Err(AnalyticsError::CalculationFailed(format!(
            "Deliverable {:?} priced at spot={spot} (non-positive / non-finite); \
             curve query likely failed or bond matured before settlement",
            deliverable.name
        )));
    }
    // Coupons only — principal repayment isn't carry. We assume no
    // CouponAndPrincipal flows fall in the [settle, delivery] window;
    // `select_ctd` enforces `maturity > delivery` upstream, so the maturity
    // payment (which is typically merged) lands strictly after delivery.
    let coupons: f64 = bond
        .cash_flows(settlement)
        .into_iter()
        .filter(|cf| cf.date > settlement && cf.date <= delivery)
        .filter(|cf| matches!(cf.flow_type, convex_bonds::traits::CashFlowType::Coupon))
        .map(|cf| cf.amount.to_f64().unwrap_or(0.0))
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

    // ---- select_ctd ----------------------------

    #[test]
    fn select_ctd_at_no_arb_price_zeroes_net_basis_for_single_deliverable() {
        let curve = flat_curve(0.05);
        let basket = vec![deliverable(0.05, d(2036, 1, 15), 1.0)];
        let (sel, _f) = select_ctd(
            &basket,
            Currency::USD,
            &curve,
            d(2026, 1, 15),
            d(2026, 4, 15),
            0.04,
            None,
        )
        .unwrap();
        assert_eq!(sel.index, 0);
        assert!(sel.net_basis_per_100.abs() < 0.05);
        assert_relative_eq!(sel.implied_repo_decimal, 0.04, epsilon = 1e-3);
    }

    #[test]
    fn select_ctd_picks_minimum_net_basis_in_two_bond_basket() {
        let mat = d(2036, 1, 15);
        let settle = d(2026, 1, 15);
        let cf_4 = approximate_cme_cf(&deliverable(0.04, mat, 0.0), Currency::USD, settle).unwrap();
        let cf_8 = approximate_cme_cf(&deliverable(0.08, mat, 0.0), Currency::USD, settle).unwrap();
        let basket = vec![deliverable(0.04, mat, cf_4), deliverable(0.08, mat, cf_8)];
        let curve = flat_curve(0.05);
        let (sel, _f) = select_ctd(
            &basket,
            Currency::USD,
            &curve,
            settle,
            d(2026, 4, 15),
            0.04,
            None,
        )
        .unwrap();
        // At no-arb F (min implied forward), the chosen CTD has net basis ≈ 0
        // and selection lands on a real basket index.
        assert!(sel.index < basket.len());
        assert!(sel.net_basis_per_100.abs() < 0.05);
    }

    #[test]
    fn select_ctd_uses_market_price_when_supplied() {
        let mat = d(2036, 1, 15);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        let basket = vec![deliverable(0.05, mat, 1.0)];
        let curve = flat_curve(0.05);
        // Market F at 95 (rich futures): net basis at chosen CTD will be > 0.
        let (sel, f_used) = select_ctd(
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
    fn select_ctd_rejects_empty_basket_and_bad_inputs() {
        let curve = flat_curve(0.04);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        let basket = vec![deliverable(0.05, d(2036, 1, 15), 1.0)];

        // Empty basket.
        let err = select_ctd(&[], Currency::USD, &curve, settle, delivery, 0.04, None);
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));

        // Delivery before settlement.
        let err = select_ctd(&basket, Currency::USD, &curve, delivery, settle, 0.04, None);
        assert!(matches!(err, Err(AnalyticsError::InvalidInput(_))));

        // Non-finite or zero market futures price.
        for bad in [0.0, -100.0, f64::NAN, f64::INFINITY] {
            let err = select_ctd(
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
    fn select_ctd_rejects_deliverable_maturing_before_delivery() {
        // A bond maturing between settle and delivery is undeliverable;
        // pricing it would still produce numbers but they're nonsensical.
        let curve = flat_curve(0.04);
        let settle = d(2026, 1, 15);
        let delivery = d(2026, 4, 15);
        // Matures one month before delivery.
        let basket = vec![deliverable(0.05, d(2026, 3, 15), 1.0)];
        let err = select_ctd(&basket, Currency::USD, &curve, settle, delivery, 0.04, None);
        match err {
            Err(AnalyticsError::InvalidInput(msg)) => {
                assert!(msg.contains("maturity") && msg.contains("delivery"));
            }
            other => panic!("expected InvalidInput, got {other:?}"),
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
    fn deliverable_to_bond_aligns_issue_date_to_maturity_grid() {
        // USD/GBP step in 6-month chunks; EUR in 12-month chunks. Anchor must
        // be on the maturity grid and at or before settlement.
        let settle = d(2026, 1, 15);

        // USD: maturity 2036-04-15; 6M steps land on 04-15 / 10-15 dates.
        let usd_issue = aligned_issue_date(d(2036, 4, 15), Currency::USD, settle).unwrap();
        assert!(usd_issue <= settle);
        assert_eq!((usd_issue.month(), usd_issue.day()), (10, 15));

        // EUR: 12M steps; landing always on the maturity month/day.
        let eur_issue = aligned_issue_date(d(2036, 4, 15), Currency::EUR, settle).unwrap();
        assert!(eur_issue <= settle);
        assert_eq!((eur_issue.month(), eur_issue.day()), (4, 15));
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
