//! Thin full-term coupon option dispatch over actual immutable supplied resources.
use super::{inputs::ResolvedInputs, *};
use convex_analytics::floating_coupon as c;
use convex_bonds::contracts::BondProjectionContext;
use convex_core::{cashflows::CashflowError, fixings::MAX_FIXING_REQUIREMENTS};

/// Full economics and explicit segment payoff/model sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TermCouponOptionInput {
    /// Original complete bond and exact calendar revisions.
    pub bond: BondInput,
    /// Actual settlement, entitlement, history eligibility and information cutoff.
    pub context: BondProjectionContext,
    /// Actual independent index projection, required only for permitted forecasts.
    pub projection: Option<CurveKey>,
    /// Actual same-currency collateral discount source.
    pub discount: CurveKey,
    /// Full original segment bounds and volatility/model revisions.
    pub options: c::TermCouponOptionInputs,
}

pub(super) fn execute(
    input: &TermCouponOptionInput,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let bond = r.bond(&input.bond)?;
    let requirements = bond.fixing_requirements(
        input.context.settlement,
        input.context.on_payment,
        r.publications.callback(),
    )?;
    let mut missing = Vec::new();
    let mut projected = false;
    for batch in requirements.chunks(MAX_FIXING_REQUIREMENTS) {
        let resolution = r.history.resolve(batch, input.context.fixing)?;
        missing.extend_from_slice(resolution.missing());
        projected |= resolution.projected_requirements().next().is_some();
    }
    if !missing.is_empty() {
        return Err(CashflowError::MissingFixings(missing).into());
    }
    let projection = match &input.projection {
        Some(key) => Some(r.curves.get(key)?),
        None if projected => {
            return Err(LibraryError::missing(
                "projection curve",
                serde_json::json!({"index":bond.terms().coupon}),
            ))
        }
        None => None,
    };
    if let Some(curve) = projection {
        let index = match &bond.terms().coupon {
            convex_bonds::contracts::BondCoupon::Term(terms) => terms.index_id.as_str(),
            _ => {
                return Err(LibraryError::invalid(
                    "projection",
                    "actual term index required",
                ))
            }
        };
        if curve.recipe().currency != bond.terms().currency
            || !matches!(&curve.recipe().role, CurveRole::Projection { index_id } if index_id == index)
        {
            return Err(LibraryError::invalid(
                "projection",
                "original index/currency/projection role mismatch",
            ));
        }
    }
    let discount = r.curves.get(&input.discount)?;
    let result = c::price_term_coupon_options(
        c::TermCouponPricing {
            bond: &bond,
            context: &input.context,
            history: &r.history,
            discount,
            options: &input.options,
        },
        r.publications.callback(),
        |request| {
            let curve = projection.ok_or_else(|| CashflowError::InvalidTerms {
                field: "projection",
                reason: "original required curve absent".into(),
            })?;
            convex_analytics::bond_contract::project_from_curve(
                request,
                bond.terms().currency,
                curve,
            )
        },
    )?;
    value(result)
}

pub(super) fn with_prepared<T>(
    input: &TermCouponOptionInput,
    r: &ResolvedInputs<'_>,
    curves: &convex_curves::contracts::CurveSet,
    f: impl FnOnce(&c::PreparedTermCouponModel<'_>) -> LibraryResult<T>,
) -> LibraryResult<T> {
    let bond = r.bond(&input.bond)?;
    let mut missing_publication = None;
    let prepared = c::prepare_term_coupon_model(
        c::TermCouponModelInputs {
            bond: &bond,
            context: &input.context,
            history: &r.history,
            options: &input.options,
            projection: input.projection.clone(),
            discount: input.discount.clone(),
        },
        curves,
        |index, date| match r.publications.get(index, date) {
            Ok(time) => Ok(time),
            Err(error) => {
                missing_publication = Some(LibraryError::missing(
                    "publication",
                    serde_json::json!({"index_id":index,"observation_date":date,"cause":error.to_string()}),
                ));
                Err(CashflowError::InvalidTerms {
                    field: "publication",
                    reason: "original required publication absent".into(),
                })
            }
        },
    );
    if let Some(error) = missing_publication {
        return Err(error);
    }
    let prepared = prepared?;
    f(&prepared)
}
