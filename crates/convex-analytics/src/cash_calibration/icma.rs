//! Contextual helper composition; cash, time, price and inverse have one owner.
use super::*;
use crate::workflow as w;
use std::cell::Cell;

pub(super) struct PreparedIcmaHelper {
    cash: w::PreparedIcmaQuotation,
    cash_count: u64,
    used: Cell<u64>,
    limit: u64,
    inverse: w::BondInverseSettings,
}
impl PreparedIcmaHelper {
    pub(super) fn work(&self) -> IcmaHelperWork {
        IcmaHelperWork {
            reserved_reference_evaluations: self.cash.reserved_reference_evaluations(),
            reserved_cash_evaluations: self.used.get(),
            max_cash_evaluations: self.limit,
        }
    }
    pub(super) fn new(
        bond: &convex_bonds::contracts::BondContract,
        helper: &CashBondHelper,
        input: &IcmaCashYield,
        resources: &CashHelperResources<'_>,
    ) -> CashCalibrationResult<Self> {
        let CashHelperResources::IcmaBond {
            calendars, pins, ..
        } = resources
        else {
            return Err(invalid(
                "ICMA resources",
                "actual independently versioned calendars required",
            ));
        };
        if *pins != input.sources.calendars
            || input.sources.security.id.as_str() != helper.id
            || input.sources.quote_revision.version != helper.quote_version
            || helper.credit_identity.as_ref().is_some_and(|identity| {
                identity.instrument_id.as_str() != helper.id
                    || identity.instrument_version != input.sources.security.version
            })
        {
            return Err(invalid(
                "ICMA sources",
                "exact original security/quote/calendar revisions required",
            ));
        }
        input.inverse.validate(10.0)?;
        let rate = helper.quote.decimal_rate()?;
        if rate < input.inverse.lower || rate > input.inverse.upper {
            return Err(invalid(
                "ICMA yield quote",
                "outside original closed inverse bracket",
            ));
        }
        if input.max_cash_evaluations == 0
            || input.max_cash_evaluations > MAX_ICMA_HELPER_CASH_EVALUATIONS
        {
            return Err(CurveContractError::BoundExceeded("ICMA helper cash evaluations").into());
        }
        let cash = w::PreparedIcmaQuotation::new(
            &w::IcmaQuotationInputs {
                contract: bond,
                sources: &input.sources,
                reference_context: &input.reference_context,
                convention: &input.convention,
                position: &input.position,
                exercise_calendar: (&bond.terms().calendars.exercise, calendars.exercise),
                purchase_calendar: (&bond.terms().calendars.payment, calendars.payment),
            },
            &w::IcmaCashSelection::Plain {
                settlement: helper.settlement,
                on_payment: helper.on_payment,
            },
        )?;
        let cash_count =
            cash.cashflow_count()?
                .checked_add(2)
                .ok_or(CurveContractError::BoundExceeded(
                    "ICMA helper cash evaluations",
                ))?;
        Ok(Self {
            cash,
            cash_count,
            used: Cell::new(0),
            limit: input.max_cash_evaluations,
            inverse: input.inverse,
        })
    }
    fn reserve(&self, passes: u64) -> CashCalibrationResult<()> {
        let next = self
            .cash_count
            .checked_mul(passes)
            .and_then(|cost| self.used.get().checked_add(cost))
            .filter(|total| *total <= self.limit)
            .ok_or(CurveContractError::BoundExceeded(
                "ICMA helper cash evaluations",
            ))?;
        self.used.set(next);
        Ok(())
    }
    pub(super) fn observed(
        &self,
        helper: &CashBondHelper,
    ) -> CashCalibrationResult<w::IcmaQuotation> {
        self.reserve(1)?;
        Ok(self.cash.quote_price(helper.quote.decimal_rate()?)?)
    }
    pub(super) fn reserve_model(&self) -> CashCalibrationResult<()> {
        // One original model cash price plus the unchanged inverse's upper work
        // reservation. No unused budget permits another bracket or retry.
        self.reserve(u64::from(self.inverse.max_iterations) + 9)
    }
    pub(super) fn model(&self, dirty: f64) -> CashCalibrationResult<w::IcmaQuotation> {
        Ok(self
            .cash
            .quote_yield(w::BondPriceTarget::Dirty(dirty), self.inverse)?)
    }
}

/// Explicit per-source declarations add up to the whole request's hard limits.
/// The mixed graph performs one extra admission pass before any family fits.
pub(super) fn check_request(
    inputs: &[CashCurveBuildInput<'_>],
    reference_passes: u64,
) -> CashCalibrationResult<()> {
    let mut reference = 0_u64;
    let mut cash = 0_u64;
    for input in inputs {
        if input.recipe.helpers.len() > MAX_CASH_HELPERS {
            return Err(CurveContractError::BoundExceeded("cash helpers").into());
        }
        for helper in &input.recipe.helpers {
            if let CashCalibrationHelper::Bond { input: helper } = helper {
                if let CashQuoteKind::ContextualIcmaYield { input } = &helper.quote_kind {
                    reference = reference
                        .checked_add(input.convention.max_reference_evaluations)
                        .ok_or(CurveContractError::BoundExceeded(
                            "total ICMA helper references",
                        ))?;
                    cash = cash.checked_add(input.max_cash_evaluations).ok_or(
                        CurveContractError::BoundExceeded("total ICMA helper cash evaluations"),
                    )?;
                }
            }
        }
    }
    if reference
        .checked_mul(reference_passes)
        .is_none_or(|v| v > w::MAX_ICMA_REFERENCE_EVALUATIONS)
        || cash > MAX_ICMA_HELPER_CASH_EVALUATIONS
    {
        return Err(
            CurveContractError::BoundExceeded("total ICMA helper work declarations").into(),
        );
    }
    Ok(())
}

pub(crate) fn preflight_icma_helpers(
    inputs: &[CashCurveBuildInput<'_>],
) -> CashCalibrationResult<()> {
    if !inputs.iter().flat_map(|i| &i.recipe.helpers).any(|helper| {
        matches!(helper, CashCalibrationHelper::Bond { input }
            if matches!(input.quote_kind, CashQuoteKind::ContextualIcmaYield { .. }))
    }) {
        return Ok(());
    }
    check_request(inputs, 2)?;
    for input in inputs {
        // Full source-order admission before ANY other graph family's fit.
        // Conversion and all numeric endpoints are deferred to the real build.
        super::build::admit(input)?;
    }
    Ok(())
}
