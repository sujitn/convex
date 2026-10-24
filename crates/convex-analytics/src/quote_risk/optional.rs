use super::*;
use crate::optional_bond::{OptionalBondRisk, PreparedOptionalBond, RiskAxis, RiskSettings};
use convex_core::contracts::{QuoteType, SnapshotId};

/// Stable canonical optional OAS risk plus both actual native grid certificates.
/// This is an engineering numerical result, not a model/market release certificate.
#[derive(Debug, Clone)]
pub struct OptionalQuoteSensitivity {
    /// Explicit signed canonical monetary result.
    pub sensitivity: QuoteSensitivity,
    /// Original actual native h-bump resolution and sensitivity-refinement diagnostics.
    pub native: OptionalBondRisk,
    /// Independently required native h/2-bump diagnostics.
    pub half: OptionalBondRisk,
}
/// Keep the original HW1F risk resolution and separate duration/convexity grid
/// guards. Price convergence or scalar bump stability cannot bypass those gates.
/// The optional model and curves stay fixed; only declared fractional OAS moves.
pub fn optional_oas_quote_sensitivity(
    prepared: &PreparedOptionalBond,
    key: QuoteRiskKey,
    position: RiskPosition,
    snapshot_id: SnapshotId,
    settings: QuoteRiskSettings,
    oas_decimal: f64,
    native_settings: RiskSettings,
) -> QuoteRiskResult<OptionalQuoteSensitivity> {
    if key.definition().factor.definition().quote_type != QuoteType::OasSpread
        || !matches!(key.definition().coordinate, RiskCoordinate::Raw { .. })
    {
        return Err(invalid(
            "optional factor",
            "explicit raw OAS coordinate required",
        ));
    }
    if position.currency != key.definition().factor.definition().currency {
        return Err(invalid(
            "optional currency",
            "single-currency model OAS and reference PV currency must agree",
        ));
    }
    position.scale()?;
    super::finite::validate_settings(&key, &settings)?;
    let native = prepared.risk(oas_decimal, RiskAxis::Oas, settings.bump, native_settings)?;
    let half = prepared.risk(
        oas_decimal,
        RiskAxis::Oas,
        settings.bump / 2.0,
        native_settings,
    )?;
    let h = settings.bump;
    let sensitivity =
        super::finite::reference(key, position.clone(), snapshot_id, settings, |d| {
            let p = if d == 0.0 {
                &native.base
            } else if d == h {
                &native.up
            } else if d == -h {
                &native.down
            } else if d == h / 2.0 {
                &half.up
            } else if d == -h / 2.0 {
                &half.down
            } else {
                return Err(invalid(
                    "optional bump",
                    "unexpected private reference coordinate",
                ));
            };
            risk_reference_pv(p.present_value, position.currency)
        })?;
    Ok(OptionalQuoteSensitivity {
        sensitivity,
        native,
        half,
    })
}
