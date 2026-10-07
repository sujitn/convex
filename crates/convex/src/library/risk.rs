//! Original complete quantity/risk/coverage guards remain native.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::{quote_risk as q, workflow as w};
use convex_core::contracts::SnapshotId;
use serde::{Deserialize, Serialize};

pub(super) fn bond_quote_risk(
    cash: &w::PreparedBondCash,
    method: &BondMethod,
    key: &q::QuoteRiskKey,
    position: &q::RiskPosition,
    snapshot: &SnapshotId,
    settings: &q::QuoteRiskSettings,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let native_method = match method {
        BondMethod::Yield {
            decimal_rate,
            convention,
        } => w::BondPriceMethod::Yield {
            decimal_rate: *decimal_rate,
            convention: *convention,
        },
        BondMethod::DiscountSpread {
            curve,
            role,
            decimal_spread,
        } => w::BondPriceMethod::DiscountSpread {
            curve: curve.clone(),
            role: role.native(),
            decimal_spread: *decimal_spread,
        },
    };
    w::ValuationInputs::Bond {
        cash,
        method: native_method,
        market: r.market(),
    }
    .validate_position(position)?;
    let definition = key.definition();
    let quote = match method {
        BondMethod::Yield { .. } => convex_core::contracts::QuoteType::Yield,
        BondMethod::DiscountSpread { .. } => convex_core::contracts::QuoteType::ZSpread,
    };
    if definition.factor.definition().quote_type != quote
        || definition.factor.definition().currency != cash.currency()
        || !matches!(definition.coordinate, q::RiskCoordinate::Raw { .. })
        || definition.response != q::RiskResponse::FrozenCurves
        || definition.native_unit != QuoteUnit::DecimalRate
    {
        return Err(LibraryError::invalid(
            "bond quote risk",
            "actual raw decimal yield/Z-or-frozen-DM factor and native currency required",
        ));
    }
    let mut failure = None;
    let result = q::reference_quote_sensitivity(
        key.clone(),
        position.clone(),
        snapshot.clone(),
        settings.clone(),
        |delta| {
            let changed = match method {
                BondMethod::Yield {
                    decimal_rate,
                    convention,
                } => BondMethod::Yield {
                    decimal_rate: decimal_rate + delta,
                    convention: *convention,
                },
                BondMethod::DiscountSpread {
                    curve,
                    role,
                    decimal_spread,
                } => BondMethod::DiscountSpread {
                    curve: curve.clone(),
                    role: role.clone(),
                    decimal_spread: decimal_spread + delta,
                },
            };
            super::products::price_bond(cash, &changed, r)
                .map(|v| v.present_value)
                .map_err(|e| {
                    let reason = e.to_string();
                    failure = Some(e);
                    q::QuoteRiskError::Invalid {
                        field: "original bond price".into(),
                        reason,
                    }
                })
        },
    );
    if let Some(error) = failure {
        return Err(error);
    }
    value(result?)
}

