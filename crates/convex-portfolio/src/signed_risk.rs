//! Complete-key signed monetary risk aggregation with explicit missing coverage.
//! Existing duration/DV01 summary APIs retain their original meanings.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use convex_analytics::quote_risk::{QuoteRiskKey, QuoteSensitivity};
use convex_core::{
    contracts::{EntityId, ExactDecimal, SnapshotId, TradeId, Version},
    Currency,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Checked portfolio input or overflow failure.
#[derive(Debug, thiserror::Error)]
#[error("signed portfolio risk: {0}")]
pub struct SignedPortfolioRiskError(String);
fn error(s: impl Into<String>) -> SignedPortfolioRiskError {
    SignedPortfolioRiskError(s.into())
}
/// Entity scope is part of the request. Multi-entity risk cannot net by accident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RiskEntityScope {
    /// One explicitly selected legal entity.
    Single {
        /// Legal entity identity.
        entity: EntityId,
    },
    /// Caller-authorized multi-entity reporting; calculation confers no authorization.
    Authorized {
        /// Explicit distinct permitted entities.
        entities: Vec<EntityId>,
        /// Actual authorization/configuration revision.
        authorization_version: Version,
    },
}
/// One expected factor outcome; missing/failed trade risk is never a numeric zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum TradeFactorRisk {
    /// Actual full native reference result with signed quantity already applied.
    Present {
        /// Stable checked sensitivity.
        sensitivity: Box<QuoteSensitivity>,
    },
    /// Requested risk that could not be calculated or was not supplied.
    Missing {
        /// Exact expected complete factor key.
        key: QuoteRiskKey,
        /// Explicit calculation/data failure or absent result.
        reason: String,
    },
}
impl TradeFactorRisk {
    fn key(&self) -> &QuoteRiskKey {
        match self {
            Self::Present { sensitivity } => &sensitivity.key,
            Self::Missing { key, .. } => key,
        }
    }
}
/// Complete expected trade manifest. Even a failed valuation must have an entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TradeSignedRisk {
    /// Exact expected trade identity.
    pub trade_id: TradeId,
    /// Original trade revision.
    pub trade_version: Version,
    /// Authoritative legal entity.
    pub entity: EntityId,
    /// Native monetary currency, including missing valuation entries.
    pub currency: Currency,
    /// Caller-defined nonnegative gross exposure in this local currency.
    /// None means unknown, not zero; the report keeps its count separately.
    pub gross_exposure: Option<ExactDecimal>,
    /// Complete expected factor keys for this trade; omission remains visible.
    pub required_factors: Vec<QuoteRiskKey>,
    /// Present or explicitly failed supplied results. Absent requested keys are missing.
    pub outcomes: Vec<TradeFactorRisk>,
}
/// Exact supplied reporting FX. This converts monetary results; it is not an FX
/// risk model or an authorization to trade cross-currency hedges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RiskFxPin {
    /// Local monetary currency.
    pub from: Currency,
    /// Reporting monetary currency.
    pub to: Currency,
    /// Positive reporting currency per1 local currency.
    pub rate: ExactDecimal,
    /// Actual original supplied observation revision.
    pub version: Version,
    /// Same immutable risk information set.
    pub snapshot_id: SnapshotId,
}
/// Coverage status; known partial values never become Complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SignedRiskCoverageStatus {
    /// Every requested trade/factor/exposure/FX is supplied.
    Complete,
    /// At least one required component is missing.
    Incomplete,
}
/// One omitted or rejected trade-factor result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MissingSignedRisk {
    /// Original expected trade.
    pub trade_id: TradeId,
    /// Original factor key.
    pub key: QuoteRiskKey,
    /// Retained failure/absence reason.
    pub reason: String,
}
/// Per-currency coverage, retaining unknown gross exposure separately.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SignedRiskCoverage {
    /// Local monetary currency.
    pub currency: Currency,
    /// Expected trade count, including every failure.
    pub trades: usize,
    /// Trades for which every required factor is actually present.
    pub covered_trades: usize,
    /// Required trade-factor pairs.
    pub required_factors: usize,
    /// Present checked trade-factor pairs.
    pub covered_factors: usize,
    /// Sum of all known gross exposure. Its definition is caller-supplied.
    pub known_gross_exposure: ExactDecimal,
    /// Known gross exposure on fully covered trades.
    pub covered_known_gross_exposure: ExactDecimal,
    /// Actual count with unknown gross exposure.
    pub unknown_exposure_trades: usize,
    /// Complete only when risk coverage and gross exposure are known.
    pub status: SignedRiskCoverageStatus,
}
/// One complete-key monetary aggregate, never mixing currencies, units or bases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SignedRiskAggregate {
    /// Exact coordinate, quote, convention, model and response semantics.
    pub key: QuoteRiskKey,
    /// Monetary reporting unit; normalization remains in the key.
    pub currency: Currency,
    /// Known partial sum; None means no present contribution at all.
    pub known_value: Option<f64>,
    /// Complete only when all required contributions and FX are present.
    pub status: SignedRiskCoverageStatus,
    /// Trades missing this particular local/reporting factor result.
    pub missing_trades: Vec<TradeId>,
}
/// Conversion exposure for one local monetary factor: d(reporting risk)/d(FX).
/// This is conversion of this risk result, not the full portfolio's FX delta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct RiskFxConversionExposure {
    /// Complete local factor being converted.
    pub key: QuoteRiskKey,
    /// Exact original rate/revision used for conversion.
    pub fx: RiskFxPin,
    /// Signed local risk; derivative per +1 reporting/local FX-rate move.
    pub local_sensitivity: Option<f64>,
    /// Retained local risk coverage status.
    pub status: SignedRiskCoverageStatus,
}
/// Pure local and pinned-FX report over an explicit expected trade manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SignedPortfolioRisk {
    /// Same original immutable input set.
    pub snapshot_id: SnapshotId,
    /// Caller-selected legal reporting scope.
    pub scope: RiskEntityScope,
    /// Complete only when all expected factors, gross exposures and FX exist.
    pub status: SignedRiskCoverageStatus,
    /// Every original per-currency expected and covered trade/exposure count.
    pub coverage: Vec<SignedRiskCoverage>,
    /// Local currency sums, separate for each full factor key.
    pub local: Vec<SignedRiskAggregate>,
    /// Reporting currency sums, preserving full original factor identity.
    pub reporting: Vec<SignedRiskAggregate>,
    /// Omitted/rejected/failed original trade-factor outcomes.
    pub missing: Vec<MissingSignedRisk>,
    /// Local currencies lacking a positive exact same-snapshot FX observation.
    pub missing_fx: Vec<Currency>,
    /// Induced conversion exposure, retaining actual supplied FX pins.
    pub fx_conversion_exposure: Vec<RiskFxConversionExposure>,
}
fn add(a: Option<f64>, b: f64) -> Result<Option<f64>, SignedPortfolioRiskError> {
    let v = a.unwrap_or(0.0) + b;
    if !v.is_finite() {
        Err(error("monetary sum overflow"))
    } else {
        Ok(Some(v))
    }
}
/// Aggregate signed monetary risk without using duration, net market value, or
/// missing-to-zero conversion. Invalid, duplicate, stale or incompatible supplied
/// results fail; expected absent results return an explicitly Incomplete report.
/// Caller supplies the COMPLETE trade manifest and authorized entity scope.
pub fn aggregate_signed_quote_risk(
    trades: &[TradeSignedRisk],
    snapshot_id: SnapshotId,
    scope: RiskEntityScope,
    reporting_currency: Currency,
    fx: &[RiskFxPin],
) -> Result<SignedPortfolioRisk, SignedPortfolioRiskError> {
    if trades.is_empty()
        || trades.len() > 4096
        || fx.len() > 256
        || trades
            .iter()
            .map(|t| t.required_factors.len())
            .sum::<usize>()
            > 65536
    {
        return Err(error(
            "bounded nonempty expected trade/factor manifest required",
        ));
    }
    let entities = match &scope {
        RiskEntityScope::Single { entity } => vec![entity.clone()],
        RiskEntityScope::Authorized { entities, .. } => {
            if entities.len() < 2
                || entities.len() > 128
                || entities.iter().collect::<HashSet<_>>().len() != entities.len()
            {
                return Err(error("explicit distinct authorized entity list required"));
            }
            entities.clone()
        }
    };
    let mut fx_map = HashMap::new();
    for f in fx {
        if f.from == f.to
            || f.to != reporting_currency
            || f.rate.decimal() <= Decimal::ZERO
            || f.snapshot_id != snapshot_id
            || fx_map.insert(f.from, f).is_some()
        {
            return Err(error("duplicate/incoherent/nonpositive/stale supplied FX"));
        }
    }
    let mut local = Vec::<SignedRiskAggregate>::new();
    let mut rows = HashMap::new();
    let mut coverage = Vec::<SignedRiskCoverage>::new();
    let mut currency_rows = HashMap::new();
    let mut missing = Vec::new();
    let mut ids = HashSet::new();
    for t in trades {
        if !ids.insert(&t.trade_id)
            || !entities.contains(&t.entity)
            || t.required_factors.is_empty()
            || t.required_factors.len() > 128
            || t.outcomes.len() > t.required_factors.len()
        {
            return Err(error(
                "duplicate trade, unauthorized entity or invalid expected factor dimensions",
            ));
        }
        let required = t.required_factors.iter().collect::<HashSet<_>>();
        if required.len() != t.required_factors.len() {
            return Err(error("duplicate expected factor key"));
        }
        let mut actual = HashMap::new();
        let mut position = None;
        let mut pv = None;
        for r in &t.outcomes {
            if !required.contains(r.key()) || actual.insert(r.key(), r).is_some() {
                return Err(error("duplicate/unrequested trade factor"));
            }
            if let TradeFactorRisk::Present { sensitivity: s } = r {
                s.validate().map_err(|e| error(e.to_string()))?;
                if s.snapshot_id != snapshot_id
                    || s.position.currency != t.currency
                    || position.as_ref().is_some_and(|p| p != &s.position)
                    || pv.is_some_and(|v| v != s.base_pv)
                {
                    return Err(error(
                        "mixed snapshot, monetary currency, position or original trade PV",
                    ));
                }
                position = Some(s.position.clone());
                pv = Some(s.base_pv);
            }
        }
        let ci = *currency_rows.entry(t.currency).or_insert_with(|| {
            coverage.push(SignedRiskCoverage {
                currency: t.currency,
                trades: 0,
                covered_trades: 0,
                required_factors: 0,
                covered_factors: 0,
                known_gross_exposure: ExactDecimal::new(Decimal::ZERO),
                covered_known_gross_exposure: ExactDecimal::new(Decimal::ZERO),
                unknown_exposure_trades: 0,
                status: SignedRiskCoverageStatus::Complete,
            });
            coverage.len() - 1
        });
        let c = &mut coverage[ci];
        c.trades += 1;
        c.required_factors += t.required_factors.len();
        let mut complete = true;
        for key in &t.required_factors {
            let i = *rows.entry((key.clone(), t.currency)).or_insert_with(|| {
                local.push(SignedRiskAggregate {
                    key: key.clone(),
                    currency: t.currency,
                    known_value: None,
                    status: SignedRiskCoverageStatus::Complete,
                    missing_trades: Vec::new(),
                });
                local.len() - 1
            });
            match actual.get(key) {
                Some(TradeFactorRisk::Present { sensitivity: s }) => {
                    local[i].known_value = add(local[i].known_value, s.value)?;
                    c.covered_factors += 1;
                }
                outcome => {
                    complete = false;
                    let reason = match outcome {
                        Some(TradeFactorRisk::Missing { reason, .. })
                            if !reason.trim().is_empty() && reason.len() <= 1024 =>
                        {
                            reason.clone()
                        }
                        Some(_) => {
                            return Err(error("bounded nonempty missing-risk reason required"))
                        }
                        None => "required trade factor result absent".into(),
                    };
                    local[i].status = SignedRiskCoverageStatus::Incomplete;
                    local[i].missing_trades.push(t.trade_id.clone());
                    missing.push(MissingSignedRisk {
                        trade_id: t.trade_id.clone(),
                        key: key.clone(),
                        reason,
                    });
                }
            }
        }
        if complete {
            c.covered_trades += 1;
        }
        if let Some(g) = t.gross_exposure {
            if g.decimal() < Decimal::ZERO {
                return Err(error(
                    "gross exposure must be nonnegative or explicitly unknown",
                ));
            }
            c.known_gross_exposure = ExactDecimal::new(
                c.known_gross_exposure
                    .decimal()
                    .checked_add(g.decimal())
                    .ok_or_else(|| error("gross exposure overflow"))?,
            );
            if complete {
                c.covered_known_gross_exposure = ExactDecimal::new(
                    c.covered_known_gross_exposure
                        .decimal()
                        .checked_add(g.decimal())
                        .ok_or_else(|| error("covered gross exposure overflow"))?,
                );
            }
        } else {
            c.unknown_exposure_trades += 1;
        }
        if !complete || c.unknown_exposure_trades > 0 {
            c.status = SignedRiskCoverageStatus::Incomplete;
        }
    }
    let mut reporting = Vec::<SignedRiskAggregate>::new();
    let mut report_rows = HashMap::new();
    let mut missing_fx = Vec::new();
    let mut fx_conversion_exposure = Vec::new();
    for l in &local {
        let i = *report_rows.entry(l.key.clone()).or_insert_with(|| {
            reporting.push(SignedRiskAggregate {
                key: l.key.clone(),
                currency: reporting_currency,
                known_value: None,
                status: SignedRiskCoverageStatus::Complete,
                missing_trades: Vec::new(),
            });
            reporting.len() - 1
        });
        let r = &mut reporting[i];
        if l.status == SignedRiskCoverageStatus::Incomplete {
            r.status = SignedRiskCoverageStatus::Incomplete;
            r.missing_trades.extend(l.missing_trades.clone());
        }
        if l.currency == reporting_currency {
            if let Some(v) = l.known_value {
                r.known_value = add(r.known_value, v)?;
            }
        } else if let Some(pin) = fx_map.get(&l.currency) {
            use rust_decimal::prelude::ToPrimitive;
            let rate = pin
                .rate
                .decimal()
                .to_f64()
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or_else(|| error("finite FX conversion required"))?;
            if let Some(v) = l.known_value {
                r.known_value = add(r.known_value, v * rate)?;
            }
            fx_conversion_exposure.push(RiskFxConversionExposure {
                key: l.key.clone(),
                fx: (*pin).clone(),
                local_sensitivity: l.known_value,
                status: l.status,
            });
        } else {
            r.status = SignedRiskCoverageStatus::Incomplete;
            if !missing_fx.contains(&l.currency) {
                missing_fx.push(l.currency);
            }
            r.missing_trades.extend(
                trades
                    .iter()
                    .filter(|t| t.currency == l.currency && t.required_factors.contains(&l.key))
                    .map(|t| t.trade_id.clone()),
            );
        }
    }
    let status = if coverage
        .iter()
        .all(|c| c.status == SignedRiskCoverageStatus::Complete)
        && missing_fx.is_empty()
    {
        SignedRiskCoverageStatus::Complete
    } else {
        SignedRiskCoverageStatus::Incomplete
    };
    Ok(SignedPortfolioRisk {
        snapshot_id,
        scope,
        status,
        coverage,
        local,
        reporting,
        missing,
        missing_fx,
        fx_conversion_exposure,
    })
}
