//! Template narrator (no LLM). Renders a trader-brief paragraph from an
//! [`Attribution`] — same input → same bytes.
//!
//! Mirrors `risk::hedging::narrate::narrate` verbatim in style: pre-sized
//! `String`, `std::fmt::Write`, `Currency::code()`, bp `{:.2}` / ccy `{:.0}`,
//! a provenance disclosure tail. Single style — exactly like the shipped
//! hedge-advisor narrator (a one-variant style enum would be speculative
//! generality; add it the day a second style is real).
//!
//! The "hero moment" — the pay-fixed swap absorbing the curve move — is a
//! deterministic clause, not a heuristic: it fires whenever the book holds a
//! swap whose PnL sign opposes the bonds'.

use rust_decimal::prelude::ToPrimitive;

use super::types::{Attribution, PnlFactor};

/// Render an `Attribution` into a trader-brief paragraph.
#[must_use]
pub fn narrate_attribution(a: &Attribution) -> String {
    use std::fmt::Write;

    let ccy = a.currency.code();
    let mut out = String::with_capacity(640);
    let total = a.total_pnl_ccy.to_f64().unwrap_or(0.0);
    let mv = a.book_market_value_t0.to_f64().unwrap_or(0.0);

    let _ = write!(
        out,
        "Book PnL {} → {}: {} {:.0} ({:.2} bp on {} {:.0} market value).",
        a.t0, a.t1, ccy, total, a.total_pnl_bps, ccy, mv,
    );

    if a.positions.is_empty() {
        out.push_str(" No positions.");
        return out;
    }

    // Biggest driver: the largest-magnitude book factor (skip exact zeros).
    if let Some(top) = a
        .factors
        .iter()
        .filter(|f| f.pnl_ccy.to_f64().map(f64::abs).unwrap_or(0.0) > 0.5)
        .max_by(|x, y| {
            let xa = x.pnl_ccy.to_f64().unwrap_or(0.0).abs();
            let ya = y.pnl_ccy.to_f64().unwrap_or(0.0).abs();
            xa.partial_cmp(&ya).unwrap_or(std::cmp::Ordering::Equal)
        })
    {
        let label = match (&top.factor, &top.benchmark) {
            (PnlFactor::Spread, Some(b)) => format!("spread ({b})"),
            (f, _) => f.label().to_string(),
        };
        let _ = write!(
            out,
            " Biggest driver: {label} {} {:.0} ({:.2} bp).",
            ccy,
            top.pnl_ccy.to_f64().unwrap_or(0.0),
            top.pnl_bps,
        );
    }

    // Curve shape (identical across positions — same curves/decomposition).
    if let Some(c) = a.positions.first().map(|p| &p.curve) {
        let _ = write!(
            out,
            " Curve move decomposed at {:.0}y pivot: parallel {:.1} bp, slope {:.1} bp, curvature {:.1} bp.",
            c.pivot_tenor_years, c.parallel_bps, c.slope_bps, c.curvature_bps,
        );
    }

    // Spread moves, per benchmark (skip the zero/None rows the engine keeps
    // for completeness — the narrator is selective, the engine is complete).
    let spreads: Vec<&super::types::FactorPnl> = a
        .factors
        .iter()
        .filter(|f| {
            f.factor == PnlFactor::Spread
                && f.benchmark.is_some()
                && f.pnl_ccy.to_f64().map(f64::abs).unwrap_or(0.0) > 0.5
        })
        .collect();
    if !spreads.is_empty() {
        out.push_str(" Spread:");
        for (i, f) in spreads.iter().enumerate() {
            let v = f.pnl_ccy.to_f64().unwrap_or(0.0);
            let dir = if v < 0.0 { "widened" } else { "tightened" };
            let _ = write!(
                out,
                "{} {} {dir} ({} {:.0}, {:.2} bp)",
                if i == 0 { "" } else { ";" },
                f.benchmark.as_deref().unwrap_or(""),
                ccy,
                v,
                f.pnl_bps,
            );
        }
        out.push('.');
    }

    // Hero moment: a swap whose PnL opposes the bonds' aggregate.
    let swap_pnl: f64 = a
        .positions
        .iter()
        .filter(|p| p.kind == "swap")
        .map(|p| p.total_pnl_ccy.to_f64().unwrap_or(0.0))
        .sum();
    let bond_pnl: f64 = a
        .positions
        .iter()
        .filter(|p| p.kind != "swap")
        .map(|p| p.total_pnl_ccy.to_f64().unwrap_or(0.0))
        .sum();
    if swap_pnl.abs() > 0.5 && bond_pnl.abs() > 0.5 && swap_pnl.signum() != bond_pnl.signum() {
        let offset_pct = (swap_pnl.abs() / bond_pnl.abs() * 100.0).min(100.0);
        let _ = write!(
            out,
            " The pay-fixed swap contributed {} {:.0}, absorbing {:.0}% of the bonds' {} {:.0} move — last week's hedge working as designed.",
            ccy, swap_pnl, offset_pct, ccy, bond_pnl,
        );
    }

    // Provenance disclosure (mirrors the hedge narrator's cost-source tail).
    let _ = write!(
        out,
        " (curves {} → {}; factor model {}.)",
        a.provenance.curve_t0_id, a.provenance.curve_t1_id, a.provenance.factor_model,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::risk::pnl::types::{
        AttributionProvenance, CurveBreakdown, FactorPnl, PositionAttribution, FACTOR_MODEL_NAME,
    };
    use convex_core::types::{Currency, Date};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn curve() -> CurveBreakdown {
        CurveBreakdown {
            parallel_bps: 8.8,
            slope_bps: 5.95,
            curvature_bps: -2.8,
            pivot_tenor_years: 2.0,
            fit_residual_l1_bps: 0.0,
        }
    }

    fn prov() -> AttributionProvenance {
        AttributionProvenance {
            curve_t0_id: "eur_govt_t0".into(),
            curve_t1_id: "eur_govt_t1".into(),
            factor_model: FACTOR_MODEL_NAME.into(),
            pivot_tenor_years: 2.0,
            tool_version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    fn fac(f: PnlFactor, ccy: i64, bps: f64, b: Option<&str>) -> FactorPnl {
        FactorPnl {
            factor: f,
            pnl_ccy: Decimal::from(ccy),
            pnl_bps: bps,
            benchmark: b.map(str::to_string),
        }
    }

    fn pos(id: &str, kind: &str, total: i64) -> PositionAttribution {
        PositionAttribution {
            position_id: Some(id.into()),
            kind: kind.into(),
            market_value_t0: dec!(10_000_000),
            total_pnl_ccy: Decimal::from(total),
            total_pnl_bps: total as f64 / 10_000_000.0 * 1e4,
            factors: vec![],
            curve: curve(),
        }
    }

    fn demo() -> Attribution {
        Attribution {
            currency: Currency::EUR,
            t0: d(2026, 5, 7),
            t1: d(2026, 5, 8),
            book_market_value_t0: dec!(25_600_000),
            total_pnl_ccy: Decimal::from(-111_986),
            total_pnl_bps: -43.74,
            factors: vec![
                fac(PnlFactor::Carry, 1_259, 0.49, None),
                fac(PnlFactor::CurveParallel, -85_781, -33.51, None),
                fac(PnlFactor::CurveSlope, -8_767, -3.42, None),
                fac(PnlFactor::Spread, 0, 0.0, None),
                fac(PnlFactor::Spread, 0, 0.0, Some("DE.BUND")),
                fac(PnlFactor::Spread, -14_687, -5.74, Some("FR.OAT-DE.BUND")),
                fac(PnlFactor::Spread, -22_589, -8.82, Some("IT.BTP-DE.BUND")),
                fac(PnlFactor::Residual, 0, 0.0, None),
            ],
            positions: vec![
                pos("OAT", "bond", -75_166),
                pos("BTP", "bond", -54_238),
                pos("BUND", "bond", -62_665),
                pos("EUR_SWAP", "swap", 80_083),
            ],
            provenance: prov(),
        }
    }

    #[test]
    fn states_total_in_ccy_and_bp() {
        let s = narrate_attribution(&demo());
        assert!(s.contains("EUR -111986"));
        assert!(s.contains("-43.74 bp"));
    }

    #[test]
    fn names_biggest_driver() {
        let s = narrate_attribution(&demo());
        assert!(s.contains("Biggest driver: curve parallel"), "got: {s}");
    }

    #[test]
    fn reports_btp_bund_widening() {
        let s = narrate_attribution(&demo());
        assert!(s.contains("IT.BTP-DE.BUND widened"), "got: {s}");
        assert!(s.contains("FR.OAT-DE.BUND widened"), "got: {s}");
        // Exactly the two non-zero benchmark rows are narrated; the zero
        // DE.BUND and None rows the engine keeps for completeness are not.
        assert_eq!(s.matches("widened").count(), 2, "got: {s}");
        assert!(!s.contains(" DE.BUND widened"), "got: {s}");
    }

    #[test]
    fn fires_hero_moment_when_swap_offsets() {
        let s = narrate_attribution(&demo());
        assert!(
            s.contains("pay-fixed swap contributed EUR 80083"),
            "got: {s}"
        );
        assert!(s.contains("working as designed"));
        // bonds total = -192069; offset ≈ 41–42%.
        assert!(s.contains("absorbing 4"), "got: {s}");
    }

    #[test]
    fn no_swap_clause_for_bonds_only_book() {
        let mut a = demo();
        a.positions.retain(|p| p.kind != "swap");
        let s = narrate_attribution(&a);
        assert!(!s.contains("pay-fixed swap"));
        assert!(!s.contains("working as designed"));
        assert!(s.contains("Book PnL"));
    }

    #[test]
    fn deterministic_same_bytes() {
        let a = demo();
        assert_eq!(narrate_attribution(&a), narrate_attribution(&a));
    }

    #[test]
    fn handles_empty_positions() {
        let mut a = demo();
        a.positions.clear();
        let s = narrate_attribution(&a);
        assert!(s.contains("No positions"));
    }

    #[test]
    fn discloses_provenance() {
        let s = narrate_attribution(&demo());
        assert!(s.contains("curves eur_govt_t0 → eur_govt_t1"));
        assert!(s.contains(FACTOR_MODEL_NAME));
    }
}
