#[path = "../../convex-ports/tests/support/snapshot_fixtures.rs"]
mod fixture;

use convex_core::{ids::CurveId, Date};
use convex_curves::{ExtrapolationMethod, RateCurveDyn};
use convex_engine::{curve_builder::BuiltCurve, CalculationGraph, CurveBuilder};
use convex_ext_file::*;
use convex_ports::{market_data::*, snapshot::ZeroExtrapolation, TraitError};
use std::sync::Arc;

struct UnreadCurveInputs;

#[async_trait::async_trait]
impl CurveInputSource for UnreadCurveInputs {
    fn source_type(&self) -> SourceType {
        SourceType::Manual
    }

    async fn get_curve_inputs(&self, _: &CurveId) -> Result<Vec<CurveInput>, TraitError> {
        panic!("unsupported policy must fail before provider access")
    }

    async fn subscribe(&self, _: &[CurveId]) -> Result<StreamReceiver<CurveInput>, TraitError> {
        panic!("construction must not subscribe")
    }
}

fn builder() -> CurveBuilder {
    CurveBuilder::new(
        Arc::new(MarketDataProvider {
            quotes: Arc::new(EmptyQuoteSource),
            curve_inputs: Arc::new(UnreadCurveInputs),
            index_fixings: Arc::new(EmptyIndexFixingSource),
            volatility: Arc::new(EmptyVolatilitySource),
            fx_rates: Arc::new(EmptyFxRateSource),
            inflation_fixings: Arc::new(EmptyInflationFixingSource),
            etf_quotes: Arc::new(EmptyEtfQuoteSource),
        }),
        Arc::new(CalculationGraph::new()),
    )
}

#[test]
fn older_engine_default_discloses_its_actual_flat_zero_policy() {
    let builder = builder();
    let curve = builder
        .create_from_points(
            "USD.METHOD".into(),
            Date::parse("2026-10-02").unwrap(),
            vec![(1.0, 0.02), (2.0, 0.03)],
        )
        .unwrap();
    assert_eq!(curve.extrapolation, ExtrapolationMethod::Flat);
    assert_eq!(curve.interpolate_rate(0.0), 0.02);
    assert_eq!(curve.interpolate_rate(40.0), 0.03);
    assert_eq!(curve.discount_factor(0.0).unwrap(), 1.0);
}

#[tokio::test]
async fn unsupported_engine_tail_fails_before_provider_cache_or_graph_work() {
    let builder = builder().with_extrapolation(ExtrapolationMethod::FlatForward);
    let id = CurveId::new("USD.METHOD");
    let date = Date::parse("2026-10-02").unwrap();
    for result in [
        builder.create_from_points(id.clone(), date, vec![(1.0, 0.02), (2.0, 0.03)]),
        builder.build(&id, date).await,
    ] {
        assert!(
            matches!(result, Err(convex_engine::EngineError::CurveBuildError(message))
            if message.contains("complete curve-method contract"))
        );
    }
    assert!(builder.get(&id).is_none());
    assert!(builder.list().is_empty());
}

#[test]
fn snapshot_tail_policy_is_retained_or_rejected_without_relabeling() {
    let mut record = fixture::curve();
    assert_eq!(record.extrapolation, ZeroExtrapolation::FlatZero);
    let reconstructed = BuiltCurve::from_snapshot(&record).unwrap();
    assert_eq!(reconstructed.extrapolation, ExtrapolationMethod::Flat);
    assert_eq!(reconstructed.discount_factor(0.0).unwrap(), 1.0);
    record.extrapolation = ZeroExtrapolation::FlatForward;
    assert!(matches!(
        BuiltCurve::from_snapshot(&record),
        Err(TraitError::InvalidInput(message)) if message.contains("complete curve-method contract")
    ));
}
