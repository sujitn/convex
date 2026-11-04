//! Checked common PV envelope. Full cash, par, accrued and query evidence stays
//! in `SwapValuation`; this bridge requests exactly net/fixed/floating model PV.

use crate::{pricing::SwapValuation, FloatingLegTerms, ReferencePin};
use convex_core::{
    contracts::{
        ContractError, DataQuality, MeasureId, MeasureResult, MeasureValue, Provenance, RequestId,
        ResultEnvelope, ResultStatus, ValuationSubject, Version,
    },
    fixings::RateOrigin,
};
use convex_curves::contracts::CurveKey;
use std::collections::{BTreeMap, BTreeSet};

/// An explicit adapter binding from an opaque native curve version to the
/// canonical numeric record revision. Neither version is parsed or invented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurveVersionBinding {
    /// Exact native identity/version actually consumed.
    pub curve: CurveKey,
    /// Exact canonical revision of the corresponding immutable curve record.
    pub record_version: Version,
}

fn invalid(reason: &'static str) -> ContractError {
    ContractError {
        field: "swap_provenance",
        reason,
    }
}
fn require_version(
    provenance: &Provenance,
    kind: &str,
    id: &str,
    version: Version,
) -> Result<(), ContractError> {
    if !provenance.input_versions.iter().any(|input| {
        input.kind.as_str() == kind && input.id.as_str() == id && input.version == version
    }) {
        return Err(invalid(
            "missing or mismatched actual consumed input revision",
        ));
    }
    Ok(())
}
fn require_pin(
    provenance: &Provenance,
    kind: &str,
    pin: &ReferencePin,
) -> Result<(), ContractError> {
    require_version(provenance, kind, pin.id.as_str(), pin.version)
}

impl SwapValuation {
    /// Return the common v2 envelope for exactly the three signed model PVs.
    /// Validate trade version, economic date, known-at and actual calendar,
    /// collateral, fixing and transitive curve versions before constructing it.
    /// Published fixing record IDs are `index_id/YYYY-MM-DD`, kind `fixing`;
    /// calendars use `calendar`, agreement `collateral`, curves `curve`.
    /// Curve bindings must be one-to-one with actual consumed native keys.
    /// Caller supplies immutable snapshot/configuration/model/build/time/quality;
    /// this function neither certifies those policies nor obtains current time.
    pub fn price_result(
        &self,
        request: RequestId,
        provenance: Provenance,
        quality: DataQuality,
        bindings: &[CurveVersionBinding],
    ) -> Result<ResultEnvelope, ContractError> {
        provenance.validate()?;
        let terms = &self.cashflows.terms;
        if provenance.subject
            != (ValuationSubject::TradeVersion {
                trade_id: terms.trade_id.clone(),
                version: terms.version,
            })
            || provenance.valuation_time.datetime().date_naive()
                != self
                    .cashflows
                    .context
                    .fixings
                    .valuation_date
                    .as_naive_date()
            || provenance.known_at != self.cashflows.context.fixings.known_at
        {
            return Err(invalid("trade version, valuation date and information cutoff must match the actual calculation"));
        }
        for pins in [&terms.fixed.leg.calendars, &terms.floating.leg().calendars] {
            require_pin(&provenance, "calendar", &pins.accrual)?;
            require_pin(&provenance, "calendar", &pins.payment)?;
        }
        match &terms.floating {
            FloatingLegTerms::Term(t) => {
                require_pin(&provenance, "calendar", &t.resets.reset_calendar)?;
                require_pin(&provenance, "calendar", &t.resets.fixing_calendar)?;
            }
            FloatingLegTerms::Overnight(t) => {
                require_pin(&provenance, "calendar", &t.observation_calendar)?
            }
        }
        require_pin(&provenance, "collateral", &terms.collateral.agreement)?;
        for rate in &self.cashflows.rates {
            if let RateOrigin::Published(observation) = &rate.resolved.origin {
                require_version(
                    &provenance,
                    "fixing",
                    &format!(
                        "{}/{}",
                        observation.index_id.as_str(),
                        observation.observation_date
                    ),
                    observation.revision,
                )?;
            }
        }
        let actual: BTreeSet<_> = self
            .consumed_curves
            .iter()
            .map(|curve| &curve.key)
            .collect();
        let bound: BTreeSet<_> = bindings.iter().map(|binding| &binding.curve).collect();
        if bindings.len() != actual.len() || bound != actual {
            return Err(invalid(
                "exact one-to-one actual curve version bindings required",
            ));
        }
        for binding in bindings {
            require_version(
                &provenance,
                "curve",
                &binding.curve.id,
                binding.record_version,
            )?;
        }
        let mut measures = BTreeMap::new();
        for (id, value) in [
            ("pv", self.present_value),
            ("fixed_pv", self.fixed_present_value),
            ("floating_pv", self.floating_present_value),
        ] {
            measures.insert(
                MeasureId::new(id)?,
                MeasureResult {
                    value: MeasureValue::ModelMoney(value),
                    quality,
                    method: provenance.model_version.clone(),
                    source_observation_id: None,
                },
            );
        }
        ResultEnvelope::new(
            request,
            ResultStatus::Complete,
            quality,
            measures,
            vec![],
            provenance,
        )
    }
}

/// Native construction/pricing status is separate from host/certification scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapCapabilityStatus {
    /// Complete callable native operation under its stated supplied-term domain.
    Supported,
    /// Operation requires the stated additional economic admissibility conditions.
    Conditional,
    /// Owning implementation remains explicit; no missing result becomes zero.
    Unsupported,
}
/// Inspectable native pricing capability and its material constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapPricingCapability {
    /// Public operation name.
    pub operation: &'static str,
    /// Native operation status, not a certificate or host endpoint claim.
    pub status: SwapCapabilityStatus,
    /// Supported economics or remaining owning task.
    pub constraints: &'static str,
}
/// Capability matrix for this exact native pricer.
pub fn swap_pricing_capabilities() -> &'static [SwapPricingCapability] {
    use SwapCapabilityStatus::{Conditional, Supported, Unsupported};
    &[
        SwapPricingCapability { operation:"pv",status:Supported,constraints:"checked single-currency IRS/OIS; explicit collateral/index mappings, eligible history, actual dated coupon profiles" },
        SwapPricingCapability { operation:"accrued",status:Supported,constraints:"signed expected outstanding coupon cash; resolved reset or interest-weight prefix; actual coupon rounding" },
        SwapPricingCapability { operation:"par",status:Conditional,constraints:"positive remaining annuities, unrounded cash, Allow negative-coupon policies; uniform replacement and parallel stepped-profile shifts" },
        SwapPricingCapability { operation:"price_result",status:Conditional,constraints:"exactly net/fixed/floating PV in common v2 envelope; checked actual input revision manifest within its 256-record bound" },
        SwapPricingCapability { operation:"quote_sensitivities",status:Unsupported,constraints:"CVX-057 calibrated discount/index quote sensitivities" },
        SwapPricingCapability { operation:"lifecycle",status:Unsupported,constraints:"CVX-061/062 trade/cash events and lifecycle analytics" },
        SwapPricingCapability { operation:"host_dispatch",status:Unsupported,constraints:"CVX-063 common schema/FFI/Java/Excel/MCP/WASM dispatch; CVX-033 server routing" },
    ]
}
