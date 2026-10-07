//! Pure valuation of actual supplied bond contractual cash.
//!
//! Floating projection and discounting are distinct exact curve roles. Optional
//! terms cannot be valued as a straight bond. All prices are per ORIGINAL face;
//! factor/current-outstanding quotation is a separate explicit convention.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use convex_bonds::contracts::{
    BondContract, BondContractError, BondProjection, BondProjectionContext, BondProjectionPolicy,
    ProjectionRequest,
};
use convex_core::{
    cashflows::{CashflowError, PaymentDateEntitlement},
    contracts::{IndexId, UtcTimestamp},
    fixings::FixingSeries,
    types::Compounding,
    Date,
};
use convex_curves::contracts::{BuiltCurve, CurveKey, CurvePoint, CurveQuery, CurveRole};
use rust_decimal::{prelude::ToPrimitive, Decimal};

/// Expected-cash PV from explicitly supplied curves. This is a model quote,
/// distinct from an observed trader price or an executable market quote.
#[derive(Debug, Clone)]
pub struct ContractPrice {
    /// Absolute discounted entitled cash in contract currency.
    pub present_value: f64,
    /// Clean price per 100 original face.
    pub clean_price: f64,
    /// Dirty price per 100 original face.
    pub dirty_price: f64,
    /// Accrued/rebate per 100 original face.
    pub accrued: f64,
    /// Actual contractual cash and fixing origins consumed.
    pub projection: BondProjection,
    /// Exact discount version, not a fabricated/rebuilt proxy.
    pub discount_curve: CurveKey,
}

fn error(field: &'static str, reason: impl Into<String>) -> BondContractError {
    CashflowError::InvalidTerms {
        field,
        reason: reason.into(),
    }
    .into()
}
fn number(value: Decimal) -> Result<f64, BondContractError> {
    value
        .to_f64()
        .filter(|x| x.is_finite())
        .ok_or_else(|| error("cash", "nonfinite decimal conversion"))
}

/// Fixed/stepped/amortizing model price on exact declared entitled cash.
pub fn price_fixed(
    bond: &BondContract,
    settlement: Date,
    on_payment: PaymentDateEntitlement,
    discount: &BuiltCurve,
) -> Result<ContractPrice, BondContractError> {
    bond.require_plain_valuation()?;
    let projection = bond.fixed_projection(settlement, on_payment)?;
    discounted(bond, settlement, projection, discount)
}

/// Plain term/overnight model price. The history/publication and forecast policy
/// are caller-supplied and are never inferred from the discount curve.
pub fn price_floating<P>(
    bond: &BondContract,
    context: &BondProjectionContext,
    history: &FixingSeries,
    publication: P,
    policy: BondProjectionPolicy<'_>,
    discount: &BuiltCurve,
) -> Result<ContractPrice, BondContractError>
where
    P: FnMut(&IndexId, Date) -> Result<UtcTimestamp, CashflowError>,
{
    bond.require_plain_valuation()?;
    let projection = bond.project(context, history, publication, policy)?;
    discounted(bond, context.settlement, projection, discount)
}

/// Explicit projection-curve policy using the ACTUAL requested reset/overnight
/// interval and native simple forward quotation. Currency and exact index role
/// must match. Missing eligible history is rejected before this is called.
pub fn project_from_curve(
    request: &ProjectionRequest,
    currency: convex_core::Currency,
    projection: &BuiltCurve,
) -> Result<Decimal, CashflowError> {
    let fail = |reason: String| CashflowError::InvalidTerms {
        field: "projection_curve",
        reason,
    };
    if projection.recipe().currency != currency
        || !matches!(&projection.recipe().role,
        CurveRole::Projection { index_id } if index_id == request.index_id.as_str())
    {
        return Err(fail("currency/index/projection role mismatch".into()));
    }
    let query = projection
        .query(&CurveQuery::ForwardRate {
            start: CurvePoint::Date(request.rate_start),
            end: CurvePoint::Date(request.rate_end),
            compounding: Compounding::Simple,
            day_count: request.day_count,
        })
        .map_err(|e| fail(e.to_string()))?;
    Decimal::try_from(query.value)
        .map_err(|_| fail("nonfinite/unrepresentable projected rate".into()))
}

fn discounted(
    bond: &BondContract,
    settlement: Date,
    projection: BondProjection,
    discount: &BuiltCurve,
) -> Result<ContractPrice, BondContractError> {
    if discount.recipe().currency != bond.terms().currency
        || !matches!(&discount.recipe().role,
        CurveRole::Discount { collateral_currency } if *collateral_currency == bond.terms().currency)
    {
        return Err(error(
            "discount_curve",
            "currency/collateral/discount role mismatch",
        ));
    }
    let df = |date| {
        discount
            .query(&CurveQuery::DiscountFactor {
                point: CurvePoint::Date(date),
            })
            .map(|q| q.value)
            .map_err(|e| error("discount_curve", e.to_string()))
    };
    discounted_projection(
        bond,
        settlement,
        projection,
        discount.recipe().key.clone(),
        df,
        0.0,
    )
}

/// Price complete supplied fixed cash against an explicitly named government
/// benchmark. A government curve is never admitted by the collateral-only API.
pub fn price_fixed_government(
    bond: &BondContract,
    settlement: Date,
    on_payment: PaymentDateEntitlement,
    government: &BuiltCurve,
    benchmark_id: &str,
) -> Result<ContractPrice, BondContractError> {
    bond.require_plain_valuation()?;
    if government.recipe().currency != bond.terms().currency
        || !matches!(&government.recipe().role,
            CurveRole::Government { benchmark_id: actual } if actual == benchmark_id)
    {
        return Err(error(
            "government_curve",
            "currency/benchmark/government role mismatch",
        ));
    }
    let projection = bond.fixed_projection(settlement, on_payment)?;
    discounted_projection(
        bond,
        settlement,
        projection,
        government.recipe().key.clone(),
        |date| {
            government
                .query(&CurveQuery::DiscountFactor {
                    point: CurvePoint::Date(date),
                })
                .map(|q| q.value)
                .map_err(|e| error("government_curve", e.to_string()))
        },
        0.0,
    )
}

// One normalized full cash calculation for collateral, government and the
// explicitly declared cash-backed issuer model. Recovery is absolute settlement
// PV supplied only by the owning credit model, never inferred here.
pub(crate) fn discounted_projection(
    bond: &BondContract,
    settlement: Date,
    projection: BondProjection,
    reference: CurveKey,
    mut df: impl FnMut(Date) -> Result<f64, BondContractError>,
    recovery_pv: f64,
) -> Result<ContractPrice, BondContractError> {
    let settlement_df = df(settlement)?;
    let mut pv = 0.0;
    for flow in projection.cashflows()? {
        let value = number(flow.amount)? * df(flow.date)? / settlement_df;
        pv += value;
        if !value.is_finite() || !pv.is_finite() {
            return Err(error("present_value", "nonfinite cash discounting"));
        }
    }
    pv += recovery_pv;
    let face = number(bond.terms().face_value)?;
    let accrued = number(projection.accrued)? / face * 100.0;
    let dirty_price = pv / face * 100.0;
    let clean_price = dirty_price - accrued;
    if !dirty_price.is_finite() || !clean_price.is_finite() || !accrued.is_finite() {
        return Err(error("price", "nonfinite normalization"));
    }
    Ok(ContractPrice {
        present_value: pv,
        clean_price,
        dirty_price,
        accrued,
        projection,
        discount_curve: reference,
    })
}
