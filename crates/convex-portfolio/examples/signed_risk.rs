//! Full native cash risk, pinned reporting FX and explicit missing coverage.
#[path = "../../convex/examples/support/quote_fixture.rs"]
mod support;

use convex_core::{contracts::*, Currency};
use convex_portfolio::signed_risk::*;

fn main() -> Result<(), support::Error> {
    let fixture = support::Fixture::load()?;
    let risk = fixture.raw_yield(&fixture.helper.terms.face_value.to_string(), 0.0001)?;
    let snapshot = support::snapshot()?;
    let entity = EntityId::new("synthetic-legal-A")?;
    let trade = TradeSignedRisk {
        trade_id: TradeId::new("synthetic-full-cash-trade")?,
        trade_version: Version::new(1)?,
        entity: entity.clone(),
        currency: risk.position.currency,
        gross_exposure: Some(risk.position.quantity),
        required_factors: vec![risk.key.clone()],
        outcomes: vec![TradeFactorRisk::Present {
            sensitivity: Box::new(risk),
        }],
    };
    let scope = RiskEntityScope::Single { entity };
    let fx = RiskFxPin {
        from: Currency::USD,
        to: Currency::EUR,
        rate: ExactDecimal::parse("0.9")?,
        version: Version::new(1)?,
        snapshot_id: snapshot.clone(),
    };
    let complete = aggregate_signed_quote_risk(
        std::slice::from_ref(&trade),
        snapshot.clone(),
        scope.clone(),
        Currency::EUR,
        std::slice::from_ref(&fx),
    )?;
    let mut missing = trade.clone();
    missing.trade_id = TradeId::new("synthetic-missing-trade")?;
    missing.outcomes = vec![TradeFactorRisk::Missing {
        key: missing.required_factors[0].clone(),
        reason: "Demonstrated missing full native valuation".into(),
    }];
    let incomplete =
        aggregate_signed_quote_risk(&[trade, missing], snapshot, scope, Currency::EUR, &[fx])?;
    if complete.status != SignedRiskCoverageStatus::Complete
        || incomplete.status != SignedRiskCoverageStatus::Incomplete
        || incomplete.coverage[0].trades != 2
        || incomplete.coverage[0].covered_trades != 1
    {
        return Err("Complete and missing coverage scenarios differ".into());
    }
    println!("{}", serde_json::to_string(&[complete, incomplete])?);
    Ok(())
}
