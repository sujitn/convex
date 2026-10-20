mod common;
use convex_credit::{convert_cds, CdsQuote, ConversionSettings, InitialPaymentState};
use serde_json::Value;

#[test]
fn complete_published_usd_sofr_grid_at_original_precision_and_dates(
) -> Result<(), Box<dyn std::error::Error>> {
    let i = common::inputs()?;
    let market = common::market(&i)?;
    let cal = common::calendar(&i);
    let g: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/comparisons/cds-isda-1.8.3/grid.json"
    ))?;
    let cases = g["cases"].as_array().ok_or("missing original cases")?;
    let measures = [
        ("clean_upfront_fraction", 1e-8),
        ("cash_settlement_amount", 0.5),
        ("clean_price_per100", 1e-6),
        ("accrued_days", 1e-8),
        ("accrued_premium", 1e-6),
    ];
    assert_eq!(cases.len(), 7164);
    assert_eq!(i.cases.len(), cases.len());
    for (c, e) in i.cases.iter().zip(cases) {
        let t = common::terms(&i, c)?;
        let contract = common::build(&t, &cal)?;
        let context = t.trade_context(InitialPaymentState::NotContracted);
        let rate = c.spread_bps.parse::<f64>()? / 10000.0;
        let r = convert_cds(
            &contract,
            &context,
            &market,
            CdsQuote::RunningSpread { decimal_rate: rate },
            ConversionSettings::default(),
        )?;
        assert_eq!(u64::from(c.row), e["row"].as_u64().ok_or("row")?);
        let v = &r.valuation;
        assert_eq!(
            v.cash_settlement_date.to_string(),
            e["cash_settlement_date"]
        );
        assert_eq!(v.step_in_date.to_string(), e["step_in_date"]);
        let a = [
            v.clean_upfront_fraction,
            v.cash_settlement_amount,
            v.clean_price_per100,
            v.accrued_days as f64,
            v.accrued_premium,
        ];
        for ((name, tolerance), actual) in measures.iter().zip(a) {
            assert_eq!(
                g["comparison_bounds"][name]["absolute"]
                    .as_str()
                    .ok_or("bound")?
                    .parse::<f64>()?,
                *tolerance
            );
            let expected = e[name].as_str().ok_or("original value")?.parse::<f64>()?;
            assert!((actual-expected).abs()<=*tolerance,"original row {} {name}: actual {actual}, expected {expected}, unchanged tolerance {tolerance}",c.row);
        }
        assert!(r.native_quote_residual.abs() <= r.settings.native_quote_tolerance);
        // Cover all maturities/coupons/recoveries over a fixed prime-spaced subset,
        // including high spreads, with unchanged declared native quote bounds.
        if c.row % 37 == 0 || c.row == 2 || c.row == 7165 {
            for q in [
                CdsQuote::CleanUpfront {
                    fraction: v.clean_upfront_fraction,
                },
                CdsQuote::DirtyUpfront {
                    fraction: v.dirty_upfront_fraction,
                },
            ] {
                let back = convert_cds(
                    &contract,
                    &context,
                    &market,
                    q,
                    ConversionSettings::default(),
                )?;
                assert!(
                    (back.conventional_spread_decimal - rate).abs() <= 1e-10,
                    "running/upfront roundtrip row {}",
                    c.row
                );
            }
        }
    }
    Ok(())
}
