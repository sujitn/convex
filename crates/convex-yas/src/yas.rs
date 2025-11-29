//! Main YAS analysis calculation.
//!
//! This module provides the complete Bloomberg YAS replication,
//! combining yield, spread, and risk calculations into a single analysis.

use crate::YasError;
use crate::invoice::SettlementInvoice;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Complete YAS analysis result.
///
/// This struct contains all metrics that would be displayed on a
/// Bloomberg YAS screen for a fixed income instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YasAnalysis {
    // ===== Yield Metrics =====
    /// Street convention yield (standard market quote)
    pub street_convention: Decimal,

    /// True yield (accounts for actual settlement)
    pub true_yield: Decimal,

    /// Current yield (annual coupon / clean price)
    pub current_yield: Decimal,

    /// Simple yield
    pub simple_yield: Decimal,

    // ===== Spread Metrics =====
    /// G-Spread (yield - interpolated government yield)
    pub g_spread: Decimal,

    /// I-Spread (yield - interpolated swap rate)
    pub i_spread: Decimal,

    /// Z-Spread (constant spread over spot curve)
    pub z_spread: Decimal,

    /// Asset swap spread (par-par)
    pub asw_spread: Option<Decimal>,

    /// Option-adjusted spread (for callable bonds)
    pub oas: Option<Decimal>,

    // ===== Risk Metrics =====
    /// Macaulay duration
    pub macaulay_duration: Decimal,

    /// Modified duration
    pub modified_duration: Decimal,

    /// Convexity
    pub convexity: Decimal,

    /// DV01 (dollar value of 1bp per $100 face)
    pub dv01: Decimal,

    // ===== Settlement Invoice =====
    /// Settlement calculation details
    pub invoice: SettlementInvoice,
}

impl YasAnalysis {
    /// Create a new YAS analysis builder.
    pub fn builder() -> YasAnalysisBuilder {
        YasAnalysisBuilder::default()
    }
}

/// Builder for YAS analysis.
#[derive(Debug, Default)]
pub struct YasAnalysisBuilder {
    street_convention: Option<Decimal>,
    true_yield: Option<Decimal>,
    current_yield: Option<Decimal>,
    simple_yield: Option<Decimal>,
    g_spread: Option<Decimal>,
    i_spread: Option<Decimal>,
    z_spread: Option<Decimal>,
    asw_spread: Option<Decimal>,
    oas: Option<Decimal>,
    macaulay_duration: Option<Decimal>,
    modified_duration: Option<Decimal>,
    convexity: Option<Decimal>,
    dv01: Option<Decimal>,
    invoice: Option<SettlementInvoice>,
}

impl YasAnalysisBuilder {
    /// Set the street convention yield
    pub fn street_convention(mut self, yield_val: Decimal) -> Self {
        self.street_convention = Some(yield_val);
        self
    }

    /// Set the true yield
    pub fn true_yield(mut self, yield_val: Decimal) -> Self {
        self.true_yield = Some(yield_val);
        self
    }

    /// Set the current yield
    pub fn current_yield(mut self, yield_val: Decimal) -> Self {
        self.current_yield = Some(yield_val);
        self
    }

    /// Set the simple yield
    pub fn simple_yield(mut self, yield_val: Decimal) -> Self {
        self.simple_yield = Some(yield_val);
        self
    }

    /// Set the G-spread
    pub fn g_spread(mut self, spread: Decimal) -> Self {
        self.g_spread = Some(spread);
        self
    }

    /// Set the I-spread
    pub fn i_spread(mut self, spread: Decimal) -> Self {
        self.i_spread = Some(spread);
        self
    }

    /// Set the Z-spread
    pub fn z_spread(mut self, spread: Decimal) -> Self {
        self.z_spread = Some(spread);
        self
    }

    /// Set the asset swap spread
    pub fn asw_spread(mut self, spread: Decimal) -> Self {
        self.asw_spread = Some(spread);
        self
    }

    /// Set the OAS
    pub fn oas(mut self, spread: Decimal) -> Self {
        self.oas = Some(spread);
        self
    }

