//! Full dated entitled cash yield/Z/discount-margin prices and checked inverses.
use super::*;
use convex_bonds::contracts::{
    BondContract, BondCoupon, BondProjection, BondProjectionContext, BondProjectionPolicy,
};
use convex_core::{
    cashflows::PaymentDateEntitlement, contracts::ModelMoney, daycounts::DayCountConvention,
    types::Compounding, Currency,
};
use convex_curves::contracts::{
    discount_from_rate, year_fraction, BuiltCurve, CurvePoint, CurveQuery, CurveRole,
};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde::{Deserialize, Serialize};

/// Explicit dated-cash yield convention. This is not an inferred Street yield.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DatedYieldConvention {
    /// Actual settlement-to-payment time basis; contextual ICMA needs a separate model.
    pub day_count: DayCountConvention,
    /// Annualized yield compounding.
    pub compounding: Compounding,
}
/// Explicit reference role; government benchmark and collateral are distinct.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum BondDiscountRole {
    /// Same-currency collateral discounting.
    Collateral,
    /// Exact supplied government benchmark identity.
    Government(String),
}
/// Full native cash prepared from a checked supplied contract. Private state
/// prevents caller-created cash/accrued/face inconsistencies.
#[derive(Debug, Clone)]
pub struct PreparedBondCash {
    contract: BondContract,
    settlement: Date,
    projection: BondProjection,
}
/// Model price with original-face units and exact native projection.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BondValue {
    /// Absolute unrounded entitled-cash PV at settlement.
    pub present_value: ModelMoney,
    /// Clean per100 ORIGINAL face.
    pub clean_per100: f64,
    /// Dirty per100 ORIGINAL face.
    pub dirty_per100: f64,
    /// Actual accrued/rebate per100 ORIGINAL face.
    pub accrued_per100: f64,
    /// Full cash, outstanding principal and original known/forecast evidence.
    pub cash: BondProjection,
    /// Quote/model representation actually used.
    pub basis: BondValueBasis,
}
/// Observed representation and discount-spread semantics retained separately.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum BondValueBasis {
    /// Full fixed cash at an explicit dated yield.
    DatedYield {
        /// Annualized decimal rate.
        decimal_rate: f64,
        /// Explicit quotation convention.
        convention: DatedYieldConvention,
    },
    /// Complete cash on an exact reference curve plus a continuous annual spread.
    DiscountSpread {
        /// Exact original curve revision.
        curve: CurveKey,
        /// Collateral/government role.
        role: BondDiscountRole,
        /// Fractional annual Z or frozen-projection DM.
        decimal_spread: f64,
        /// True for floating cash, holding forecasts fixed.
        discount_margin: bool,
    },
}
impl PreparedBondCash {
    /// Prepare actual fixed/stepped/amortizing/bill/zero cash and entitlement.
    pub fn fixed(
        contract: &BondContract,
        settlement: Date,
        on_payment: PaymentDateEntitlement,
    ) -> WorkflowResult<Self> {
        contract.require_plain_valuation()?;
        Ok(Self {
            contract: contract.clone(),
            settlement,
            projection: contract.fixed_projection(settlement, on_payment)?,
        })
    }
    /// Prepare all floating cash with eligible history and separate exact index
    /// projection. Missing due history fails before any forecast.
    pub fn floating(
        contract: &BondContract,
        context: &BondProjectionContext,
        market: &SuppliedMarket<'_>,
        projection: Option<&CurveKey>,
    ) -> WorkflowResult<Self> {
        contract.require_plain_valuation()?;
        if matches!(contract.terms().coupon, BondCoupon::Fixed) {
            return Err(WorkflowError::NotApplicable(
                "floating preparation requires an actual term or overnight coupon",
            ));
        }
        let mut forecast = |request: &convex_bonds::contracts::ProjectionRequest| {
            let key =
                projection.ok_or_else(|| convex_core::cashflows::CashflowError::InvalidTerms {
                    field: "projection",
                    reason: "missing supplied projection source".into(),
                })?;
            let curve = market.curves.get(key).map_err(|e| {
                convex_core::cashflows::CashflowError::InvalidTerms {
                    field: "projection",
                    reason: e.to_string(),
                }
            })?;
            crate::bond_contract::project_from_curve(request, contract.terms().currency, curve)
        };
        let requirements = with_publications(market.publications, |p| {
            contract.fixing_requirements(context.settlement, context.on_payment, p)
        })?;
        market.publications.require(&requirements)?;
        let resolution = resolve_requirements(&requirements, market.fixings, context.fixing)?;
        if !resolution.projected.is_empty() {
            let key =
                projection.ok_or(WorkflowError::MissingInput(vec![MissingInput::Projection]))?;
            market.curves.get(key)?;
        }
        let projection = contract.project(
            context,
            market.fixings,
            market.publications.callback(),
            BondProjectionPolicy::Supplied(&mut forecast),
        )?;
        Ok(Self {
            contract: contract.clone(),
            settlement: context.settlement,
            projection,
        })
    }
    /// Complete original checked economics.
    pub fn contract(&self) -> &BondContract {
        &self.contract
    }
    /// Actual settlement boundary.
    pub fn settlement(&self) -> Date {
        self.settlement
    }
    /// Full native expected cash. Projection is not recorded payment.
    pub fn cashflows(&self) -> &BondProjection {
        &self.projection
    }
    /// Exact original face used by monetary quantity scaling.
    pub fn face_value(&self) -> Decimal {
        self.contract.terms().face_value
    }
    /// Actual currency from contract terms.
    pub fn currency(&self) -> Currency {
        self.contract.terms().currency
    }
    pub(super) fn fixed_yield(&self) -> WorkflowResult<()> {
        if !matches!(self.contract.terms().coupon, BondCoupon::Fixed) {
            return Err(WorkflowError::NotApplicable("floating cash has no contractual bond yield; use frozen-projection discount margin"));
        }
        Ok(())
    }
    pub(super) fn monotone(&self) -> WorkflowResult<bool> {
        let flows = self.projection.cashflows()?;
        if flows.iter().any(|c| c.amount < Decimal::ZERO) {
            return Err(WorkflowError::Unsupported(
                "mixed-sign cash needs a complete multiple-root inverse model",
            ));
        }
        Ok(flows
            .iter()
            .any(|c| c.date > self.settlement && c.amount > Decimal::ZERO))
    }
    pub(super) fn price(
        &self,
        mut discount: impl FnMut(Date) -> WorkflowResult<f64>,
        basis: BondValueBasis,
    ) -> WorkflowResult<BondValue> {
        let number = |v: Decimal| {
            v.to_f64()
                .filter(|n| n.is_finite() && (v.is_zero() || *n != 0.0))
                .ok_or_else(|| invalid("cash", "Decimal conversion overflow/underflow"))
        };
        let mut pv = 0.0;
        for cash in self.projection.cashflows()? {
            let df = discount(cash.date)?;
            if df <= 0.0 {
                return Err(invalid("discount", "positive finite factor required"));
            }
            pv = finite(
                pv + number(cash.amount)? * finite(df, "discount")?,
                "bond PV",
            )?;
        }
        let face = number(self.face_value())?;
        let accrued = finite(number(self.projection.accrued)? / face * 100.0, "accrued")?;
        let dirty = finite(pv / face * 100.0, "dirty price")?;
        Ok(BondValue {
            present_value: crate::quote_risk::risk_reference_pv(pv, self.currency())?,
            clean_per100: finite(dirty - accrued, "clean price")?,
            dirty_per100: dirty,
            accrued_per100: accrued,
            cash: self.projection.clone(),
            basis,
        })
    }
    /// Full actual fixed-cash quotation, including separate principal steps.
    pub fn price_yield(
        &self,
        decimal_rate: f64,
        convention: DatedYieldConvention,
    ) -> WorkflowResult<BondValue> {
        self.fixed_yield()?;
        if !decimal_rate.is_finite() || decimal_rate.abs() > 10.0 {
            return Err(invalid(
                "yield",
                "finite annual decimal rate within +/-10 required",
            ));
        }
        self.price(
            |date| {
                let t = if date == self.settlement {
                    0.0
                } else {
                    year_fraction(self.settlement, date, convention.day_count)?
                };
                Ok(discount_from_rate(decimal_rate, t, convention.compounding)?)
            },
            BondValueBasis::DatedYield {
                decimal_rate,
                convention,
            },
        )
    }
    /// Z-spread for fixed cash; frozen-projection discount margin for floating
    /// cash. ACT/365F continuous spread uses the actual settlement horizon.
    pub fn price_spread(
        &self,
        reference: &BuiltCurve,
        role: BondDiscountRole,
        decimal_spread: f64,
    ) -> WorkflowResult<BondValue> {
        if !decimal_spread.is_finite() || decimal_spread.abs() > 1.0 {
            return Err(invalid(
                "spread",
                "finite fractional annual spread within +/-1 required",
            ));
        }
        let matches = match (&role, &reference.recipe().role) {
            (
                BondDiscountRole::Collateral,
                CurveRole::Discount {
                    collateral_currency,
                },
            ) => *collateral_currency == self.currency(),
            (BondDiscountRole::Government(expected), CurveRole::Government { benchmark_id }) => {
                expected == benchmark_id
            }
            _ => false,
        };
        if !matches || reference.recipe().currency != self.currency() {
            return Err(invalid(
                "bond reference",
                "exact currency and financial role required",
            ));
        }
        let df = |date| {
            Ok::<_, WorkflowError>(
                reference
                    .query(&CurveQuery::DiscountFactor {
                        point: CurvePoint::Date(date),
                    })?
                    .value,
            )
        };
        let anchor = df(self.settlement)?;
        self.price(
            |date| {
                let t = if date == self.settlement {
                    0.0
                } else {
                    year_fraction(self.settlement, date, DayCountConvention::Act365Fixed)?
                };
                finite(
                    df(date)? / anchor * (-decimal_spread * t).exp(),
                    "spread discount",
                )
            },
            BondValueBasis::DiscountSpread {
                curve: reference.recipe().key.clone(),
                role,
                decimal_spread,
                discount_margin: !matches!(self.contract.terms().coupon, BondCoupon::Fixed),
            },
        )
    }
}
