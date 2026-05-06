//! Mark-driven bond pricing.
//!
//! `price_from_mark` accepts a trader [`Mark`] (price, yield, or spread) and
//! returns the canonical bond quote: clean, dirty, accrued, derived YTM, and
//! — when the mark itself was a spread — the spread in basis points.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use convex_bonds::traits::{Bond, FixedCouponBond};
use convex_core::types::{Date, Frequency, Mark, PriceKind, SpreadType};
use convex_curves::RateCurveDyn;

use crate::error::{AnalyticsError, AnalyticsResult};
use crate::functions::{dirty_price_from_yield, yield_to_maturity};
use crate::spreads::ZSpreadCalculator;

/// Output of `price_from_mark`. Prices and accrued are per 100 face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricingResult {
    /// Clean price per 100.
    pub clean_price_per_100: f64,
    /// Dirty price per 100 (clean + accrued).
    pub dirty_price_per_100: f64,
    /// Accrued interest per 100.
    pub accrued_per_100: f64,
    /// Yield to maturity as decimal (0.05 = 5%).
    pub ytm_decimal: f64,
    /// Z-spread in bps. Some only when the input mark was a spread.
    pub z_spread_bps: Option<f64>,
}

fn dec_to_f64(d: Decimal, field: &str) -> AnalyticsResult<f64> {
    d.to_f64()
        .ok_or_else(|| AnalyticsError::InvalidInput(format!("{field}: non-finite decimal")))
}

fn f64_to_dec(x: f64, field: &str) -> AnalyticsResult<Decimal> {
    Decimal::from_f64_retain(x)
        .ok_or_else(|| AnalyticsError::InvalidInput(format!("{field}: non-finite f64")))
}