/// Full product data for original graph revaluation. Model-specific optional
/// and bond futures use their original dedicated risk APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RiskProductInput {
    /// Fixed/stepped/amortizing or floating complete native cash price.
    Bond {
        /// Full original contract and calendar revisions.
        bond: Box<BondInput>,
        /// Original fixing/settlement/entitlement state.
        context: convex_bonds::contracts::BondProjectionContext,
        /// Original explicit projection role.
        projection: Option<CurveKey>,
        /// Original yield/reference representation.
        method: BondMethod,
    },
    /// Original full IRS/OIS trade.
    Swap {
        /// Complete original dated/reset/collateral economics.
        terms: Box<convex_rates::SwapTerms>,
        /// Frozen actual eligible history and payment ownership.
        context: convex_rates::CashflowContext,
    },
    /// Original full single-name CDS trade.
    Cds {
        /// Actual full law/entity/recovery/curve/dates.
        terms: Box<convex_credit::CdsTerms>,
        /// Actual surviving/initial payment state.
        context: convex_credit::CdsPricingContext,
    },
    /// Actual full rate futures and clearing/history/source economics.
    RateFuture {
        /// Full original inputs, no tenor proxy.
        input: Box<RateFutureInput>,
    },
}
fn with_product<T>(
    product: &RiskProductInput,
    r: &ResolvedInputs<'_>,
    f: impl FnOnce(&w::ValuationInputs<'_>) -> LibraryResult<T>,
) -> LibraryResult<T> {
    match product {
        RiskProductInput::Bond {
            bond,
            context,
            projection,
            method,
        } => {
            let contract = r.bond(bond)?;
            match &contract.terms().coupon {
                convex_bonds::contracts::BondCoupon::Fixed => {
                    let cash =
                        super::products::prepare_bond(bond, context, projection.as_ref(), r)?;
                    let method = match method {
                        BondMethod::Yield {
                            decimal_rate,
                            convention,
                        } => w::BondPriceMethod::Yield {
                            decimal_rate: *decimal_rate,
                            convention: *convention,
                        },
                        BondMethod::DiscountSpread {
                            curve,
                            role,
                            decimal_spread,
                        } => w::BondPriceMethod::DiscountSpread {
                            curve: curve.clone(),
                            role: role.native(),
                            decimal_spread: *decimal_spread,
                        },
                    };
                    f(&w::ValuationInputs::Bond {
                        cash: &cash,
                        method,
                        market: r.market(),
                    })
                }
                _ => {
                    let BondMethod::DiscountSpread {
                        curve,
                        role,
                        decimal_spread,
                    } = method
                    else {
                        return Err(LibraryError {
                            code: LibraryErrorCode::NotApplicable,
                            message: "floating cash has no contractual fixed-cash yield".into(),
                            details: Value::Null,
                        });
                    };
                    if !matches!(role, BondReferenceRole::Collateral {}) {
                        return Err(LibraryError::invalid(
                            "floating discount role",
                            "native floating workflow requires collateral",
                        ));
                    }
                    f(&w::ValuationInputs::FloatingBond {
                        contract: &contract,
                        context,
                        discount: curve.clone(),
                        projection: projection.clone(),
                        discount_margin: *decimal_spread,
                        market: r.market(),
                    })
                }
            }
        }
        RiskProductInput::Swap { terms, context } => {
            let contract = terms.as_ref().clone().build(&r.swap_calendars(terms)?)?;
            f(&w::ValuationInputs::Swap {
                contract: &contract,
                context: *context,
                market: r.market(),
            })
        }
        RiskProductInput::Cds { terms, context } => {
            let contract = terms.as_ref().clone().build(&r.credit_calendars(terms)?)?;
            f(&w::ValuationInputs::Cds {
                contract: &contract,
                context,
                curves: &r.curves,
            })
        }
        RiskProductInput::RateFuture { input } => {
            let history = convex_analytics::futures::FuturesFixings::new(
                r.fixing_observations().to_vec(),
                input.fixing_selection,
            )?;
            let inputs = super::products::rate_future(input, r, &history)?;
            f(&w::ValuationInputs::RateFuture {
                inputs: &inputs,
                publications: &r.publications,
            })
        }
    }
}
pub(super) fn graph_risk(
    product: &RiskProductInput,
    graph: &q::PreparedQuoteRiskGraph<'_>,
    key: q::QuoteRiskKey,
    position: q::RiskPosition,
    snapshot_id: SnapshotId,
    settings: q::QuoteRiskSettings,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let original = r.with_curves(graph.baseline().curves.clone());
    with_product(product, &original, |p| {
        value(w::curve_quote_risk(
            p,
            graph,
            key,
            position,
            snapshot_id,
            settings,
        )?)
    })
}
pub(super) fn graph_price(
    product: &RiskProductInput,
    baseline: &q::QuoteGraphScenario,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let original = r.with_curves(baseline.curves.clone());
    with_product(product, &original, |p| value(w::price(p)?))
}
pub(super) fn graph_scenario(
    product: &RiskProductInput,
    baseline: &q::QuoteGraphScenario,
    shocked: &q::QuoteGraphScenario,
    position: q::RiskPosition,
    snapshot_id: SnapshotId,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let original = r.with_curves(baseline.curves.clone());
    with_product(product, &original, |p| {
        value(w::revalue_curves(
            p,
            &shocked.curves,
            position,
            snapshot_id,
        )?)
    })
}
/// Complete expected portfolio/FX manifest; missing risk and unknown gross
/// remain visible and cannot become successful empty/zero aggregates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AggregateInput {
    /// Every expected trade, factor outcome and gross exposure.
    pub trades: Vec<convex_portfolio::signed_risk::TradeSignedRisk>,
    /// Actual immutable common snapshot.
    pub snapshot_id: SnapshotId,
    /// Explicit entity scope; this calculation confers no authorization.
    pub scope: convex_portfolio::signed_risk::RiskEntityScope,
    /// Actual reporting currency.
    pub reporting_currency: Currency,
    /// Original same-snapshot reporting FX source/revision pins.
    pub fx: Vec<convex_portfolio::signed_risk::RiskFxPin>,
}
pub(super) fn aggregate(input: &AggregateInput) -> LibraryResult<Value> {
    value(
        convex_portfolio::signed_risk::aggregate_signed_quote_risk(
            &input.trades,
            input.snapshot_id.clone(),
            input.scope.clone(),
            input.reporting_currency,
            &input.fx,
        )
        .map_err(|e| LibraryError {
            code: LibraryErrorCode::InvalidInput,
            message: e.to_string(),
            details: Value::Null,
        })?,
    )
}
