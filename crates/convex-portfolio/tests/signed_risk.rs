#[path = "../../convex/examples/support/quote_fixture.rs"]
mod support;
use convex_analytics::quote_risk::*;
use convex_core::{contracts::*, Currency};
use convex_portfolio::signed_risk::*;
use support::Error;
fn scope() -> Result<RiskEntityScope, Error> {
    Ok(RiskEntityScope::Single {
        entity: EntityId::new("legal-A")?,
    })
}
fn trade(id: &str, r: QuoteSensitivity) -> Result<TradeSignedRisk, Error> {
    Ok(TradeSignedRisk {
        trade_id: TradeId::new(id)?,
        trade_version: Version::new(1)?,
        entity: EntityId::new("legal-A")?,
        currency: r.position.currency,
        gross_exposure: Some(ExactDecimal::parse("5000")?),
        required_factors: vec![r.key.clone()],
        outcomes: vec![TradeFactorRisk::Present {
            sensitivity: Box::new(r),
        }],
    })
}
fn long() -> Result<QuoteSensitivity, Error> {
    support::Fixture::load()?.raw_yield("5000", 0.0001)
}
fn fx(from: Currency) -> Result<RiskFxPin, Error> {
    Ok(RiskFxPin {
        from,
        to: Currency::EUR,
        rate: ExactDecimal::parse("0.9")?,
        version: Version::new(7)?,
        snapshot_id: support::snapshot()?,
    })
}
#[test]
fn signed_quantity_sums_do_not_use_net_market_value_or_duration() -> Result<(), Error> {
    let f = support::Fixture::load()?;
    let l = f.raw_yield("5000", 0.0001)?;
    let s = f.raw_yield("-5000", 0.0001)?;
    assert_eq!(l.base_pv, -s.base_pv);
    let a = trade("long", l)?;
    let mut b = trade("short", s)?;
    // Distinct quote/convention factors stay visible even when trade PVs net to zero.
    let mut definition = b.required_factors[0].definition().clone();
    let mut factor = definition.factor.definition().clone();
    factor.model_version = ModelId::new("other-explicit-model-v1")?;
    definition.factor = RiskFactorKey::new(factor)?;
    let key = QuoteRiskKey::new(definition)?;
    b.required_factors[0] = key.clone();
    if let TradeFactorRisk::Present { sensitivity } = &mut b.outcomes[0] {
        sensitivity.key = key;
    }
    let result =
        aggregate_signed_quote_risk(&[a, b], support::snapshot()?, scope()?, Currency::USD, &[])?;
    assert_eq!(result.status, SignedRiskCoverageStatus::Complete);
    assert_eq!(result.local.len(), 2);
    assert!(result
        .local
        .iter()
        .all(|r| r.known_value.is_some_and(|v| v.abs() > 0.1)));
    assert_eq!(
        result.coverage[0]
            .known_gross_exposure
            .decimal()
            .to_string(),
        "10000"
    );
    Ok(())
}
#[test]
fn missing_trades_are_incomplete_and_all_missing_values_are_absent() -> Result<(), Error> {
    let r = long()?;
    let a = trade("covered", r.clone())?;
    let mut b = trade("rejected", r.clone())?;
    b.outcomes = vec![TradeFactorRisk::Missing {
        key: r.key.clone(),
        reason: "original curve calibration failed".into(),
    }];
    let result = aggregate_signed_quote_risk(
        &[a, b.clone()],
        support::snapshot()?,
        scope()?,
        Currency::USD,
        &[],
    )?;
    assert_eq!(result.status, SignedRiskCoverageStatus::Incomplete);
    assert_eq!(result.coverage[0].trades, 2);
    assert_eq!(result.coverage[0].covered_trades, 1);
    assert_eq!(result.coverage[0].covered_factors, 1);
    assert_eq!(result.missing.len(), 1);
    assert_eq!(result.local[0].known_value, Some(r.value));
    assert_eq!(result.local[0].status, SignedRiskCoverageStatus::Incomplete);
    b.outcomes.clear();
    let all =
        aggregate_signed_quote_risk(&[b], support::snapshot()?, scope()?, Currency::USD, &[])?;
    assert_eq!(all.local[0].known_value, None);
    assert_eq!(all.reporting[0].known_value, None);
    assert_eq!(all.coverage[0].covered_trades, 0);
    Ok(())
}
#[test]
fn unknown_gross_exposure_remains_unknown_without_losing_risk_count() -> Result<(), Error> {
    let mut a = trade("unknown-size", long()?)?;
    a.gross_exposure = None;
    let report =
        aggregate_signed_quote_risk(&[a], support::snapshot()?, scope()?, Currency::USD, &[])?;
    assert_eq!(report.status, SignedRiskCoverageStatus::Incomplete);
    assert_eq!(report.coverage[0].unknown_exposure_trades, 1);
    assert_eq!(report.coverage[0].covered_trades, 1);
    assert_eq!(report.local[0].status, SignedRiskCoverageStatus::Complete);
    Ok(())
}
#[test]
fn pinned_fx_conversion_retains_local_risk_and_induced_conversion_exposure() -> Result<(), Error> {
    let r = long()?;
    let a = trade("USD-trade", r.clone())?;
    let pin = fx(Currency::USD)?;
    let result = aggregate_signed_quote_risk(
        &[a],
        support::snapshot()?,
        scope()?,
        Currency::EUR,
        std::slice::from_ref(&pin),
    )?;
    assert_eq!(result.status, SignedRiskCoverageStatus::Complete);
    assert_eq!(result.local[0].known_value, Some(r.value));
    assert!((result.reporting[0].known_value.ok_or("reporting")? - r.value * 0.9).abs() < 1e-9);
    assert_eq!(result.fx_conversion_exposure[0].fx, pin);
    assert_eq!(
        result.fx_conversion_exposure[0].local_sensitivity,
        Some(r.value)
    );
    Ok(())
}
#[test]
fn absent_fx_is_incomplete_even_when_local_risk_was_calculated() -> Result<(), Error> {
    let a = trade("missing-FX", long()?)?;
    let result =
        aggregate_signed_quote_risk(&[a], support::snapshot()?, scope()?, Currency::EUR, &[])?;
    assert_eq!(result.status, SignedRiskCoverageStatus::Incomplete);
    assert_eq!(result.missing_fx, vec![Currency::USD]);
    assert!(result.local[0].known_value.is_some());
    assert_eq!(result.reporting[0].known_value, None);
    assert_eq!(result.reporting[0].missing_trades.len(), 1);
    Ok(())
}
#[test]
fn duplicate_wrong_snapshot_or_wrong_currency_fx_fails() -> Result<(), Error> {
    let a = trade("t", long()?)?;
    let pin = fx(Currency::USD)?;
    assert!(aggregate_signed_quote_risk(
        std::slice::from_ref(&a),
        support::snapshot()?,
        scope()?,
        Currency::EUR,
        &[pin.clone(), pin.clone()]
    )
    .is_err());
    let mut stale = pin.clone();
    stale.snapshot_id = SnapshotId::new("stale")?;
    assert!(aggregate_signed_quote_risk(
        std::slice::from_ref(&a),
        support::snapshot()?,
        scope()?,
        Currency::EUR,
        &[stale]
    )
    .is_err());
    let mut zero = pin.clone();
    zero.rate = ExactDecimal::parse("0")?;
    assert!(aggregate_signed_quote_risk(
        std::slice::from_ref(&a),
        support::snapshot()?,
        scope()?,
        Currency::EUR,
        &[zero]
    )
    .is_err());
    let mut wrong = pin;
    wrong.to = Currency::GBP;
    assert!(aggregate_signed_quote_risk(
        &[a],
        support::snapshot()?,
        scope()?,
        Currency::EUR,
        &[wrong]
    )
    .is_err());
    Ok(())
}
#[test]
fn duplicate_trade_factor_unrequested_or_mixed_original_pv_fails() -> Result<(), Error> {
    let a = trade("duplicate", long()?)?;
    assert!(aggregate_signed_quote_risk(
        &[a.clone(), a.clone()],
        support::snapshot()?,
        scope()?,
        Currency::USD,
        &[]
    )
    .is_err());
    let mut dup = a.clone();
    dup.required_factors.push(dup.required_factors[0].clone());
    assert!(aggregate_signed_quote_risk(
        &[dup],
        support::snapshot()?,
        scope()?,
        Currency::USD,
        &[]
    )
    .is_err());
    let mut dup = a.clone();
    dup.outcomes.push(dup.outcomes[0].clone());
    assert!(aggregate_signed_quote_risk(
        &[dup],
        support::snapshot()?,
        scope()?,
        Currency::USD,
        &[]
    )
    .is_err());
    let mut stale = a.clone();
    if let TradeFactorRisk::Present { sensitivity } = &mut stale.outcomes[0] {
        sensitivity.snapshot_id = SnapshotId::new("wrong-snapshot")?;
    }
    assert!(aggregate_signed_quote_risk(
        &[stale],
        support::snapshot()?,
        scope()?,
        Currency::USD,
        &[]
    )
    .is_err());
    let mut wrong = a;
    if let TradeFactorRisk::Present { sensitivity } = &mut wrong.outcomes[0] {
        sensitivity.position.currency = Currency::EUR;
    }
    assert!(aggregate_signed_quote_risk(
        &[wrong],
        support::snapshot()?,
        scope()?,
        Currency::USD,
        &[]
    )
    .is_err());
    Ok(())
}
#[test]
fn legal_entities_require_explicit_authorized_reporting_scope() -> Result<(), Error> {
    let a = trade("entity-A", long()?)?;
    let mut b = trade("entity-B", long()?)?;
    b.entity = EntityId::new("legal-B")?;
    assert!(aggregate_signed_quote_risk(
        &[a.clone(), b.clone()],
        support::snapshot()?,
        scope()?,
        Currency::USD,
        &[]
    )
    .is_err());
    let scope = RiskEntityScope::Authorized {
        entities: vec![a.entity.clone(), b.entity.clone()],
        authorization_version: Version::new(1)?,
    };
    let report =
        aggregate_signed_quote_risk(&[a, b], support::snapshot()?, scope, Currency::USD, &[])?;
    assert_eq!(report.status, SignedRiskCoverageStatus::Complete);
    assert_eq!(report.coverage[0].trades, 2);
    Ok(())
}
#[test]
fn complete_model_pins_never_net_under_a_shared_target_or_same_currency() -> Result<(), Error> {
    let r = long()?;
    let mut records = Vec::new();
    // Actual full reference yield risk is reused only as monetary record validation;
    // factor identity itself must remain disjoint under different declared pins.
    for (i, name) in ["model1", "model2", "model3", "model4", "model5", "model6"]
        .iter()
        .enumerate()
    {
        let mut copy = r.clone();
        let mut d = copy.key.definition().clone();
        let mut f = d.factor.definition().clone();
        f.model_version = ModelId::new(format!("{name}-explicit-model"))?;
        d.factor = RiskFactorKey::new(f)?;
        copy.key = QuoteRiskKey::new(d)?;
        records.push(trade(&format!("t{i}"), copy)?);
    }
    let report =
        aggregate_signed_quote_risk(&records, support::snapshot()?, scope()?, Currency::USD, &[])?;
    assert_eq!(report.local.len(), 6);
    assert_eq!(report.reporting.len(), 6);
    Ok(())
}
#[test]
fn requested_yield_zero_par_z_cds_and_recovery_keep_distinct_coverage_rows() -> Result<(), Error> {
    use convex_curves::contracts::{CurveKey, QuoteUnit};
    let risk = long()?;
    let curve = CurveKey {
        id: "shared-risk-object".into(),
        version: "original-v1".into(),
    };
    let raw = || -> Result<RiskCoordinate, Error> {
        Ok(RiskCoordinate::Raw {
            input_id: "shared-risk-object".into(),
            version: Version::new(1)?,
        })
    };
    let quote = || -> Result<RiskCoordinate, Error> {
        Ok(RiskCoordinate::Quote {
            curve: curve.clone(),
            helper_id: "shared-helper".into(),
            quote_version: Version::new(1)?,
        })
    };
    let keys = vec![
        risk.key.clone(),
        support::key(
            QuoteType::ZeroRate,
            RiskCoordinate::Zero {
                curve: curve.clone(),
                node: None,
            },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "parallel",
        )?,
        support::key(
            QuoteType::SwapParRate,
            quote()?,
            QuoteUnit::DecimalRate,
            RiskResponse::RebuildDependents,
            "5Y",
        )?,
        support::key(
            QuoteType::ZSpread,
            raw()?,
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "spot",
        )?,
        support::key(
            QuoteType::CdsSpread,
            quote()?,
            QuoteUnit::DecimalRate,
            RiskResponse::RebuildDependents,
            "5Y",
        )?,
        support::key(
            QuoteType::RecoveryRate,
            RiskCoordinate::Recovery { curve },
            QuoteUnit::DecimalRate,
            RiskResponse::FrozenCurves,
            "parallel",
        )?,
    ];
    let mut t = trade("explicit-required-taxonomy", risk)?;
    t.required_factors = keys;
    // Only the actual full bond yield result is supplied; other requested
    // economics remain absent, rather than relabeling that PV as another risk.
    let result =
        aggregate_signed_quote_risk(&[t], support::snapshot()?, scope()?, Currency::USD, &[])?;
    assert_eq!(result.local.len(), 6);
    assert_eq!(result.reporting.len(), 6);
    assert_eq!(
        result
            .local
            .iter()
            .filter(|r| r.known_value.is_some())
            .count(),
        1
    );
    assert_eq!(result.missing.len(), 5);
    assert_eq!(result.status, SignedRiskCoverageStatus::Incomplete);
    Ok(())
}
#[test]
fn negative_or_overflowed_gross_and_empty_expected_manifest_fail() -> Result<(), Error> {
    let mut a = trade("negative-gross", long()?)?;
    a.gross_exposure = Some(ExactDecimal::parse("-1")?);
    assert!(
        aggregate_signed_quote_risk(&[a], support::snapshot()?, scope()?, Currency::USD, &[])
            .is_err()
    );
    assert!(
        aggregate_signed_quote_risk(&[], support::snapshot()?, scope()?, Currency::USD, &[])
            .is_err()
    );
    let mut a = trade("max", long()?)?;
    a.gross_exposure = Some(ExactDecimal::parse("79228162514264337593543950335")?);
    let mut b = trade("one", long()?)?;
    b.gross_exposure = Some(ExactDecimal::parse("1")?);
    assert!(aggregate_signed_quote_risk(
        &[a, b],
        support::snapshot()?,
        scope()?,
        Currency::USD,
        &[]
    )
    .is_err());
    Ok(())
}