    /// Set the Macaulay duration
    pub fn macaulay_duration(mut self, duration: Decimal) -> Self {
        self.macaulay_duration = Some(duration);
        self
    }

    /// Set the modified duration
    pub fn modified_duration(mut self, duration: Decimal) -> Self {
        self.modified_duration = Some(duration);
        self
    }

    /// Set the convexity
    pub fn convexity(mut self, conv: Decimal) -> Self {
        self.convexity = Some(conv);
        self
    }

    /// Set the DV01
    pub fn dv01(mut self, dv01: Decimal) -> Self {
        self.dv01 = Some(dv01);
        self
    }

    /// Set the settlement invoice
    pub fn invoice(mut self, invoice: SettlementInvoice) -> Self {
        self.invoice = Some(invoice);
        self
    }

    /// Build the YAS analysis
    pub fn build(self) -> Result<YasAnalysis, YasError> {
        Ok(YasAnalysis {
            street_convention: self.street_convention.ok_or_else(|| {
                YasError::MissingData("street_convention is required".to_string())
            })?,
            true_yield: self.true_yield.unwrap_or(Decimal::ZERO),
            current_yield: self.current_yield.unwrap_or(Decimal::ZERO),
            simple_yield: self.simple_yield.unwrap_or(Decimal::ZERO),
            g_spread: self.g_spread.unwrap_or(Decimal::ZERO),
            i_spread: self.i_spread.unwrap_or(Decimal::ZERO),
            z_spread: self.z_spread.unwrap_or(Decimal::ZERO),
            asw_spread: self.asw_spread,
            oas: self.oas,
            macaulay_duration: self.macaulay_duration.unwrap_or(Decimal::ZERO),
            modified_duration: self.modified_duration.unwrap_or(Decimal::ZERO),
            convexity: self.convexity.unwrap_or(Decimal::ZERO),
            dv01: self.dv01.unwrap_or(Decimal::ZERO),
            invoice: self.invoice.ok_or_else(|| {
                YasError::MissingData("settlement invoice is required".to_string())
            })?,
        })
    }
}

impl std::fmt::Display for YasAnalysis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== YAS Analysis ===")?;
        writeln!(f, "\nYields:")?;
        writeln!(f, "  Street Convention: {:.6}%", self.street_convention)?;
        writeln!(f, "  True Yield:        {:.6}%", self.true_yield)?;
        writeln!(f, "  Current Yield:     {:.3}%", self.current_yield)?;
        writeln!(f, "\nSpreads:")?;
        writeln!(f, "  G-Spread:          {:.1} bps", self.g_spread)?;
        writeln!(f, "  I-Spread:          {:.1} bps", self.i_spread)?;
        writeln!(f, "  Z-Spread:          {:.1} bps", self.z_spread)?;
        writeln!(f, "\nRisk Metrics:")?;
        writeln!(f, "  Mod Duration:      {:.3}", self.modified_duration)?;
        writeln!(f, "  Convexity:         {:.3}", self.convexity)?;
        writeln!(f, "  DV01:              ${:.4}", self.dv01)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_yas_builder() {
        let invoice = SettlementInvoice {
            settlement_date: chrono::NaiveDate::from_ymd_opt(2020, 4, 29).unwrap(),
            clean_price: dec!(110.503),
            accrued_interest: dec!(2.698611),
            dirty_price: dec!(113.201611),
            accrued_days: 134,
            principal_amount: dec!(1105030.0),
            accrued_amount: dec!(26986.11),
            settlement_amount: dec!(1132016.11),
            face_value: dec!(1000000.0),
        };

        let analysis = YasAnalysis::builder()
            .street_convention(dec!(4.905895))
            .true_yield(dec!(4.903264))
            .current_yield(dec!(6.561))
            .g_spread(dec!(448.5))
            .z_spread(dec!(444.7))
            .modified_duration(dec!(4.209))
            .convexity(dec!(0.219))
            .dv01(dec!(0.0477))
            .invoice(invoice)
            .build()
            .unwrap();

        assert_eq!(analysis.street_convention, dec!(4.905895));
        assert_eq!(analysis.g_spread, dec!(448.5));
    }
}