/// Price a bond against a trader [`Mark`] and return the canonical result.
///
/// `curve` is required for spread marks. `quote_frequency` is the compounding
/// frequency used when deriving YTM from a price/spread mark.
pub fn price_from_mark<B>(
    bond: &B,
    settlement: Date,
    mark: &Mark,
    curve: Option<&dyn RateCurveDyn>,
    quote_frequency: Frequency,
) -> AnalyticsResult<PricingResult>
where
    B: Bond + FixedCouponBond,
{
    let maturity = bond
        .maturity()
        .ok_or_else(|| AnalyticsError::InvalidInput("bond has no maturity (perpetual)".into()))?;
    if settlement >= maturity {
        return Err(AnalyticsError::InvalidSettlement {
            settlement: settlement.to_string(),
            maturity: maturity.to_string(),
        });
    }

    let accrued = dec_to_f64(bond.accrued_interest(settlement), "accrued")?;

    // Reduce every variant to a dirty price; everything else is derived from it.
    let (dirty, z_spread_bps) = match mark {
        Mark::Price { value, kind } => {
            let v = dec_to_f64(*value, "price")?;
            match kind {
                PriceKind::Clean => (v + accrued, None),
                PriceKind::Dirty => (v, None),
            }
        }
        Mark::Yield { value, frequency } => {
            let y = dec_to_f64(*value, "yield")?;
            (
                dirty_price_from_yield(bond, settlement, y, *frequency)?,
                None,
            )
        }
        Mark::Spread { value, .. } => {
            let curve = curve.ok_or_else(|| {
                AnalyticsError::InvalidInput("spread mark requires a curve".into())
            })?;
            match value.spread_type() {
                SpreadType::ZSpread => {
                    let z_decimal = dec_to_f64(value.as_decimal(), "z-spread")?;
                    let dirty = ZSpreadCalculator::new(curve)
                        .price_with_spread(bond, z_decimal, settlement);
                    let z_bps = dec_to_f64(value.as_bps(), "z-spread bps")?;
                    (dirty, Some(z_bps))
                }
                // I-spread (vs swap curve) and G-spread (vs govt curve) share
                // the same arithmetic — the caller decides which curve to pass.
                // bond_yield = curve_par_rate_at_maturity + spread_decimal,
                // then yield → dirty price. z_spread_bps left None — call
                // the Z-spread tool directly if you want the equivalent.
                SpreadType::ISpread | SpreadType::GSpread => {
                    let t_maturity = curve.date_to_tenor(maturity);
                    let curve_rate = curve
                        .par_swap_rate(t_maturity, quote_frequency.periods_per_year())
                        .map_err(|e| AnalyticsError::CurveError(e.to_string()))?;
                    let spread_decimal = dec_to_f64(value.as_decimal(), "spread")?;
                    let target_yield = curve_rate + spread_decimal;
                    (
                        dirty_price_from_yield(bond, settlement, target_yield, quote_frequency)?,
                        None,
                    )
                }
                other => {
                    return Err(AnalyticsError::InvalidInput(format!(
                        "{other} mark not yet supported (Z-spread, I-spread, G-spread only; \
                         OAS marks require a callable bond + vol — call `compute_spread` instead)"
                    )));
                }
            }
        }
    };

    let clean = dirty - accrued;
    let ytm_decimal = yield_to_maturity(
        bond,
        settlement,
        f64_to_dec(clean, "clean price")?,
        quote_frequency,
    )?
    .yield_value;

    Ok(PricingResult {
        clean_price_per_100: clean,
        dirty_price_per_100: dirty,
        accrued_per_100: accrued,
        ytm_decimal,
        z_spread_bps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_bonds::instruments::FixedRateBond;
    use convex_core::daycounts::DayCountConvention;
    use convex_core::types::{Compounding, Currency, Mark, Spread, SpreadType};
    use convex_curves::{DiscreteCurve, InterpolationMethod, RateCurve, ValueType};
    use rust_decimal_macros::dec;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn bond_5pct_10y() -> FixedRateBond {
        FixedRateBond::builder()
            .cusip_unchecked("TEST10Y5")
            .coupon_rate(dec!(0.05))
            .maturity(d(2035, 1, 15))
            .issue_date(d(2025, 1, 15))
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::Thirty360US)
            .currency(Currency::USD)
            .face_value(dec!(100))
            .build()
            .unwrap()
    }

    fn flat_curve(rate: f64) -> RateCurve<DiscreteCurve> {
        let dc = DiscreteCurve::new(
            d(2025, 1, 15),
            vec![0.5, 1.0, 2.0, 5.0, 10.0, 30.0],
            vec![rate; 6],
            ValueType::ZeroRate {
                compounding: Compounding::Continuous,
                day_count: DayCountConvention::Act365Fixed,
            },
            InterpolationMethod::Linear,
        )
        .unwrap();
        RateCurve::new(dc)
    }

    #[test]
    fn clean_mark_round_trips() {
        let bond = bond_5pct_10y();
        let mark = Mark::Price {
            value: dec!(99.5),
            kind: PriceKind::Clean,
        };
        let r = price_from_mark(&bond, d(2025, 4, 15), &mark, None, Frequency::SemiAnnual).unwrap();
        assert!((r.clean_price_per_100 - 99.5).abs() < 1e-9);
        assert!((r.dirty_price_per_100 - r.clean_price_per_100 - r.accrued_per_100).abs() < 1e-9);
        assert!(r.z_spread_bps.is_none());
    }

    #[test]
    fn dirty_mark_decomposes_to_clean_plus_accrued() {
        let bond = bond_5pct_10y();
        let mark = Mark::Price {
            value: dec!(101.5),
            kind: PriceKind::Dirty,
        };
        let r = price_from_mark(&bond, d(2025, 7, 15), &mark, None, Frequency::SemiAnnual).unwrap();
        assert!((r.dirty_price_per_100 - 101.5).abs() < 1e-9);
        assert!((r.clean_price_per_100 + r.accrued_per_100 - 101.5).abs() < 1e-9);
    }

    #[test]
    fn yield_mark_inverts_price_mark() {
        let bond = bond_5pct_10y();
        let settle = d(2025, 4, 15);
        let p1 = price_from_mark(
            &bond,
            settle,
            &Mark::Price {
                value: dec!(99.5),
                kind: PriceKind::Clean,
            },
            None,
            Frequency::SemiAnnual,
        )
        .unwrap();
        let p2 = price_from_mark(
            &bond,
            settle,
            &Mark::Yield {
                value: f64_to_dec(p1.ytm_decimal, "ytm").unwrap(),
                frequency: Frequency::SemiAnnual,
            },
            None,
            Frequency::SemiAnnual,
        )
        .unwrap();
        assert!((p2.clean_price_per_100 - p1.clean_price_per_100).abs() < 1e-6);
    }

    #[test]
    fn spread_mark_passes_through_input_z_spread() {
        let bond = bond_5pct_10y();
        let curve = flat_curve(0.04);
        let mark = Mark::Spread {
            value: Spread::new(dec!(75), SpreadType::ZSpread),
            benchmark: "FLAT.4PCT".into(),
        };
        let r = price_from_mark(
            &bond,
            d(2025, 4, 15),
            &mark,
            Some(&curve),
            Frequency::SemiAnnual,
        )
        .unwrap();
        // Pass-through: no re-solve, exact bps.
        assert_eq!(r.z_spread_bps, Some(75.0));
        assert!(r.clean_price_per_100 > 0.0);
    }

    #[test]
    fn spread_mark_without_curve_errors() {
        let bond = bond_5pct_10y();
        let mark = Mark::Spread {
            value: Spread::new(dec!(50), SpreadType::ZSpread),
            benchmark: "USD.SOFR".into(),
        };
        let err =
            price_from_mark(&bond, d(2025, 4, 15), &mark, None, Frequency::SemiAnnual).unwrap_err();
        assert!(matches!(err, AnalyticsError::InvalidInput(_)));
    }

    #[test]
    fn ispread_mark_round_trips_to_yield_mark() {
        // 50 bp I-spread over a flat 4% curve at SA → bond yield = 4.5% SA.
        // The same bond marked at Yield(0.045) should produce the same dirty.
        let bond = bond_5pct_10y();
        let curve = flat_curve(0.04);
        let i_mark = Mark::Spread {
            value: Spread::new(dec!(50), SpreadType::ISpread),
            benchmark: "USD.SOFR".into(),
        };
        let y_mark = Mark::Yield {
            value: dec!(0.045),
            frequency: Frequency::SemiAnnual,
        };
        let r_i = price_from_mark(
            &bond,
            d(2025, 4, 15),
            &i_mark,
            Some(&curve),
            Frequency::SemiAnnual,
        )
        .unwrap();
        let r_y =
            price_from_mark(&bond, d(2025, 4, 15), &y_mark, None, Frequency::SemiAnnual).unwrap();
        // The curve's par swap rate at 10Y is close to but not exactly 4%
        // (continuous-zero → semi-annual par compound conversion), so the
        // implied bond yield is close to but not exactly 4.5%. Tolerance is
        // dominated by that conversion (~5 bps).
        assert!(
            (r_i.dirty_price_per_100 - r_y.dirty_price_per_100).abs() < 0.5,
            "I-spread → yield round-trip dirty price mismatch: {} vs {}",
            r_i.dirty_price_per_100,
            r_y.dirty_price_per_100
        );
        assert!(r_i.z_spread_bps.is_none());
    }

    #[test]
    fn gspread_mark_works_against_govt_curve() {
        // G-spread shares the I-spread arithmetic; the only difference is
        // which curve the caller passes in. 75 bps G-spread over a flat 3%
        // govt curve → bond yield ≈ 3.75% SA. Just verify it produces a
        // sensible dirty price.
        let bond = bond_5pct_10y();
        let govt = flat_curve(0.03);
        let mark = Mark::Spread {
            value: Spread::new(dec!(75), SpreadType::GSpread),
            benchmark: "USD.TSY.10Y".into(),
        };
        let r = price_from_mark(
            &bond,
            d(2025, 4, 15),
            &mark,
            Some(&govt),
            Frequency::SemiAnnual,
        )
        .unwrap();
        // 5% coupon at ~3.75% yield → premium price > 100.
        assert!(
            r.clean_price_per_100 > 105.0,
            "expected premium price (yield well below coupon); got {}",
            r.clean_price_per_100
        );
    }

    #[test]
    fn oas_mark_still_rejected() {
        // OAS marks need callable + vol; not handled by price_from_mark.
        let bond = bond_5pct_10y();
        let curve = flat_curve(0.04);
        let mark = Mark::Spread {
            value: Spread::new(dec!(50), SpreadType::OAS),
            benchmark: "USD.SOFR".into(),
        };
        let err = price_from_mark(
            &bond,
            d(2025, 4, 15),
            &mark,
            Some(&curve),
            Frequency::SemiAnnual,
        )
        .unwrap_err();
        match err {
            AnalyticsError::InvalidInput(msg) => {
                assert!(msg.contains("OAS") || msg.contains("callable"))
            }
            _ => panic!("expected InvalidInput, got {err:?}"),
        }
    }
}
