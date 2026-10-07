use super::*;
use convex_core::contracts::{RiskMethod, SnapshotId, Version};
use serde::{Deserialize, Serialize};

/// Explicit caller-admitted comparison policy, never inferred from a model name.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskTolerance {
    /// Absolute bound in reported monetary units.
    pub absolute: f64,
    /// Relative bound applied to the larger absolute compared value.
    pub relative: f64,
}
impl RiskTolerance {
    /// Reject nonfinite, negative or vacuous policies.
    pub fn validate(self) -> QuoteRiskResult<()> {
        if !self.absolute.is_finite()
            || !self.relative.is_finite()
            || self.absolute < 0.0
            || !(0.0..=1.0).contains(&self.relative)
            || (self.absolute == 0.0 && self.relative == 0.0)
        {
            return Err(invalid(
                "tolerance",
                "positive finite absolute/relative policy required",
            ));
        }
        Ok(())
    }
    /// Allowed discrepancy; overflow fails.
    pub fn bound(self, a: f64, b: f64) -> QuoteRiskResult<f64> {
        self.validate()?;
        finite(a, "comparison")?;
        finite(b, "comparison")?;
        finite(
            self.absolute + self.relative * a.abs().max(b.abs()),
            "comparison bound",
        )
    }
}
/// Reference central bump and mandatory half-bump stability controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteRiskSettings {
    /// Positive canonical annual-rate/fraction/price-point bump, before normalization.
    pub bump: f64,
    /// Signed monetary half-step discrepancy tolerance.
    pub stability: RiskTolerance,
    /// Exact caller-admitted numerical policy revision.
    pub policy_version: Version,
}
/// A signed stable central sensitivity, including every actual reference valuation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteSensitivity {
    /// Complete immutable factor/coordinate/unit/response key.
    pub key: QuoteRiskKey,
    /// Exact actual position scaling, applied once to original native PV.
    pub position: RiskPosition,
    /// Actual caller information set; scenario values are hypothetical within it.
    pub snapshot_id: SnapshotId,
    /// Declared central/stability policy.
    pub settings: QuoteRiskSettings,
    /// Actual reference method; this route accepts only evaluated central differences.
    pub method: RiskMethod,
    /// Original scaled monetary PV, including economic side.
    pub base_pv: f64,
    /// Actual scaled PV at +h in canonical coordinate units.
    pub plus_pv: f64,
    /// Actual scaled PV at -h.
    pub minus_pv: f64,
    /// Actual scaled PV at +h/2.
    pub half_plus_pv: f64,
    /// Actual scaled PV at -h/2.
    pub half_minus_pv: f64,
    /// Signed central sensitivity in the key's stated normalization.
    pub value: f64,
    /// Independently evaluated half-step sensitivity.
    pub half_value: f64,
}
impl QuoteSensitivity {
    /// Validate deserialized or caller-created records before aggregation.
    pub fn validate(&self) -> QuoteRiskResult<()> {
        if self.method != RiskMethod::Central {
            return Err(invalid(
                "method",
                "actual reference result must declare Central",
            ));
        }
        self.position.scale()?;
        validate_settings(&self.key, &self.settings)?;
        for v in [
            self.base_pv,
            self.plus_pv,
            self.minus_pv,
            self.half_plus_pv,
            self.half_minus_pv,
            self.value,
            self.half_value,
        ] {
            finite(v, "sensitivity")?;
        }
        let expected = (self.plus_pv - self.minus_pv) / (2.0 * self.settings.bump)
            * self.key.definition().unit.coordinate_size();
        let half = (self.half_plus_pv - self.half_minus_pv) / self.settings.bump
            * self.key.definition().unit.coordinate_size();
        if self.value != expected || self.half_value != half {
            return Err(invalid(
                "sensitivity",
                "reported normalization disagrees with retained actual PVs",
            ));
        }
        if (self.value - self.half_value).abs()
            > self.settings.stability.bound(self.value, self.half_value)?
        {
            return Err(invalid(
                "stability",
                "central sensitivity fails declared half-step tolerance",
            ));
        }
        Ok(())
    }
    /// Conventional rate-loss sign, explicitly converted without another bp multiplier.
    pub fn dv01_loss(&self) -> QuoteRiskResult<f64> {
        self.validate()?;
        if self.key.definition().factor.definition().family
            != convex_core::contracts::FactorFamily::InterestRate
        {
            return Err(invalid(
                "dv01_loss",
                "only annualized interest-rate monetary risk admitted",
            ));
        }
        Ok(-self.value)
    }
}
pub(super) fn validate_settings(key: &QuoteRiskKey, s: &QuoteRiskSettings) -> QuoteRiskResult<()> {
    s.stability.validate()?;
    let maximum = match key.definition().unit {
        QuoteRiskUnit::CurrencyPerBp | QuoteRiskUnit::CurrencyPerAnnualRate => 0.1,
        QuoteRiskUnit::CurrencyPerFraction => 0.5,
        QuoteRiskUnit::CurrencyPerPricePoint => 100.0,
    };
    if !s.bump.is_finite() || s.bump < 1e-8 || s.bump > maximum {
        return Err(invalid(
            "bump",
            "finite canonical bump within admitted reference domain required",
        ));
    }
    Ok(())
}
/// Initial reference implementation. The callback must reprice the SAME complete
/// native reference trade at the supplied canonical move; it returns currency PV,
/// never clean-per100 or a rounded margin posting. Errors abort accepted risk.
/// Graph-backed coordinates should use `curve_quote_sensitivity`, which owns
/// original full helper rebuilding and freezes calibration publications.
pub fn reference_quote_sensitivity<F>(
    key: QuoteRiskKey,
    position: RiskPosition,
    snapshot_id: SnapshotId,
    settings: QuoteRiskSettings,
    reprice: F,
) -> QuoteRiskResult<QuoteSensitivity>
where
    F: FnMut(f64) -> QuoteRiskResult<convex_core::contracts::ModelMoney>,
{
    if key.definition().factor.definition().quote_type
        == convex_core::contracts::QuoteType::OasSpread
    {
        return Err(invalid("optional risk","use optional_oas_quote_sensitivity with the native model resolution/refinement controls"));
    }
    reference(key, position, snapshot_id, settings, reprice)
}
pub(super) fn reference<F>(
    key: QuoteRiskKey,
    position: RiskPosition,
    snapshot_id: SnapshotId,
    settings: QuoteRiskSettings,
    mut reprice: F,
) -> QuoteRiskResult<QuoteSensitivity>
where
    F: FnMut(f64) -> QuoteRiskResult<convex_core::contracts::ModelMoney>,
{
    validate_settings(&key, &settings)?;
    let scale = position.scale()?;
    let h = settings.bump;
    let mut price = |d| {
        let v = reprice(d)?;
        if v.currency() != position.currency {
            return Err(invalid(
                "reference PV currency",
                "native PV and position currency disagree",
            ));
        }
        finite(v.value().get() * scale, "scaled PV")
    };
    let base_pv = price(0.0)?;
    let plus_pv = price(h)?;
    let minus_pv = price(-h)?;
    let half_plus_pv = price(h / 2.0)?;
    let half_minus_pv = price(-h / 2.0)?;
    let unit = key.definition().unit.coordinate_size();
    let result = QuoteSensitivity {
        key,
        position,
        snapshot_id,
        settings,
        method: RiskMethod::Central,
        base_pv,
        plus_pv,
        minus_pv,
        half_plus_pv,
        half_minus_pv,
        value: finite((plus_pv - minus_pv) / (2.0 * h) * unit, "normalized risk")?,
        half_value: finite((half_plus_pv - half_minus_pv) / h * unit, "half risk")?,
    };
    result.validate()?;
    Ok(result)
}
/// Checked reconciliation, retained even when nonlinear bucket additivity fails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskReconciliation {
    /// Signed sum of individual original coordinates.
    pub bucket_total: f64,
    /// Actual simultaneous parallel central sensitivity.
    pub parallel: f64,
    /// Actual bucket-minus-parallel discrepancy.
    pub difference: f64,
    /// Declared allowed monetary discrepancy.
    pub bound: f64,
    /// False means no accepted additive approximation.
    pub passed: bool,
}
/// Compare matched original single-coordinate results to an actually repriced
/// parallel result. Factors must share family/target/quote/currency/model/basis,
/// response/unit/native convention, snapshot/position and numerical policy.
pub fn reconcile_quote_buckets(
    buckets: &[QuoteSensitivity],
    parallel: &QuoteSensitivity,
    tolerance: RiskTolerance,
) -> QuoteRiskResult<RiskReconciliation> {
    if !matches!(
        parallel.key.definition().coordinate,
        RiskCoordinate::ParallelQuotes { .. }
    ) {
        return Err(invalid("parallel","node completeness must be checked against PreparedQuoteRiskGraph with reconcile_curve_node_buckets"));
    }
    reconcile(buckets, parallel, tolerance)
}
pub(super) fn reconcile(
    buckets: &[QuoteSensitivity],
    parallel: &QuoteSensitivity,
    tolerance: RiskTolerance,
) -> QuoteRiskResult<RiskReconciliation> {
    if buckets.is_empty() || buckets.len() > 128 {
        return Err(invalid("buckets", "1..128 complete coordinates required"));
    }
    parallel.validate()?;
    let mut total = 0.0;
    let mut seen = std::collections::HashSet::new();
    for b in buckets {
        b.validate()?;
        let mut bf = b.key.definition().factor.definition().clone();
        let pf = parallel.key.definition().factor.definition();
        bf.tenor = pf.tenor.clone();
        if !seen.insert(&b.key)
            || bf != *pf
            || b.key.definition().response != parallel.key.definition().response
            || b.key.definition().unit != parallel.key.definition().unit
            || (!matches!(
                parallel.key.definition().coordinate,
                RiskCoordinate::ParallelQuotes { .. }
            ) && b.key.definition().native_unit != parallel.key.definition().native_unit)
            || b.snapshot_id != parallel.snapshot_id
            || b.position != parallel.position
            || b.settings != parallel.settings
            || b.base_pv != parallel.base_pv
            || pf.tenor.as_str() != "parallel"
        {
            return Err(invalid(
                "buckets",
                "duplicate or incompatible original risk coordinates",
            ));
        }
        total = finite(total + b.value, "bucket sum")?;
    }
    match &parallel.key.definition().coordinate {
        RiskCoordinate::ParallelQuotes{curve,quotes}=>{
            if quotes.len()!=buckets.len()||buckets.iter().any(|b|!matches!(&b.key.definition().coordinate,RiskCoordinate::Quote{curve:c,helper_id,quote_version} if c==curve&&quotes.contains(&(helper_id.clone(),*quote_version,b.key.definition().native_unit)))){return Err(invalid("buckets","parallel original helper set differs from complete individual coordinates"));}
        },
        RiskCoordinate::Zero{curve,node:None}=>if buckets.iter().any(|b|!matches!(&b.key.definition().coordinate,RiskCoordinate::Zero{curve:c,node:Some(_)} if c==curve)){return Err(invalid("buckets","parallel zero grid differs"));},
        RiskCoordinate::Basis{curve,node:None}=>if buckets.iter().any(|b|!matches!(&b.key.definition().coordinate,RiskCoordinate::Basis{curve:c,node:Some(_)} if c==curve)){return Err(invalid("buckets","parallel basis grid differs"));},
        RiskCoordinate::Hazard{curve,node:None}=>if buckets.iter().any(|b|!matches!(&b.key.definition().coordinate,RiskCoordinate::Hazard{curve:c,node:Some(_)} if c==curve)){return Err(invalid("buckets","parallel hazard grid differs"));},
        _=>return Err(invalid("parallel","explicit simultaneous quote/node/basis/hazard scope required")),
    }
    let bound = tolerance.bound(total, parallel.value)?;
    let difference = finite(total - parallel.value, "bucket difference")?;
    Ok(RiskReconciliation {
        bucket_total: total,
        parallel: parallel.value,
        difference,
        bound,
        passed: difference.abs() <= bound,
    })
}
