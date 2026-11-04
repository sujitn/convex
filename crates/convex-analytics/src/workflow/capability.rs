//! Capability queries describe native dispatch, never market/release certification.
use super::*;

/// Exact construction resource requirements, retaining native identity types.
#[derive(Debug, Clone)]
pub enum ConstructionRequirements {
    /// Four actual bond calendar roles; floating/optional roles remain distinct.
    Bond(convex_bonds::contracts::BondCalendarIds),
    /// Independent fixed/floating/reset/observation calendar revisions.
    Swap(Vec<convex_rates::ReferencePin>),
    /// Premium and settlement calendar revisions.
    Cds(Vec<convex_credit::ReferencePin>),
    /// Delivery or accrual calendar plus the actual physical basket requirement.
    Future {
        /// Actual calendar revision.
        calendar: convex_core::futures::ResourcePin,
        /// Actual basket pin for bond futures.
        basket: Option<convex_core::futures::ResourcePin>,
    },
}
/// Enumerate complete construction resources without building or looking them up.
pub fn construction_requirements(input: &ConstructionInput<'_>) -> ConstructionRequirements {
    match input {
        ConstructionInput::Bond { terms, .. } => {
            ConstructionRequirements::Bond(terms.calendars.clone())
        }
        ConstructionInput::Swap { terms, .. } => {
            let mut pins = vec![
                terms.fixed.leg.calendars.accrual.clone(),
                terms.fixed.leg.calendars.payment.clone(),
                terms.floating.leg().calendars.accrual.clone(),
                terms.floating.leg().calendars.payment.clone(),
            ];
            match &terms.floating {
                convex_rates::FloatingLegTerms::Term(t) => {
                    pins.push(t.resets.reset_calendar.clone());
                    pins.push(t.resets.fixing_calendar.clone());
                }
                convex_rates::FloatingLegTerms::Overnight(t) => {
                    pins.push(t.observation_calendar.clone())
                }
            };
            ConstructionRequirements::Swap(pins)
        }
        ConstructionInput::Cds { terms, .. } => ConstructionRequirements::Cds(vec![
            terms.premium_calendar.clone(),
            terms.settlement_calendar.clone(),
        ]),
        ConstructionInput::Future(terms) => match &terms.settlement {
            convex_core::futures::FuturesSettlement::Bond { delivery, .. } => {
                ConstructionRequirements::Future {
                    calendar: delivery.calendar.clone(),
                    basket: Some(delivery.basket.clone()),
                }
            }
            convex_core::futures::FuturesSettlement::Rate { accrual, .. } => {
                let calendar = match accrual {
                    convex_core::futures::RateAccrual::Overnight { calendar, .. }
                    | convex_core::futures::RateAccrual::Term { calendar, .. } => calendar.clone(),
                };
                ConstructionRequirements::Future {
                    calendar,
                    basket: None,
                }
            }
        },
    }
}
/// Potential exact curve roles declared by the product. This does not assert
/// consumption: full native results record actually consumed curves/history.
pub fn declared_curve_requirements(input: &ValuationInputs<'_>) -> WorkflowResult<Vec<CurveKey>> {
    Ok(match input {
        ValuationInputs::Bond { method, .. } => match method {
            BondPriceMethod::Yield { .. } => vec![],
            BondPriceMethod::DiscountSpread { curve, .. } => vec![curve.clone()],
        },
        ValuationInputs::FloatingBond {
            discount,
            projection,
            ..
        } => std::iter::once(discount.clone())
            .chain(projection.iter().cloned())
            .collect(),
        ValuationInputs::Swap { contract, .. } => vec![
            contract.terms().collateral.discount_curve.clone(),
            contract.terms().floating.projection_curve().clone(),
        ],
        ValuationInputs::Cds { contract, .. } => vec![
            contract.terms().discount_curve.clone(),
            contract.terms().survival_curve.clone(),
        ],
        ValuationInputs::RateFuture { inputs, .. } => inputs
            .projection
            .iter()
            .map(|p| p.curve.recipe().key.clone())
            .collect(),
        ValuationInputs::BondFuture(_) => vec![],
        ValuationInputs::OptionalBond { .. } => {
            return Err(WorkflowError::NotApplicable(
                "prepared optional input has already consumed exact curves; inspect native model price for original discount/projection keys",
            ));
        }
    })
}
/// Closed set of workflow operations; economic inapplicability is explicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowOperation {
    /// Native full price, with PV/quote-level meaning.
    Price,
    /// Pure expected cash, not recorded payment.
    Cashflows,
    /// Fixed full dated-cash yield inversion.
    BondYield,
    /// Constant full fixed-cash Z-spread inversion.
    BondSpread,
    /// Full floating forecast discounted at a frozen-projection margin.
    DiscountMargin,
    /// Uniform IRS/OIS fixed fair rate.
    SwapFairRate,
    /// Standard running/clean/dirty credit conversion.
    CdsConversion,
    /// Original fixed-model HW1F OAS inverse.
    OptionalOas,
    /// Actual remaining accrual equivalent rate.
    RemainingFutureRate,
    /// Actual graph quote/node full-revaluation risk.
    CurveQuoteRisk,
    /// Original native optional and all-member futures factor risk.
    NativeModelRisk,
}
/// Native route availability; every supported route still validates all inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowCapabilityStatus {
    /// A public native implementation exists for these inputs; not a certificate.
    Available,
    /// This measure has no meaning for this product/representation.
    NotApplicable,
    /// Use the original native model-specific route with its full guard contract.
    Unsupported,
}
/// Query result includes the applicable public route or economic explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowCapability {
    /// Requested operation.
    pub operation: WorkflowOperation,
    /// Native route status, independent of input admission/certification.
    pub status: WorkflowCapabilityStatus,
    /// Stable public route or explicit reason.
    pub reason: &'static str,
}
/// Query actual workflow dispatch; never turn NotApplicable into a numeric zero.
pub fn capability(input: &ValuationInputs<'_>, operation: WorkflowOperation) -> WorkflowCapability {
    use WorkflowCapabilityStatus as S;
    use WorkflowOperation as O;
    let available = match (input, operation) {
        (_, O::Price) => Some("workflow::price"),
        (
            ValuationInputs::Bond { .. } | ValuationInputs::FloatingBond { .. }
            | ValuationInputs::Swap { .. } | ValuationInputs::Cds { .. },
            O::Cashflows,
        ) => Some("workflow::cashflows"),
        (ValuationInputs::Bond { .. }, O::BondYield) => Some("workflow::solve_bond_yield"),
        (ValuationInputs::Bond { .. }, O::BondSpread) => Some("workflow::solve_bond_spread"),
        (ValuationInputs::FloatingBond { .. }, O::DiscountMargin) => {
            Some("workflow::solve_bond_spread on prepared full projected cash")
        }
        (ValuationInputs::Swap { .. }, O::SwapFairRate) => Some("workflow::inverse / SwapFairRate"),
        (ValuationInputs::Cds { .. }, O::CdsConversion) => Some("workflow::inverse / Cds"),
        (ValuationInputs::OptionalBond { .. }, O::OptionalOas) => Some("workflow::inverse / OptionalOas"),
        (ValuationInputs::RateFuture { .. }, O::RemainingFutureRate) => {
            Some("workflow::inverse / RateFuture")
        }
        (
            ValuationInputs::Bond { method: BondPriceMethod::DiscountSpread { .. }, .. }
            | ValuationInputs::FloatingBond { .. } | ValuationInputs::Swap { .. }
            | ValuationInputs::Cds { .. } | ValuationInputs::RateFuture { .. },
            O::CurveQuoteRisk,
        ) => Some("workflow::curve_quote_risk"),
        (ValuationInputs::OptionalBond { .. }, O::NativeModelRisk) => {
            Some("optional_bond::PreparedOptionalBond::risk / quote_risk::optional_oas_quote_sensitivity")
        }
        (ValuationInputs::BondFuture(_), O::NativeModelRisk) => Some("futures::bond_futures_risk"),
        _ => None,
    };
    let (status, reason) = if let Some(route) = available {
        (S::Available, route)
    } else if operation == O::CurveQuoteRisk
        && matches!(
            input,
            ValuationInputs::OptionalBond { .. } | ValuationInputs::BondFuture(_)
        )
    {
        (S::Unsupported, "model-specific curve/CTD risk requires original native model guard and all-member inputs")
    } else {
        (S::NotApplicable, "operation has no applicable route for this product/representation; inspect full native cash/model diagnostics")
    };
    WorkflowCapability {
        operation,
        status,
        reason,
    }
}
