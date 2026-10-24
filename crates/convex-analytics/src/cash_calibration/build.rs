//! Whole-graph admission and deterministic all-or-error dependency construction.

use super::*;
use convex_curves::{ExtrapolationMethod, InterpolationMethod};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Prepared<'a> {
    pub recipe: &'a CashCurveRecipe,
    pub bonds: Vec<Option<pricing::PreparedBond>>,
}
pub(super) fn dependencies(model: Option<&BondBasisModel>) -> Vec<CurveKey> {
    match model {
        Some(BondBasisModel::ReferenceZeroSpread { reference, .. }) => vec![reference.clone()],
        Some(BondBasisModel::CreditParRecovery {
            discount, survival, ..
        }) => vec![discount.clone(), survival.clone()],
        None => vec![],
    }
}
fn settings(input: &CashCurveRecipe) -> CashCalibrationResult<()> {
    match (&input.curve.calibration, input.bootstrap) {
        (CalibrationMode::Ufr(s), None) => {
            s.validate_recipe(&input.curve)?;
            if input.basis_model.is_some() {
                return Err(invalid("UFR cash model", "Government/Repo continuous-zero model required; residual basis has a separate owner"));
            }
        }
        (CalibrationMode::Parametric(s), None) => {
            s.validate(input.helpers.len())?;
            if input.basis_model.is_some()
                || input.curve.interpolation != s.initial.method()
                || !matches!(
                    input.curve.extrapolation,
                    ExtrapolationMethod::None | ExtrapolationMethod::Flat
                )
            {
                return Err(invalid("parametric cash model", "exact NS/Svensson Government/Repo continuous-zero model and None/raw Flat required; residual basis has a separate owner"));
            }
        }
        (CalibrationMode::Piecewise, Some(s)) => {
            if !s.lower_parameter.is_finite()
                || !s.upper_parameter.is_finite()
                || s.lower_parameter >= s.upper_parameter
                || s.lower_parameter < -10.0
                || s.upper_parameter > 10.0
                || s.max_iterations == 0
                || s.max_iterations > 256
                || input.helpers.len() != input.pillars.len()
                || input.overlap != CashOverlapPolicy::Reject
            {
                return Err(invalid("piecewise settings", "closed finite bounded bracket,1..256 iterations and one original helper per pillar required"));
            }
            let mut dates: Vec<_> = input
                .helpers
                .iter()
                .map(CashCalibrationHelper::pillar)
                .collect();
            dates.sort();
            if dates != input.pillars {
                return Err(invalid(
                    "piecewise pillars",
                    "full last-relevant helper dates must equal the causal grid",
                ));
            }
        }
        (CalibrationMode::Global(s), None) => {
            if s.initial_zeros.len() != input.pillars.len()
                || s.quote_scales.len() != input.helpers.len()
                || s.weights.len() != input.helpers.len()
                || s.max_iterations == 0
                || s.max_iterations > 256
                || !s.objective_tolerance.is_finite()
                || s.objective_tolerance <= 0.0
                || s.objective_tolerance > 1.0
                || !s.lower_zero.is_finite()
                || !s.upper_zero.is_finite()
                || s.lower_zero >= s.upper_zero
                || s.lower_zero < -10.0
                || s.upper_zero > 10.0
                || s.initial_zeros
                    .iter()
                    .any(|v| !v.is_finite() || *v < s.lower_zero || *v > s.upper_zero)
                || s.quote_scales.iter().any(|v| !v.is_finite() || *v <= 0.0)
                || s.weights.iter().any(|v| !v.is_finite() || *v <= 0.0)
            {
                return Err(invalid("global settings", "matching bounded dimensions, finite starts/brackets and positive original scales/weights/objective required"));
            }
        }
        _ => {
            return Err(invalid(
                "calibration mode",
                "explicit Piecewise with bootstrap or Global without bootstrap required",
            ))
        }
    }
    Ok(())
}
pub(super) fn admit<'a>(input: &CashCurveBuildInput<'a>) -> CashCalibrationResult<Prepared<'a>> {
    let r = input.recipe;
    let c = &r.curve;
    c.key.validate()?;
    if r.helpers.is_empty()
        || r.helpers.len() > MAX_CASH_HELPERS
        || r.pillars.is_empty()
        || r.pillars.len() > MAX_CASH_HELPERS
        || r.helpers.len() != input.resources.len()
    {
        return Err(CurveContractError::BoundExceeded("cash helpers/parameters/resources").into());
    }
    let expected_representation = match &c.role {
        CurveRole::Government { benchmark_id }
            if label(benchmark_id) && r.basis_model.is_none() =>
        {
            CurveRepresentation::ZeroRates {
                compounding: convex_core::types::Compounding::Continuous,
            }
        }
        CurveRole::Repo { collateral_id } if label(collateral_id) && r.basis_model.is_none() => {
            CurveRepresentation::ZeroRates {
                compounding: convex_core::types::Compounding::Continuous,
            }
        }
        CurveRole::IssuerSpread {
            entity_id,
            model_id,
        } if label(entity_id) && label(model_id) && r.basis_model.is_some() => {
            CurveRepresentation::IssuerSpreads
        }
        _ => {
            return Err(invalid(
                "curve role/model",
                "explicit Government, Repo or IssuerSpread with matching basis model required",
            ))
        }
    };
    if c.representation != expected_representation
        || (!matches!(
            c.calibration,
            CalibrationMode::Parametric(_) | CalibrationMode::Ufr(_)
        ) && c.interpolation != InterpolationMethod::Linear)
        || (!matches!(c.calibration, CalibrationMode::Ufr(_))
            && !matches!(
                c.extrapolation,
                ExtrapolationMethod::None | ExtrapolationMethod::Flat | ExtrapolationMethod::Linear
            ))
        || c.time_basis != convex_core::daycounts::DayCountConvention::Act365Fixed
    {
        return Err(invalid("representation/method", "ACT365F Linear continuous-zero or signed residual basis with explicit extension required"));
    }
    let CurveSource::Helpers(ids) = &c.source else {
        return Err(invalid("source", "full original cash helper IDs required"));
    };
    if ids
        .iter()
        .map(String::as_str)
        .ne(r.helpers.iter().map(CashCalibrationHelper::id))
    {
        return Err(invalid(
            "helper IDs",
            "source must preserve every original helper in source order",
        ));
    }
    let mut expected: BTreeSet<_> = dependencies(r.basis_model.as_ref()).into_iter().collect();
    if let CalibrationMode::Ufr(settings) = &c.calibration {
        expected.insert(settings.base.clone());
    }
    let declared: BTreeSet<_> = c.dependencies.iter().cloned().collect();
    if declared != expected
        || c.dependencies.len() != declared.len()
        || declared.iter().any(|d| d.id == c.key.id)
    {
        return Err(invalid(
            "dependencies",
            "exact unique external reference/credit versions consumed by model required",
        ));
    }
    if r.pillars[0] <= c.reference_date || r.pillars.windows(2).any(|d| d[1] <= d[0]) {
        return Err(invalid(
            "parameter pillars",
            "strictly increasing future dated grid required",
        ));
    }
    for &date in &r.pillars {
        year_fraction(c.reference_date, date, c.time_basis)?;
    }
    settings(r)?;
    let mut helper_ids = BTreeSet::new();
    let mut helper_dates = BTreeSet::new();
    let mut bonds = Vec::new();
    for (h, resource) in r.helpers.iter().zip(input.resources) {
        if !label(h.id())
            || !helper_ids.insert(h.id())
            || h.pillar() <= c.reference_date
            || h.pillar()
                > *r.pillars
                    .last()
                    .ok_or_else(|| invalid("pillars", "empty"))?
            || !h.tolerance().is_finite()
            || h.tolerance() <= 0.0
            || h.tolerance() > 1.0
        {
            return Err(invalid("original helper", "unique bounded IDs, informative full horizon and original positive tolerance required"));
        }
        if !helper_dates.insert(h.pillar()) && r.overlap != CashOverlapPolicy::RetainAllGlobal {
            return Err(invalid(
                "overlapping pillars",
                "explicitly reject or retain all in global fit; no helper may be dropped",
            ));
        }
        match (h, &c.role, resource) {
            (CashCalibrationHelper::Bond { input: h }, CurveRole::Government { .. }, _) => {
                if h.credit_identity.is_some() {
                    return Err(invalid(
                        "government mapping",
                        "issuer mapping is inapplicable to government fit",
                    ));
                }
                if h.terms.currency != c.currency {
                    return Err(invalid("currency", "cash helper currency differs"));
                }
                bonds.push(Some(pricing::admit(h, resource, c.reference_date)?));
            }
            (
                CashCalibrationHelper::Bond { input: h },
                CurveRole::IssuerSpread { entity_id, .. },
                _,
            ) => {
                let identity = h
                    .credit_identity
                    .as_ref()
                    .ok_or_else(|| invalid("issuer identity", "actual legal mapping required"))?;
                if h.quote_kind == CashQuoteKind::ParCouponRate
                    || h.terms.currency != c.currency
                    || identity.entity_id.as_str() != entity_id
                    || identity.currency != c.currency
                    || identity.maturity != h.terms.schedule.end_date
                {
                    return Err(invalid("issuer helper", "full cash price/yield and exact original legal entity/currency/maturity required"));
                }
                bonds.push(Some(pricing::admit(h, resource, c.reference_date)?));
            }
            (
                CashCalibrationHelper::Repo { input: h },
                CurveRole::Repo { collateral_id },
                CashHelperResources::Repo(calendar),
            ) => {
                h.quote.decimal_rate()?;
                let tau = year_fraction(h.start_date, h.end_date, h.day_count)?;
                if h.collateral_id != *collateral_id
                    || calendar.pin != &h.calendar
                    || h.start_date < c.reference_date
                    || !calendar
                        .calendar
                        .try_is_business_day(h.start_date)
                        .map_err(convex_core::cashflows::CashflowError::from)?
                    || !calendar
                        .calendar
                        .try_is_business_day(h.end_date)
                        .map_err(convex_core::cashflows::CashflowError::from)?
                    || 1.0 + h.quote.decimal_rate()? * tau <= 0.0
                {
                    return Err(invalid("repo loan", "exact collateral/calendar, business value/cash dates and positive original repayment required"));
                }
                bonds.push(None);
            }
            _ => {
                return Err(invalid(
                    "helper family/resources",
                    "actual full cash or repo helper must match curve role and resources",
                ))
            }
        }
    }
    Ok(Prepared { recipe: r, bonds })
}

