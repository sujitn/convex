use std::sync::Arc;

use convex_core::Date;
use convex_engine::{CalculationGraph, CurveBuilder};

#[test]
fn curve_content_changes_cannot_share_an_input_hash() {
    use convex_engine::ports::market_data::MarketDataProvider;
    use convex_ext_file::*;
    // Empty adapters provide no market inputs; explicit points exercise the actual builder.
    let market = MarketDataProvider {
        quotes: Arc::new(EmptyQuoteSource),
        curve_inputs: Arc::new(EmptyCurveInputSource),
        index_fixings: Arc::new(EmptyIndexFixingSource),
        volatility: Arc::new(EmptyVolatilitySource),
        fx_rates: Arc::new(EmptyFxRateSource),
        inflation_fixings: Arc::new(EmptyInflationFixingSource),
        etf_quotes: Arc::new(EmptyEtfQuoteSource),
    };
    let builder = CurveBuilder::new(Arc::new(market), Arc::new(CalculationGraph::new()));
    let date = Date::parse("2026-10-02").unwrap();
    let first = builder
        .create_from_points("USD.TEST".into(), date, vec![(1.0, 0.04), (2.0, 0.04)])
        .unwrap();
    let second = builder
        .create_from_points("USD.TEST".into(), date, vec![(1.0, 0.05), (2.0, 0.05)])
        .unwrap();
    assert_ne!(first.inputs_hash, second.inputs_hash);
}
