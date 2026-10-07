use super::*;
use convex_core::{
    contracts::{ExactDecimal, FactorTarget, QuoteType, RiskBasis, RiskFactorKey, Version},
    Currency,
};
use convex_curves::contracts::{CurveKey, QuoteUnit};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

/// Economic response under the declared coordinate change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskResponse {
    /// Hold other calibrated curves/model parameters fixed.
    FrozenCurves,
    /// Rebuild all supplied full recipes in dependency order at original quotes.
    RebuildDependents,
}
/// Exact original coordinate. All fields participate in equality and hashing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RiskCoordinate {
    /// One complete original market helper, never an interpolated tenor proxy.
    Quote {
        /// Exact output curve.
        curve: CurveKey,
        /// Original helper identity.
        helper_id: String,
        /// Original immutable observation revision.
        quote_version: Version,
    },
    /// Simultaneous moves of every explicitly listed compatible original quote.
    ParallelQuotes {
        /// Exact output curve.
        curve: CurveKey,
        /// Sorted unique original IDs and actual revisions.
        quotes: Vec<(String, Version, QuoteUnit)>,
    },
    /// Actual continuous-zero grid node; None explicitly means parallel.
    Zero {
        /// Exact original grid.
        curve: CurveKey,
        /// Actual interpolation-grid index, including any origin.
        node: Option<usize>,
    },
    /// Actual residual promised-cash basis grid, separate from credit/default.
    Basis {
        /// Exact original grid.
        curve: CurveKey,
        /// Actual grid index; None means parallel.
        node: Option<usize>,
    },
    /// Fractional recovery assumption on the named legal credit curve.
    Recovery {
        /// Exact entity/seniority curve.
        curve: CurveKey,
    },
    /// Actual piecewise forward-hazard interval, holding recovery separate.
    Hazard {
        /// Exact original log-linear survival grid.
        curve: CurveKey,
        /// End-grid index (origin is not an interval); None means parallel.
        node: Option<usize>,
    },
    /// Supplied instrument quote/model OAS, evaluated with the full native pricer.
    Raw {
        /// Exact economic quote identity.
        input_id: String,
        /// Original immutable quote/model revision.
        version: Version,
    },
}
/// Canonical monetary normalization. Fraction and price-point deltas are not PV01.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteRiskUnit {
    /// Monetary change normalized to an annualized +1bp rate/spread move.
    CurrencyPerBp,
    /// Monetary derivative per +1 fraction upfront/recovery.
    CurrencyPerFraction,
    /// Monetary derivative per +1 native price point.
    CurrencyPerPricePoint,
    /// Monetary derivative per +1 decimal annualized hazard rate.
    CurrencyPerAnnualRate,
}
impl QuoteRiskUnit {
    /// Canonical coordinate move represented by one reported unit.
    pub const fn coordinate_size(self) -> f64 {
        match self {
            Self::CurrencyPerBp => 0.0001,
            _ => 1.0,
        }
    }
}
/// Checked complete factor key. Native quotation and response participate in netting.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "QuoteRiskDefinition", into = "QuoteRiskDefinition")]
pub struct QuoteRiskKey(QuoteRiskDefinition);
/// Complete ingress for a checked signed quote key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteRiskDefinition {
    /// Existing provider-neutral canonical family/target/currency/tenor/semantics pins.
    pub factor: RiskFactorKey,
    /// Original actual coordinate and revision.
    pub coordinate: RiskCoordinate,
    /// Actual original quote unit; decimal, percent and bp conversion is explicit.
    pub native_unit: QuoteUnit,
    /// Held versus fully rebuilt dependencies.
    pub response: RiskResponse,
    /// Signed monetary normalization.
    pub unit: QuoteRiskUnit,
}
impl QuoteRiskKey {
    /// Validate compatible coordinate, basis, unit and response before valuation.
    pub fn new(d: QuoteRiskDefinition) -> QuoteRiskResult<Self> {
        let f = d.factor.definition();
        let q = f.quote_type;
        let rate = matches!(
            q,
            QuoteType::Yield
                | QuoteType::ZeroRate
                | QuoteType::OisParRate
                | QuoteType::SwapParRate
                | QuoteType::CdsSpread
                | QuoteType::ZSpread
                | QuoteType::DepositRate
                | QuoteType::FraRate
                | QuoteType::BasisSwapSpread
                | QuoteType::BondParCoupon
                | QuoteType::RepoRate
                | QuoteType::BankDiscountYield
                | QuoteType::RateFutureRate
                | QuoteType::ResidualBasis
                | QuoteType::OasSpread
        );
        let expected = if rate {
            QuoteRiskUnit::CurrencyPerBp
        } else if matches!(
            q,
            QuoteType::CdsUpfront | QuoteType::CdsDirtyUpfront | QuoteType::RecoveryRate
        ) {
            QuoteRiskUnit::CurrencyPerFraction
        } else if q == QuoteType::HazardRate {
            QuoteRiskUnit::CurrencyPerAnnualRate
        } else {
            QuoteRiskUnit::CurrencyPerPricePoint
        };
        let native_valid = if q == QuoteType::RateFutureRate {
            d.native_unit == QuoteUnit::FuturePrice100MinusRate
        } else if rate {
            matches!(
                d.native_unit,
                QuoteUnit::DecimalRate | QuoteUnit::PercentRate | QuoteUnit::BasisPoints
            )
        } else if matches!(q, QuoteType::CdsUpfront | QuoteType::CdsDirtyUpfront) {
            d.native_unit == QuoteUnit::UpfrontFraction
        } else if matches!(q, QuoteType::RecoveryRate | QuoteType::HazardRate) {
            d.native_unit == QuoteUnit::DecimalRate
        } else {
            d.native_unit == QuoteUnit::PricePer100
        };
        let (curve, valid) = match &d.coordinate {
            RiskCoordinate::Quote {
                curve, helper_id, ..
            } => (
                Some(curve),
                label(helper_id)
                    && f.basis == RiskBasis::MarketQuoteRecalibrated
                    && d.response == RiskResponse::RebuildDependents,
            ),
            RiskCoordinate::ParallelQuotes { curve, quotes } => (
                Some(curve),
                !quotes.is_empty()
                    && quotes.len() <= 128
                    && quotes.iter().all(|(id, _, _)| label(id))
                    && quotes.windows(2).all(|w| w[0].0 < w[1].0)
                    && f.tenor.as_str() == "parallel"
                    && f.basis == RiskBasis::MarketQuoteRecalibrated
                    && d.response == RiskResponse::RebuildDependents,
            ),
            RiskCoordinate::Zero { curve, node } => (
                Some(curve),
                q == QuoteType::ZeroRate
                    && d.native_unit == QuoteUnit::DecimalRate
                    && node.is_none_or(|n| n < 4096)
                    && (node.is_some() || f.tenor.as_str() == "parallel"),
            ),
            RiskCoordinate::Basis { curve, node } => (
                Some(curve),
                q == QuoteType::ResidualBasis
                    && d.native_unit == QuoteUnit::DecimalRate
                    && node.is_none_or(|n| n < 4096)
                    && (node.is_some() || f.tenor.as_str() == "parallel"),
            ),
            RiskCoordinate::Recovery { curve } => (Some(curve), q == QuoteType::RecoveryRate),
            RiskCoordinate::Hazard { curve, node } => (
                Some(curve),
                q == QuoteType::HazardRate
                    && node.is_none_or(|n| n > 0 && n < 4096)
                    && d.response == RiskResponse::FrozenCurves
                    && (node.is_some() || f.tenor.as_str() == "parallel"),
            ),
            RiskCoordinate::Raw { input_id, .. } => (
                None,
                label(input_id)
                    && f.basis == RiskBasis::RawQuote
                    && d.response == RiskResponse::FrozenCurves,
            ),
        };
        let target = match (&f.target, curve) {
            (FactorTarget::Curve { curve_id }, Some(c)) => curve_id.as_str() == c.id,
            (FactorTarget::Entity { .. }, Some(_)) => q == QuoteType::RecoveryRate,
            (FactorTarget::Instrument { .. }, None) => true,
            _ => false,
        };
        if let Some(c) = curve {
            c.validate()?;
        }
        if !valid || !target || !native_valid || d.unit != expected {
            return Err(invalid(
                "factor",
                "incompatible coordinate/target/native unit/normalization/response",
            ));
        }
        Ok(Self(d))
    }
    /// Immutable complete definition.
    pub fn definition(&self) -> &QuoteRiskDefinition {
        &self.0
    }
    /// Convert one signed canonical annual-rate/fraction/price move to original units.
    pub fn native_move(&self, delta: f64) -> QuoteRiskResult<f64> {
        finite(delta, "bump")?;
        let factor = if self.0.factor.definition().quote_type == QuoteType::RateFutureRate {
            -100.0
        } else {
            match self.0.native_unit {
                QuoteUnit::PercentRate => 100.0,
                QuoteUnit::BasisPoints => 10000.0,
                _ => 1.0,
            }
        };
        finite(delta * factor, "native bump")
    }
}
impl TryFrom<QuoteRiskDefinition> for QuoteRiskKey {
    type Error = QuoteRiskError;
    fn try_from(d: QuoteRiskDefinition) -> QuoteRiskResult<Self> {
        Self::new(d)
    }
}
impl From<QuoteRiskKey> for QuoteRiskDefinition {
    fn from(k: QuoteRiskKey) -> Self {
        k.0
    }
}

