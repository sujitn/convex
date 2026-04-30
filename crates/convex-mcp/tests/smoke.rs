//! Smoke tests for the MCP tool surface.
//!
//! Each tool is exercised through a realistic end-to-end call. The aim is
//! contract verification: handler signatures match, every input shape
//! deserialises, every code path returns Ok on canonical input. Numerical
//! correctness lives in the per-crate unit tests.

use rmcp::handler::server::wrapper::Parameters;
use rust_decimal_macros::dec;

use convex::{Currency, DayCountConvention, Frequency, Mark, PriceKind, Spread, SpreadType};
use convex_mcp::server::*;

fn d(year: i32, month: u32, day: u32) -> DateInput {
    DateInput { year, month, day }
}

fn ust_10y_spec() -> BondSpec {
    BondSpec {
        coupon_rate_pct: 4.5,
        maturity: d(2035, 1, 15),
        issue_date: d(2025, 1, 15),
        frequency: Frequency::SemiAnnual,
        day_count: DayCountConvention::ActActIcma,
        currency: Currency::USD,
        face_value: 100.0,
        make_whole_spread_bps: None,
    }
}

fn flat_curve_spec(rate_pct: f64) -> CurveSpec {
    CurveSpec {
        reference_date: d(2025, 1, 15),
        tenors_years: vec![0.5, 1.0, 2.0, 5.0, 10.0, 30.0],
        zero_rates_pct: vec![rate_pct; 6],
    }
}

#[tokio::test]
async fn end_to_end_happy_path() {
    let server = ConvexMcpServer::new();

    // 1. Create a bond by id.
    server
        .create_bond(Parameters(CreateBondParams {
            id: "UST.10Y".into(),
            spec: ust_10y_spec(),
        }))
        .await
        .expect("create_bond");

    // 2. Create a curve by id.
    server
        .create_curve(Parameters(CreateCurveParams {
            id: "USD.TSY".into(),
            spec: flat_curve_spec(4.0),
        }))
        .await
        .expect("create_curve");

    // 3. Bootstrap a SOFR curve from market quotes; store under USD.SOFR.
    server
        .bootstrap_curve(Parameters(BootstrapCurveParams {
            reference_date: d(2025, 1, 15),
            instruments: vec![
                BootstrapInstrument::Deposit {
                    tenor_years: 0.25,
                    rate_pct: 4.40,
                    day_count: DayCountConvention::Act360,
                },
                BootstrapInstrument::Swap {
                    tenor_years: 2.0,
                    fixed_rate_pct: 4.20,
                    fixed_frequency: Frequency::SemiAnnual,
                    fixed_day_count: DayCountConvention::Thirty360US,
                },
                BootstrapInstrument::Swap {
                    tenor_years: 5.0,
                    fixed_rate_pct: 4.30,
                    fixed_frequency: Frequency::SemiAnnual,
                    fixed_day_count: DayCountConvention::Thirty360US,
                },
                BootstrapInstrument::Swap {
                    tenor_years: 10.0,
                    fixed_rate_pct: 4.45,
                    fixed_frequency: Frequency::SemiAnnual,
                    fixed_day_count: DayCountConvention::Thirty360US,
                },
            ],
            store_as: Some("USD.SOFR".into()),
        }))
        .await
        .expect("bootstrap_curve");

    // 4. Query a zero rate.
    server
        .get_zero_rate(Parameters(GetRateParams {
            curve: CurveRef::Id("USD.TSY".into()),
            tenor_years: 5.0,
        }))
        .await
        .expect("get_zero_rate");

    // 5. Price the bond from each Mark variant.
    for mark in [
        Mark::Price {
            value: dec!(99.5),
            kind: PriceKind::Clean,
        },
        Mark::Yield {
            value: dec!(0.0455),
            frequency: Frequency::SemiAnnual,
        },
        Mark::Spread {
            value: Spread::new(dec!(50), SpreadType::ZSpread),
            benchmark: "USD.TSY".into(),
        },
    ] {
        server
            .price_bond(Parameters(PriceBondParams {
                bond: BondRef::Id("UST.10Y".into()),
                settlement: d(2025, 4, 15),
                mark,
                curve: Some(CurveRef::Id("USD.TSY".into())),
                quote_frequency: None,
            }))
            .await
            .expect("price_bond");
    }

    // 6. compute_spread for each kind.
    for kind in [
        SpreadKind::ZSpread,
        SpreadKind::ISpread,
        SpreadKind::GSpread,
    ] {
        server
            .compute_spread(Parameters(ComputeSpreadParams {
                bond: BondRef::Id("UST.10Y".into()),
                curve: CurveRef::Id("USD.TSY".into()),
                settlement: d(2025, 4, 15),
                clean_price_per_100: 99.5,
                kind,
            }))
            .await
            .expect("compute_spread");
    }

    // 7. calculate_yield.
    server
        .calculate_yield(Parameters(CalculateYieldParams {
            bond: BondRef::Id("UST.10Y".into()),
            settlement: d(2025, 4, 15),
            clean_price_per_100: 99.5,
        }))
        .await
        .expect("calculate_yield");

    // 8. Listing tools.
    server.list_all_bonds().await.expect("list_all_bonds");
    server.list_all_curves().await.expect("list_all_curves");
}

#[tokio::test]
async fn inline_specs_avoid_registry_round_trip() {
    let server = ConvexMcpServer::new();

    server
        .price_bond(Parameters(PriceBondParams {
            bond: BondRef::Spec(ust_10y_spec()),
            settlement: d(2025, 4, 15),
            mark: Mark::Price {
                value: dec!(99.5),
                kind: PriceKind::Clean,
            },
            curve: Some(CurveRef::Spec(flat_curve_spec(4.0))),
            quote_frequency: None,
        }))
        .await
        .expect("price_bond inline");

    server
        .compute_spread(Parameters(ComputeSpreadParams {
            bond: BondRef::Spec(ust_10y_spec()),
            curve: CurveRef::Spec(flat_curve_spec(4.0)),
            settlement: d(2025, 4, 15),
            clean_price_per_100: 99.5,
            kind: SpreadKind::ZSpread,
        }))
        .await
        .expect("compute_spread inline");
}

#[tokio::test]
async fn unknown_bond_id_errors() {
    let server = ConvexMcpServer::new();
    let res = server
        .calculate_yield(Parameters(CalculateYieldParams {
            bond: BondRef::Id("MISSING".into()),
            settlement: d(2025, 4, 15),
            clean_price_per_100: 100.0,
        }))
        .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn settlement_after_maturity_errors() {
    let server = ConvexMcpServer::new();
    let res = server
        .price_bond(Parameters(PriceBondParams {
            bond: BondRef::Spec(ust_10y_spec()),
            settlement: d(2040, 1, 15), // past maturity
            mark: Mark::Price {
                value: dec!(99.5),
                kind: PriceKind::Clean,
            },
            curve: None,
            quote_frequency: None,
        }))
        .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn spread_mark_without_curve_errors() {
    let server = ConvexMcpServer::new();
    let res = server
        .price_bond(Parameters(PriceBondParams {
            bond: BondRef::Spec(ust_10y_spec()),
            settlement: d(2025, 4, 15),
            mark: Mark::Spread {
                value: Spread::new(dec!(50), SpreadType::ZSpread),
                benchmark: "X".into(),
            },
            curve: None,
            quote_frequency: None,
        }))
        .await;
    assert!(res.is_err());
}
