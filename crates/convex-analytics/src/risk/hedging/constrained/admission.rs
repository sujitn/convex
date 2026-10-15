//! Original risk validation, exact quantity admission and complete aggregation.
use super::evidence::{AdmittedSensitivity, MethodBinding};
use super::*;
use crate::quote_risk::{RiskPosition, RiskQuantityUnit};
use convex_core::{contracts::*, Currency};
use nalgebra::{DMatrix, DVector};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use std::collections::{HashMap, HashSet};

pub(super) struct Prepared<S> {
    pub exposure: Exposure<S>,
    pub design: DMatrix<f64>,
    pub offset: DVector<f64>,
    pub risk: DMatrix<f64>,
    pub lower: Vec<Decimal>,
    pub upper: Vec<Decimal>,
    pub held: Vec<Decimal>,
    pub reference: Vec<Decimal>,
    pub buy: Vec<f64>,
    pub sell: Vec<f64>,
}
pub(super) fn number(value: Decimal) -> Result<f64, HedgeError> {
    let result = value
        .to_f64()
        .ok_or_else(|| invalid("quantity", "Decimal conversion failed"))?;
    if value != Decimal::ZERO && result == 0.0 {
        return Err(invalid("quantity", "nonzero Decimal conversion underflow"));
    }
    finite(result, "quantity")
}
fn quantity(value: Decimal, unit: RiskQuantityUnit) -> Result<(), HedgeError> {
    if value.abs() > Decimal::from(1_000_000_000_000_000i64)
        || unit == RiskQuantityUnit::Contracts && value.fract() != Decimal::ZERO
    {
        return Err(invalid(
            "quantity",
            "bounded exact native quantity and integer contracts required",
        ));
    }
    Ok(())
}
fn scaled<S>(vector: &RiskVector<S>, q: Decimal) -> Result<f64, HedgeError> {
    let p = RiskPosition {
        quantity: ExactDecimal::new(q),
        ..vector.reference_position.clone()
    };
    p.scale().map_err(|e| invalid("quantity", e.to_string()))
}
fn settings(s: &HedgeSolveSettings) -> Result<(), HedgeError> {
    for value in [
        s.rank_relative_tolerance,
        s.max_condition,
        s.solver_tolerance,
        s.factor_tolerance,
        s.cost_tolerance,
        s.cost_scale,
        s.cost_weight,
    ] {
        finite(value, "settings")?;
    }
    if !(0.0..=1e-3).contains(&s.rank_relative_tolerance)
        || s.rank_relative_tolerance == 0.0
        || !(1.0..=1e4).contains(&s.max_condition)
        || !(0.0..=1e-6).contains(&s.solver_tolerance)
        || s.solver_tolerance == 0.0
        || !(0.0..=1e-6).contains(&s.factor_tolerance)
        || !(0.0..=1e-6).contains(&s.cost_tolerance)
        || s.cost_scale <= 0.0
        || s.cost_weight < 0.0
        || !(1..=100_000).contains(&s.max_work)
        || !(1..=1000).contains(&s.svd_iterations)
    {
        return Err(invalid(
            "settings",
            "explicit bounded scaling/rank/condition/feasibility/work policy required",
        ));
    }
    Ok(())
}
fn validate_vector<S: AdmittedSensitivity>(
    p: &Problem<S>,
    v: &RiskVector<S>,
) -> Result<Vec<f64>, HedgeError> {
    let position = &v.reference_position;
    position
        .scale()
        .map_err(|e| invalid("reference_position", e.to_string()))?;
    if v.entity != p.entity
        || v.settlement != p.settlement
        || position.quantity != position.reference_quantity
        || (v.product == HedgeProduct::Futures)
            != (position.quantity_unit == RiskQuantityUnit::Contracts)
        || position.quantity_unit == RiskQuantityUnit::TradeCopies
        || v.credit_scope.len() > 16
        || v.product == HedgeProduct::Cds && v.credit_scope.len() != 1
        || v.product == HedgeProduct::Irs && !v.credit_scope.is_empty()
    {
        return Err(invalid("risk", "exact entity/settlement, positive native reference size and product/quantity/credit scope required"));
    }
    for (i, c) in v.credit_scope.iter().enumerate() {
        if !(0.0..=1.0).contains(&c.recovery_fraction.get())
            || v.credit_scope[..i]
                .iter()
                .any(|previous| previous.entity_id == c.entity_id)
        {
            return Err(invalid(
                "credit_scope",
                "unique exact legal entities and fractional recovery required",
            ));
        }
    }
    let fx = match (&v.fx, position.currency == p.reporting_currency) {
        (None, true) => 1.0,
        (Some(f), false)
            if f.from == position.currency
                && f.to == p.reporting_currency
                && f.snapshot_id == p.snapshot_id
                && f.rate.decimal() > Decimal::ZERO =>
        {
            number(f.rate.decimal())?
        }
        (None, false) => return Err(HedgeError::Missing(format!("FX for {}", v.id.as_str()))),
        _ => return Err(invalid(
            "fx",
            "exact same-snapshot native/reporting conversion required; identity FX must be absent",
        )),
    };
    if v.sensitivities.len() != p.factors.len() {
        return Err(HedgeError::Missing(format!(
            "complete factor vector for {}",
            v.id.as_str()
        )));
    }
    let mut values = Vec::with_capacity(p.factors.len());
    let mut baseline = None;
    for (factor, s) in p.factors.iter().zip(&v.sensitivities) {
        s.validate_record()
            .map_err(|e| invalid("sensitivity", e.to_string()))?;
        let base_pv = s
            .baseline()
            .map_err(|e| invalid("sensitivity", e.to_string()))?;
        if *s.key() != factor.key
            || !s.matches_settings(&factor.risk_settings)
            || s.position() != position
            || s.snapshot() != &p.snapshot_id
            || baseline.is_some_and(|first| base_pv != first)
        {
            return Err(invalid("sensitivity", "full factor order/units/revisions/bump/model/snapshot/reference PV/quantity mismatch"));
        }
        baseline = Some(base_pv);
        if s.value() != 0.0
            && factor
                .credit
                .as_ref()
                .is_some_and(|c| !v.credit_scope.contains(c))
        {
            return Err(invalid("credit_scope", "nonzero credit exposure requires the exact original entity/seniority/document/restructuring/recovery scope"));
        }
        values.push(finite(s.value() * fx, "FX risk")?);
    }
    Ok(values)
}
pub(super) fn prepare<S: AdmittedSensitivity>(p: &Problem<S>) -> Result<Prepared<S>, HedgeError> {
    if p.factors.is_empty()
        || p.factors.len() > 16
        || p.candidates.len() > 4
        || p.holdings.len() > 256
        || p.expected_holdings.len() > 256
    {
        return Err(HedgeError::Bound(
            "1..16 factors, at most4 candidates and256 complete holdings",
        ));
    }
    BusinessDate::try_from(p.settlement).map_err(|e| invalid("settlement", e.to_string()))?;
    settings(&p.settings)?;
    if finite(p.max_cost, "max_cost")? < 0.0 {
        return Err(invalid(
            "max_cost",
            "nonnegative reporting-money budget required",
        ));
    }
    let mut factors = HashSet::new();
    for f in &p.factors {
        f.risk_settings
            .validate_for(&f.key)
            .map_err(|e| invalid("risk_settings", e.to_string()))?;
        if !factors.insert(&f.key) {
            return Err(invalid("factors", "duplicate full original key"));
        }
        for value in [f.scale, f.weight, f.target, f.lower, f.upper] {
            finite(value, "factor")?;
        }
        let family = f.key.definition().factor.definition().family;
        let credit = matches!(
            family,
            FactorFamily::CreditSpread | FactorFamily::Hazard | FactorFamily::Recovery
        ) && f.key.definition().factor.definition().quote_type != QuoteType::OasSpread;
        if f.scale <= 0.0 || f.weight <= 0.0 || f.lower > f.upper || credit != f.credit.is_some() {
            return Err(invalid("factor", "positive scaling/weight, closed bounds and explicit original credit scope required"));
        }
        if let Some(c) = &f.credit {
            if !(0.0..=1.0).contains(&c.recovery_fraction.get())
                || matches!(&f.key.definition().factor.definition().target,
                    FactorTarget::Entity { entity_id } if *entity_id != c.entity_id)
            {
                return Err(invalid(
                    "factor.credit",
                    "exact entity and recovery fraction required",
                ));
            }
        }
    }
    let expected: HashSet<_> = p.expected_holdings.iter().collect();
    if expected.len() != p.expected_holdings.len() {
        return Err(invalid("expected_holdings", "duplicate expected identity"));
    }
    let mut actual = HashSet::new();
    let mut instruments = HashSet::new();
    let mut current = vec![0.0; p.factors.len()];
    let mut pending = current.clone();
    let mut fx_by_currency = HashMap::<Currency, Option<HedgeFxPin>>::new();
    let mut check_fx = |v: &RiskVector<S>| -> Result<(), HedgeError> {
        let currency = v.reference_position.currency;
        if let Some(previous) = fx_by_currency.insert(currency, v.fx.clone()) {
            if previous != v.fx {
                return Err(invalid(
                    "fx",
                    "different original FX observations for the same native currency",
                ));
            }
        }
        Ok(())
    };
    let mut methods = HashMap::<crate::quote_risk::QuoteRiskKey, MethodBinding>::new();
    let mut check_methods = |v: &RiskVector<S>| -> Result<(), HedgeError> {
        for sensitivity in &v.sensitivities {
            let method = sensitivity.method_binding();
            if methods
                .insert(sensitivity.key().clone(), method)
                .is_some_and(|previous| previous != method)
            {
                return Err(invalid("risk method", "same full factor requires matching computation method, side and original parameter across every column"));
            }
        }
        Ok(())
    };
    for h in &p.holdings {
        if !actual.insert(&h.risk.id)
            || !expected.contains(&h.risk.id)
            || !instruments.insert((&h.risk.instrument.id, h.risk.instrument.version))
        {
            return Err(invalid(
                "holdings",
                "duplicate/unrequested holding or original instrument",
            ));
        }
        let values = validate_vector(p, &h.risk)?;
        check_methods(&h.risk)?;
        check_fx(&h.risk)?;
        let unit = h.risk.reference_position.quantity_unit;
        quantity(h.current_quantity.decimal(), unit)?;
        quantity(h.pending_quantity.decimal(), unit)?;
        let a = scaled(&h.risk, h.current_quantity.decimal())?;
        let b = scaled(&h.risk, h.pending_quantity.decimal())?;
        for (i, value) in values.into_iter().enumerate() {
            current[i] = finite(current[i] + value * a, "current exposure")?;
            pending[i] = finite(pending[i] + value * b, "pending exposure")?;
        }
    }
    if expected != actual {
        return Err(HedgeError::Missing(
            "expected current/pending holdings".into(),
        ));
    }
    let before: Vec<_> = current
        .iter()
        .zip(&pending)
        .map(|(a, b)| finite(a + b, "before exposure"))
        .collect::<Result<_, _>>()?;
    let mut design = DMatrix::zeros(p.factors.len(), p.candidates.len());
    let mut risk = design.clone();
    let offset = DVector::from_vec(
        p.factors
            .iter()
            .zip(&before)
            .map(|(f, b)| finite((b - f.target) / f.scale * f.weight.sqrt(), "scaled offset"))
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    let mut held = Vec::new();
    let mut reference = Vec::new();
    let mut buy = Vec::new();
    let mut sell = Vec::new();
    let mut ids = HashSet::new();
    let mut candidate_instruments = HashSet::new();
    for (j, c) in p.candidates.iter().enumerate() {
        if !ids.insert(&c.risk.id)
            || !candidate_instruments.insert((&c.risk.instrument.id, c.risk.instrument.version))
        {
            return Err(invalid(
                "candidates",
                "duplicate candidate identity or original instrument",
            ));
        }
        if !c.eligibility.eligible
            || c.eligibility.reason.trim().is_empty()
            || c.eligibility.reason.len() > 1024
        {
            return Err(invalid(
                "eligibility",
                "actual eligible decision and bounded nonempty reason required",
            ));
        }
        let values = validate_vector(p, &c.risk)?;
        check_methods(&c.risk)?;
        check_fx(&c.risk)?;
        let unit = c.risk.reference_position.quantity_unit;
        for value in [
            c.trade_lower,
            c.trade_upper,
            c.holding_lower,
            c.holding_upper,
            c.buy_available,
            c.sell_available,
        ] {
            quantity(value.decimal(), unit)?;
        }
        if c.trade_lower.decimal() > c.trade_upper.decimal()
            || c.holding_lower.decimal() > c.holding_upper.decimal()
            || c.buy_available.decimal() < Decimal::ZERO
            || c.sell_available.decimal() < Decimal::ZERO
            || c.costs.currency != c.risk.reference_position.currency
            || c.costs.buy_per_reference.decimal() < Decimal::ZERO
            || c.costs.sell_per_reference.decimal() < Decimal::ZERO
        {
            return Err(invalid("candidate", "closed original quantity intervals, nonnegative side liquidity/costs and exact monetary currency required"));
        }
        match c.lot {
            Some(lot) => {
                quantity(lot.decimal(), unit)?;
                if lot.decimal() <= Decimal::ZERO {
                    return Err(invalid("lot", "positive native trade lot required"));
                }
            }
            None if unit == RiskQuantityUnit::Contracts => {
                return Err(invalid(
                    "lot",
                    "integer futures require an explicit original lot",
                ))
            }
            None => (),
        }
        let original = p.holdings.iter().find(|h| h.risk.id == c.risk.id);
        if original.is_some_and(|h| h.risk != c.risk)
            || p.holdings
                .iter()
                .any(|h| h.risk.instrument == c.risk.instrument && h.risk.id != c.risk.id)
        {
            return Err(invalid(
                "candidate.risk",
                "matching original held instrument requires exact full risk and identity",
            ));
        }
        let holding = original
            .map(|h| {
                h.current_quantity
                    .decimal()
                    .checked_add(h.pending_quantity.decimal())
            })
            .unwrap_or(Some(Decimal::ZERO))
            .ok_or_else(|| invalid("holding", "Decimal current/pending overflow"))?;
        quantity(holding, unit)?;
        let holding_lo = c
            .holding_lower
            .decimal()
            .checked_sub(holding)
            .ok_or_else(|| invalid("holding", "Decimal bound overflow"))?;
        let holding_hi = c
            .holding_upper
            .decimal()
            .checked_sub(holding)
            .ok_or_else(|| invalid("holding", "Decimal bound overflow"))?;
        lower.push(
            c.trade_lower
                .decimal()
                .max(-c.sell_available.decimal())
                .max(holding_lo),
        );
        upper.push(
            c.trade_upper
                .decimal()
                .min(c.buy_available.decimal())
                .min(holding_hi),
        );
        held.push(holding);
        reference.push(c.risk.reference_position.reference_quantity.decimal());
        let fx = c
            .risk
            .fx
            .as_ref()
            .map(|f| number(f.rate.decimal()))
            .transpose()?
            .unwrap_or(1.0);
        buy.push(finite(
            number(c.costs.buy_per_reference.decimal())? * fx,
            "buy cost",
        )?);
        sell.push(finite(
            number(c.costs.sell_per_reference.decimal())? * fx,
            "sell cost",
        )?);
        for (i, value) in values.into_iter().enumerate() {
            risk[(i, j)] = value;
            design[(i, j)] = finite(
                value / p.factors[i].scale * p.factors[i].weight.sqrt(),
                "scaled design",
            )?;
        }
    }
    Ok(Prepared {
        exposure: Exposure {
            problem: p.clone(),
            current,
            pending,
            before,
        },
        design,
        offset,
        risk,
        lower,
        upper,
        held,
        reference,
        buy,
        sell,
    })
}
