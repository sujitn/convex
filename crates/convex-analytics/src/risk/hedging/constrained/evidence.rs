//! Closed internal admission for existing central and actual directional records.
use crate::quote_risk::*;
use convex_core::contracts::SnapshotId;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum MethodBinding {
    Central,
    Directional {
        settings: CouponDirectionalRiskSettings,
        parameter: f64,
    },
}
pub(super) trait AdmittedSensitivity: Clone + PartialEq {
    fn validate_record(&self) -> QuoteRiskResult<()>;
    fn key(&self) -> &QuoteRiskKey;
    fn position(&self) -> &RiskPosition;
    fn snapshot(&self) -> &SnapshotId;
    fn matches_settings(&self, settings: &QuoteRiskSettings) -> bool;
    fn baseline(&self) -> QuoteRiskResult<f64>;
    fn value(&self) -> f64;
    fn method_binding(&self) -> MethodBinding;
}
impl AdmittedSensitivity for QuoteSensitivity {
    fn validate_record(&self) -> QuoteRiskResult<()> {
        self.validate()
    }
    fn key(&self) -> &QuoteRiskKey {
        &self.key
    }
    fn position(&self) -> &RiskPosition {
        &self.position
    }
    fn snapshot(&self) -> &SnapshotId {
        &self.snapshot_id
    }
    fn matches_settings(&self, settings: &QuoteRiskSettings) -> bool {
        self.settings == *settings
    }
    fn baseline(&self) -> QuoteRiskResult<f64> {
        Ok(self.base_pv)
    }
    fn value(&self) -> f64 {
        self.value
    }
    fn method_binding(&self) -> MethodBinding {
        MethodBinding::Central
    }
}
impl AdmittedSensitivity for HedgeSensitivityEvidence {
    fn validate_record(&self) -> QuoteRiskResult<()> {
        match self {
            Self::Central(v) => v.validate(),
            Self::Directional(v) => v.validate(),
        }
    }
    fn key(&self) -> &QuoteRiskKey {
        match self {
            Self::Central(v) => &v.key,
            Self::Directional(v) => &v.key,
        }
    }
    fn position(&self) -> &RiskPosition {
        match self {
            Self::Central(v) => &v.position,
            Self::Directional(v) => &v.position,
        }
    }
    fn snapshot(&self) -> &SnapshotId {
        match self {
            Self::Central(v) => &v.snapshot_id,
            Self::Directional(v) => &v.snapshot_id,
        }
    }
    fn matches_settings(&self, settings: &QuoteRiskSettings) -> bool {
        match self {
            Self::Central(v) => v.settings == *settings,
            Self::Directional(v) => v.risk_settings() == *settings,
        }
    }
    fn baseline(&self) -> QuoteRiskResult<f64> {
        match self {
            Self::Central(v) => Ok(v.base_pv),
            Self::Directional(v) => v.base_pv(),
        }
    }
    fn value(&self) -> f64 {
        match self {
            Self::Central(v) => v.value,
            Self::Directional(v) => v.first,
        }
    }
    fn method_binding(&self) -> MethodBinding {
        match self {
            Self::Central(_) => MethodBinding::Central,
            Self::Directional(v) => MethodBinding::Directional {
                settings: v.settings,
                parameter: v.original_parameter,
            },
        }
    }
}