/// Explicit units of an already signed native reference valuation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskQuantityUnit {
    /// Face or notional relative to the full original trade's positive reference size.
    Notional,
    /// Signed integer futures contracts, relative to the declared reference count.
    Contracts,
    /// Exact signed copies of an explicitly defined full trade.
    TradeCopies,
}
/// Scale a full native reference trade exactly once. Side inside its economics
/// remains inside the pricer; quantity is relative to that reference side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskPosition {
    /// Actual signed position in declared units.
    pub quantity: ExactDecimal,
    /// Positive size already contained in native PV, in the SAME units.
    pub reference_quantity: ExactDecimal,
    /// Prevent implicit per100/notional/contract conversion.
    pub quantity_unit: RiskQuantityUnit,
    /// Native valuation and monetary risk currency.
    pub currency: Currency,
}
impl RiskPosition {
    /// Validate Decimal division and finite conversion; no rounded contract counts.
    pub fn scale(&self) -> QuoteRiskResult<f64> {
        let q = self.quantity.decimal();
        let r = self.reference_quantity.decimal();
        if r <= rust_decimal::Decimal::ZERO
            || q.abs() > rust_decimal::Decimal::from(1_000_000_000_000_000i64)
            || r > rust_decimal::Decimal::from(1_000_000_000_000_000i64)
            || (self.quantity_unit == RiskQuantityUnit::Contracts
                && (q.fract() != rust_decimal::Decimal::ZERO
                    || r.fract() != rust_decimal::Decimal::ZERO))
        {
            return Err(invalid(
                "quantity",
                "bounded quantity, positive reference and integer contracts required",
            ));
        }
        let s = q
            .checked_div(r)
            .and_then(|n| n.to_f64())
            .ok_or_else(|| invalid("quantity", "Decimal ratio failed"))?;
        if q != rust_decimal::Decimal::ZERO && s == 0.0 {
            return Err(invalid("quantity", "nonzero quantity ratio underflow"));
        }
        finite(s, "quantity")
    }
}