pub(super) fn observe(
    p: &mut Prepared<'_>,
    resources: &[CashHelperResources<'_>],
) -> CashCalibrationResult<()> {
    for ((helper, bond), resource) in p.recipe.helpers.iter().zip(&mut p.bonds).zip(resources) {
        if let (CashCalibrationHelper::Bond { input: h }, Some(bond)) = (helper, bond) {
            pricing::observe(bond, h, resource)?;
        }
    }
    Ok(())
}
pub(super) fn prepare<'a>(input: &CashCurveBuildInput<'a>) -> CashCalibrationResult<Prepared<'a>> {
    super::icma::check_request(std::slice::from_ref(input), 1)?;
    let mut prepared = admit(input)?;
    observe(&mut prepared, input.resources)?;
    Ok(prepared)
}

/// Build the entire supplied cash-curve graph after full contract/topology
/// preflight. Government/Repo roots and issuer references have distinct roles;
/// failures return no partial set and cannot replace the caller's last-good set.
pub fn build_cash_curve_set(
    inputs: &[CashCurveBuildInput<'_>],
    supplied: &CurveSet,
    controls: FitNumericalControls,
) -> CashCalibrationResult<CashCurveBuildOutput> {
    if inputs.is_empty() || inputs.len() + supplied.len() > MAX_CURVES {
        return Err(CurveContractError::BoundExceeded("cash graph curves").into());
    }
    if inputs.iter().map(|i| i.recipe.helpers.len()).sum::<usize>() > MAX_CASH_HELPERS
        || inputs.iter().map(|i| i.recipe.pillars.len()).sum::<usize>() > MAX_CASH_HELPERS
    {
        return Err(
            CurveContractError::BoundExceeded("total original cash helpers/parameters").into(),
        );
    }
    super::icma::check_request(inputs, 1)?;
    let mut by_id = BTreeMap::new();
    for input in inputs {
        let c = &input.recipe.curve;
        if supplied.keys().any(|k| k.id == c.key.id)
            || by_id.insert(c.key.id.clone(), input).is_some()
        {
            return Err(invalid(
                "curve identity",
                "new unique immutable identities required; supplied curves cannot be overwritten",
            ));
        }
    }
    for input in inputs {
        for key in &input.recipe.curve.dependencies {
            key.validate()?;
            if by_id
                .get(&key.id)
                .is_none_or(|i| i.recipe.curve.key != *key)
            {
                supplied.get(key)?;
            }
        }
    }
    // Resolve financial roles/model metadata before any solver, including an
    // issuer recipe whose reference is another new government recipe.
    let metadata = |key: &CurveKey| -> CashCalibrationResult<&CurveRecipe> {
        if let Some(input) = by_id.get(&key.id) {
            if input.recipe.curve.key == *key {
                return Ok(&input.recipe.curve);
            }
        }
        Ok(supplied.get(key)?.recipe())
    };
    for input in inputs {
        if let Some(model) = &input.recipe.basis_model {
            let (reference, credit) = match model {
                BondBasisModel::ReferenceZeroSpread { reference, .. } => {
                    (metadata(reference)?, None)
                }
                BondBasisModel::CreditParRecovery {
                    discount, survival, ..
                } => (metadata(discount)?, Some(metadata(survival)?)),
            };
            for helper in &input.recipe.helpers {
                if let CashCalibrationHelper::Bond { input: h } = helper {
                    pricing::check_basis_metadata(
                        h,
                        &input.recipe.curve,
                        model,
                        reference,
                        credit,
                    )?;
                }
            }
        }
    }
    let mut ready: BTreeSet<CurveKey> = supplied.keys().cloned().collect();
    let mut pending = by_id;
    let mut order = Vec::new();
    while !pending.is_empty() {
        let id = pending
            .iter()
            .find(|(_, i)| {
                i.recipe
                    .curve
                    .dependencies
                    .iter()
                    .all(|d| ready.contains(d))
            })
            .map(|(id, _)| id.clone())
            .ok_or_else(|| {
                CurveContractError::DependencyCycle(
                    pending
                        .values()
                        .map(|i| i.recipe.curve.key.clone())
                        .collect(),
                )
            })?;
        let input = pending
            .remove(&id)
            .ok_or_else(|| invalid("graph", "missing preflight input"))?;
        ready.insert(input.recipe.curve.key.clone());
        order.push(input);
    }
    // Prepare every original contract before any solver or trial pricing.
    let mut prepared: Vec<_> = order
        .iter()
        .map(|i| admit(i))
        .collect::<CashCalibrationResult<_>>()?;
    for p in &prepared {
        if matches!(
            p.recipe.basis_model,
            Some(BondBasisModel::CreditParRecovery { .. })
        ) {
            for bond in p.bonds.iter().flatten() {
                pricing::check_credit_cash(bond)?;
            }
        }
    }
    for (p, input) in prepared.iter_mut().zip(&order) {
        observe(p, input.resources)?;
    }
    let mut curves = supplied.clone();
    let mut calibrations = Vec::new();
    let mut evaluations = 0;
    for p in prepared {
        let fitted = fit::fit(&p, &curves, controls, &mut evaluations)?;
        curves = curves.with_curve(fitted.curve.clone())?;
        calibrations.push(fitted);
    }
    Ok(CashCurveBuildOutput {
        build_order: calibrations
            .iter()
            .map(|c| c.curve.recipe().key.clone())
            .collect(),
        curves,
        calibrations,
        price_evaluations: evaluations,
    })
}
