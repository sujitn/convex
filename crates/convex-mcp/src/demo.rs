//! Demo mode with realistic December 2025 market data.
//!
//! This module provides sample bonds and yield curves for testing and demonstration.
//! Data is designed to be internally consistent and representative of December 2025
//! market conditions.

use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use convex_bonds::instruments::{
    CallableBond, FixedRateBond, FixedRateBondBuilder, FloatingRateNote, FloatingRateNoteBuilder,
    ZeroCouponBond,
};
use convex_bonds::traits::Bond;
use convex_bonds::types::{CallEntry, CallSchedule, CallType};
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Currency, Date, Frequency};
use convex_curves::multicurve::RateIndex;
use convex_curves::{DiscreteCurve, InterpolationMethod, RateCurve, ValueType};

use crate::server::{StoredBond, StoredCurve};

/// Demo market data for December 2025
#[derive(Clone, Debug)]
pub struct DemoData {
    /// Reference/settlement date
    pub reference_date: Date,
    /// Sample bonds
    pub bonds: HashMap<String, StoredBond>,
    /// Sample yield curves
    pub curves: HashMap<String, StoredCurve>,
    /// Market snapshot description
    pub market_description: String,
}

/// Bond metadata for demo listing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DemoBondInfo {
    /// Bond identifier
    pub id: String,
    /// Human-readable bond description
    pub description: String,
    /// Bond type (Fixed, Zero, Callable, FRN)
    pub bond_type: String,
    /// Currency code (USD, EUR, GBP)
    pub currency: String,
    /// Coupon rate as percentage (None for zero coupon bonds)
    pub coupon: Option<f64>,
    /// Maturity date as string
    pub maturity: String,
    /// Clean price as percentage of par
    pub price: f64,
    /// Yield to maturity as percentage
    pub yield_pct: f64,
}

/// Curve metadata for demo listing
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DemoCurveInfo {
    /// Curve identifier
    pub id: String,
    /// Human-readable curve description
    pub description: String,
    /// Currency code (USD, EUR, GBP)
    pub currency: String,
    /// Curve type (Government, Swap, Credit)
    pub curve_type: String,
    /// Reference date as string
    pub reference_date: String,
    /// Tenor points in years
    pub tenors: Vec<f64>,
}

impl DemoData {
    /// Create demo data with December 2025 market conditions
    ///
    /// Market assumptions:
    /// - Fed Funds: 4.25-4.50% (post-cut cycle)
    /// - 10Y Treasury: ~4.40%
    /// - ECB deposit rate: ~2.75%
    /// - BOE rate: ~4.00%
    /// - USD IG spreads: 80-120bp
    /// - USD HY spreads: 300-400bp
    pub fn december_2025() -> Self {
        let reference_date = Date::from_ymd(2025, 12, 20).unwrap();

        let mut bonds = HashMap::new();
        let mut curves = HashMap::new();

        // ========================================
        // USD Treasury Curve
        // ========================================
        let usd_tsy_tenors = vec![0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];
        let usd_tsy_rates = vec![
            0.0438, // 3M: 4.38%
            0.0435, // 6M: 4.35%
            0.0425, // 1Y: 4.25%
            0.0415, // 2Y: 4.15%
            0.0420, // 3Y: 4.20%
            0.0430, // 5Y: 4.30%
            0.0438, // 7Y: 4.38%
            0.0445, // 10Y: 4.45%
            0.0465, // 20Y: 4.65%
            0.0460, // 30Y: 4.60%
        ];
        if let Ok(curve) = create_zero_curve(
            reference_date,
            usd_tsy_tenors.clone(),
            usd_tsy_rates,
            DayCountConvention::ActActIsda,
        ) {
            curves.insert("USD.TSY".to_string(), curve);
        }

        // ========================================
        // USD SOFR Swap Curve
        // ========================================
        let usd_swap_tenors = vec![0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 15.0, 20.0, 30.0];
        let usd_swap_rates = vec![
            0.0435, // 3M
            0.0432, // 6M
            0.0420, // 1Y
            0.0412, // 2Y
            0.0418, // 3Y
            0.0428, // 5Y
            0.0436, // 7Y
            0.0442, // 10Y
            0.0455, // 15Y
            0.0462, // 20Y
            0.0458, // 30Y
        ];
        if let Ok(curve) = create_zero_curve(
            reference_date,
            usd_swap_tenors,
            usd_swap_rates,
            DayCountConvention::Act360,
        ) {
            curves.insert("USD.SOFR".to_string(), curve);
        }

        // ========================================
        // EUR Curves
        // ========================================
        // German Bund curve
        let eur_bund_tenors = vec![0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 15.0, 20.0, 30.0];
        let eur_bund_rates = vec![
            0.0270, // 6M: 2.70%
            0.0260, // 1Y: 2.60%
            0.0245, // 2Y: 2.45%
            0.0240, // 3Y: 2.40%
            0.0235, // 5Y: 2.35%
            0.0240, // 7Y: 2.40%
            0.0250, // 10Y: 2.50%
            0.0265, // 15Y: 2.65%
            0.0275, // 20Y: 2.75%
            0.0280, // 30Y: 2.80%
        ];
        if let Ok(curve) = create_zero_curve(
            reference_date,
            eur_bund_tenors,
            eur_bund_rates,
            DayCountConvention::ActActIsda,
        ) {
            curves.insert("EUR.BUND".to_string(), curve);
        }

        // Italian BTP curve (spreads over Bunds)
        let eur_btp_rates = vec![
            0.0320, // 6M: +50bp
            0.0320, // 1Y: +60bp
            0.0335, // 2Y: +90bp
            0.0345, // 3Y: +105bp
            0.0360, // 5Y: +125bp
            0.0380, // 7Y: +140bp
            0.0400, // 10Y: +150bp (BTP-Bund spread ~150bp)
            0.0420, // 15Y: +155bp
            0.0435, // 20Y: +160bp
            0.0450, // 30Y: +170bp
        ];
        if let Ok(curve) = create_zero_curve(
            reference_date,
            vec![0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 15.0, 20.0, 30.0],
            eur_btp_rates,
            DayCountConvention::ActActIsda,
        ) {
            curves.insert("EUR.BTP".to_string(), curve);
        }

        // ========================================
        // GBP Gilt Curve
        // ========================================
        let gbp_gilt_tenors = vec![0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];
        let gbp_gilt_rates = vec![
            0.0395, // 6M: 3.95%
            0.0385, // 1Y: 3.85%
            0.0375, // 2Y: 3.75%
            0.0378, // 3Y: 3.78%
            0.0385, // 5Y: 3.85%
            0.0395, // 7Y: 3.95%
            0.0410, // 10Y: 4.10%
            0.0450, // 20Y: 4.50%
            0.0465, // 30Y: 4.65%
        ];
        if let Ok(curve) = create_zero_curve(
            reference_date,
            gbp_gilt_tenors,
            gbp_gilt_rates,
            DayCountConvention::ActActIsda,
        ) {
            curves.insert("GBP.GILT".to_string(), curve);
        }

        // ========================================
        // USD IG Credit Curve
        // ========================================
        let usd_ig_tenors = vec![1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];
        let usd_ig_rates = vec![
            0.0515, // 1Y: TSY + 90bp
            0.0520, // 2Y: TSY + 105bp
            0.0530, // 3Y: TSY + 110bp
            0.0545, // 5Y: TSY + 115bp
            0.0560, // 7Y: TSY + 122bp
            0.0575, // 10Y: TSY + 130bp
            0.0600, // 20Y: TSY + 135bp
            0.0600, // 30Y: TSY + 140bp
        ];
        if let Ok(curve) = create_zero_curve(
            reference_date,
            usd_ig_tenors,
            usd_ig_rates,
            DayCountConvention::Thirty360US,
        ) {
            curves.insert("USD.IG".to_string(), curve);
        }

        // ========================================
        // Sample Bonds - US Treasuries
        // ========================================

        // 2Y Treasury (on-the-run)
        if let Some(bond) = create_fixed_bond(
            "UST.2Y",
            dec!(0.0425),
            Date::from_ymd(2027, 11, 30).unwrap(),
            Date::from_ymd(2025, 11, 30).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::ActActIcma,
            Currency::USD,
        ) {
            bonds.insert("UST.2Y".to_string(), StoredBond::Fixed(bond));
        }

        // 5Y Treasury
        if let Some(bond) = create_fixed_bond(
            "UST.5Y",
            dec!(0.0430),
            Date::from_ymd(2030, 11, 30).unwrap(),
            Date::from_ymd(2025, 11, 30).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::ActActIcma,
            Currency::USD,
        ) {
            bonds.insert("UST.5Y".to_string(), StoredBond::Fixed(bond));
        }

        // 10Y Treasury (on-the-run)
        if let Some(bond) = create_fixed_bond(
            "UST.10Y",
            dec!(0.0450),
            Date::from_ymd(2035, 11, 15).unwrap(),
            Date::from_ymd(2025, 11, 15).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::ActActIcma,
            Currency::USD,
        ) {
            bonds.insert("UST.10Y".to_string(), StoredBond::Fixed(bond));
        }

        // 30Y Treasury
        if let Some(bond) = create_fixed_bond(
            "UST.30Y",
            dec!(0.0475),
            Date::from_ymd(2055, 11, 15).unwrap(),
            Date::from_ymd(2025, 11, 15).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::ActActIcma,
            Currency::USD,
        ) {
            bonds.insert("UST.30Y".to_string(), StoredBond::Fixed(bond));
        }

        // Off-the-run 10Y (old issue, higher coupon)
        if let Some(bond) = create_fixed_bond(
            "UST.10Y.OLD",
            dec!(0.0375),
            Date::from_ymd(2034, 5, 15).unwrap(),
            Date::from_ymd(2024, 5, 15).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::ActActIcma,
            Currency::USD,
        ) {
            bonds.insert("UST.10Y.OLD".to_string(), StoredBond::Fixed(bond));
        }

        // ========================================
        // US Corporate Bonds
        // ========================================

        // Apple 10Y (AA+ rated, tight spread)
        if let Some(bond) = create_fixed_bond(
            "AAPL.10Y",
            dec!(0.0485),
            Date::from_ymd(2035, 9, 15).unwrap(),
            Date::from_ymd(2025, 9, 15).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
            Currency::USD,
        ) {
            bonds.insert("AAPL.10Y".to_string(), StoredBond::Fixed(bond));
        }

        // Microsoft 5Y (AAA rated)
        if let Some(bond) = create_fixed_bond(
            "MSFT.5Y",
            dec!(0.0460),
            Date::from_ymd(2030, 6, 1).unwrap(),
            Date::from_ymd(2025, 6, 1).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
            Currency::USD,
        ) {
            bonds.insert("MSFT.5Y".to_string(), StoredBond::Fixed(bond));
        }

        // JPMorgan 10Y (A rated bank)
        if let Some(bond) = create_fixed_bond(
            "JPM.10Y",
            dec!(0.0560),
            Date::from_ymd(2035, 4, 1).unwrap(),
            Date::from_ymd(2025, 4, 1).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
            Currency::USD,
        ) {
            bonds.insert("JPM.10Y".to_string(), StoredBond::Fixed(bond));
        }

        // High Yield Bond (BB rated)
        if let Some(bond) = create_fixed_bond(
            "HY.SAMPLE",
            dec!(0.0825),
            Date::from_ymd(2030, 3, 15).unwrap(),
            Date::from_ymd(2025, 3, 15).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
            Currency::USD,
        ) {
            bonds.insert("HY.SAMPLE".to_string(), StoredBond::Fixed(bond));
        }

        // ========================================
        // EUR Bonds
        // ========================================

        // German Bund 10Y
        if let Some(bond) = create_fixed_bond(
            "DBR.10Y",
            dec!(0.0250),
            Date::from_ymd(2035, 8, 15).unwrap(),
            Date::from_ymd(2025, 8, 15).unwrap(),
            Frequency::Annual,
            DayCountConvention::ActActIcma,
            Currency::EUR,
        ) {
            bonds.insert("DBR.10Y".to_string(), StoredBond::Fixed(bond));
        }

        // French OAT 10Y
        if let Some(bond) = create_fixed_bond(
            "FRTR.10Y",
            dec!(0.0300),
            Date::from_ymd(2035, 5, 25).unwrap(),
            Date::from_ymd(2025, 5, 25).unwrap(),
            Frequency::Annual,
            DayCountConvention::ActActIcma,
            Currency::EUR,
        ) {
            bonds.insert("FRTR.10Y".to_string(), StoredBond::Fixed(bond));
        }

        // Italian BTP 10Y
        if let Some(bond) = create_fixed_bond(
            "BTPS.10Y",
            dec!(0.0400),
            Date::from_ymd(2035, 9, 1).unwrap(),
            Date::from_ymd(2025, 9, 1).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::ActActIcma,
            Currency::EUR,
        ) {
            bonds.insert("BTPS.10Y".to_string(), StoredBond::Fixed(bond));
        }

        // ========================================
        // GBP Gilts
        // ========================================

        // UK Gilt 10Y
        if let Some(bond) = create_fixed_bond(
            "UKT.10Y",
            dec!(0.0425),
            Date::from_ymd(2035, 7, 22).unwrap(),
            Date::from_ymd(2025, 7, 22).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::ActActIcma,
            Currency::GBP,
        ) {
            bonds.insert("UKT.10Y".to_string(), StoredBond::Fixed(bond));
        }

        // UK Gilt 30Y
        if let Some(bond) = create_fixed_bond(
            "UKT.30Y",
            dec!(0.0475),
            Date::from_ymd(2055, 7, 22).unwrap(),
            Date::from_ymd(2025, 7, 22).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::ActActIcma,
            Currency::GBP,
        ) {
            bonds.insert("UKT.30Y".to_string(), StoredBond::Fixed(bond));
        }

        // ========================================
        // Callable Bond
        // ========================================

        // Callable corporate (callable at par after 5 years)
        if let Some(base_bond) = create_fixed_bond(
            "CALLABLE.SAMPLE",
            dec!(0.0600),
            Date::from_ymd(2035, 6, 15).unwrap(),
            Date::from_ymd(2025, 6, 15).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
            Currency::USD,
        ) {
            let call_schedule = CallSchedule::new(CallType::American)
                .with_entry(CallEntry::new(Date::from_ymd(2030, 6, 15).unwrap(), 100.0))
                .with_entry(CallEntry::new(Date::from_ymd(2031, 6, 15).unwrap(), 100.0))
                .with_entry(CallEntry::new(Date::from_ymd(2032, 6, 15).unwrap(), 100.0));
            let callable = CallableBond::new(base_bond, call_schedule);
            bonds.insert(
                "CALLABLE.SAMPLE".to_string(),
                StoredBond::Callable(callable),
            );
        }

        // ========================================
        // Premium and Discount Bonds
        // ========================================

        // Premium bond (high coupon, trading above par)
        if let Some(bond) = create_fixed_bond(
            "PREMIUM.SAMPLE",
            dec!(0.0700),
            Date::from_ymd(2030, 3, 1).unwrap(),
            Date::from_ymd(2020, 3, 1).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
            Currency::USD,
        ) {
            bonds.insert("PREMIUM.SAMPLE".to_string(), StoredBond::Fixed(bond));
        }

        // Discount bond (low coupon, trading below par)
        if let Some(bond) = create_fixed_bond(
            "DISCOUNT.SAMPLE",
            dec!(0.0200),
            Date::from_ymd(2030, 3, 1).unwrap(),
            Date::from_ymd(2020, 3, 1).unwrap(),
            Frequency::SemiAnnual,
            DayCountConvention::Thirty360US,
            Currency::USD,
        ) {
            bonds.insert("DISCOUNT.SAMPLE".to_string(), StoredBond::Fixed(bond));
        }

        Self {
            reference_date,
            bonds,
            curves,
            market_description: String::from(
                "December 2025 market snapshot:\n\
                 - Fed Funds: 4.25-4.50% (post-cut cycle)\n\
                 - 10Y Treasury: ~4.45%\n\
                 - ECB deposit rate: ~2.75%\n\
                 - BOE rate: ~4.00%\n\
                 - USD IG spreads: 80-130bp\n\
                 - BTP-Bund 10Y spread: ~150bp\n\
                 - Settlement date: 2025-12-20",
            ),
        }
    }

    /// Get list of demo bonds with metadata
    pub fn list_demo_bonds(&self) -> Vec<DemoBondInfo> {
        let mut result = Vec::new();

        // Define approximate prices/yields for demo bonds
        let bond_info = [
            ("UST.2Y", "US Treasury 2Y On-the-Run", 99.85, 4.33),
            ("UST.5Y", "US Treasury 5Y", 99.50, 4.42),
            ("UST.10Y", "US Treasury 10Y On-the-Run", 100.25, 4.47),
            ("UST.30Y", "US Treasury 30Y", 101.50, 4.62),
            ("UST.10Y.OLD", "US Treasury 10Y Off-the-Run", 93.50, 4.55),
            ("AAPL.10Y", "Apple Inc 10Y (AA+)", 100.50, 4.80),
            ("MSFT.5Y", "Microsoft Corp 5Y (AAA)", 100.75, 4.45),
            ("JPM.10Y", "JPMorgan Chase 10Y (A)", 99.25, 5.68),
            ("HY.SAMPLE", "High Yield Sample (BB)", 97.50, 8.75),
            ("DBR.10Y", "German Bund 10Y", 100.00, 2.50),
            ("FRTR.10Y", "French OAT 10Y", 99.50, 3.05),
            ("BTPS.10Y", "Italian BTP 10Y", 98.00, 4.20),
            ("UKT.10Y", "UK Gilt 10Y", 100.75, 4.15),
            ("UKT.30Y", "UK Gilt 30Y", 101.25, 4.68),
            ("CALLABLE.SAMPLE", "Callable Corporate (NC5)", 102.50, 5.65),
            ("PREMIUM.SAMPLE", "Premium Bond (7% coupon)", 112.50, 4.85),
            ("DISCOUNT.SAMPLE", "Discount Bond (2% coupon)", 89.50, 4.60),
        ];

        for (id, desc, price, yield_pct) in bond_info {
            if let Some(bond) = self.bonds.get(id) {
                let (bond_type, currency, coupon, maturity) = match bond {
                    StoredBond::Fixed(b) => {
                        let coupon = b.coupon_rate_decimal().to_string().parse::<f64>().ok();
                        let mat = b.maturity().map(|d| d.to_string()).unwrap_or_default();
                        let ccy = format!("{:?}", b.currency());
                        ("Fixed", ccy, coupon.map(|c| c * 100.0), mat)
                    }
                    StoredBond::Callable(b) => {
                        let base = b.base_bond();
                        let coupon = base.coupon_rate_decimal().to_string().parse::<f64>().ok();
                        let mat = base.maturity().map(|d| d.to_string()).unwrap_or_default();
                        let ccy = format!("{:?}", base.currency());
                        ("Callable", ccy, coupon.map(|c| c * 100.0), mat)
                    }
                    StoredBond::Zero(b) => {
                        let mat = b.maturity().map(|d| d.to_string()).unwrap_or_default();
                        ("Zero", "USD".to_string(), None, mat)
                    }
                    StoredBond::Floating(b) => {
                        let mat = b.maturity().map(|d| d.to_string()).unwrap_or_default();
                        ("FRN", "USD".to_string(), None, mat)
                    }
                };

                result.push(DemoBondInfo {
                    id: id.to_string(),
                    description: desc.to_string(),
                    bond_type: bond_type.to_string(),
                    currency,
                    coupon,
                    maturity,
                    price,
                    yield_pct,
                });
            }
        }

        result
    }

    /// Get list of demo curves with metadata
    pub fn list_demo_curves(&self) -> Vec<DemoCurveInfo> {
        let mut result = Vec::new();

        let curve_info = [
            ("USD.TSY", "US Treasury Zero Curve", "USD", "Government"),
            ("USD.SOFR", "USD SOFR Swap Curve", "USD", "Swap"),
            ("USD.IG", "USD Investment Grade Credit", "USD", "Credit"),
            ("EUR.BUND", "German Bund Zero Curve", "EUR", "Government"),
            ("EUR.BTP", "Italian BTP Zero Curve", "EUR", "Government"),
            ("GBP.GILT", "UK Gilt Zero Curve", "GBP", "Government"),
        ];

        for (id, desc, currency, curve_type) in curve_info {
            if let Some(curve) = self.curves.get(id) {
                result.push(DemoCurveInfo {
                    id: id.to_string(),
                    description: desc.to_string(),
                    currency: currency.to_string(),
                    curve_type: curve_type.to_string(),
                    reference_date: self.reference_date.to_string(),
                    tenors: curve.inner().tenors().to_vec(),
                });
            }
        }

        result
    }
}

// ========================================
// Helper Functions
// ========================================

fn create_zero_curve(
    reference_date: Date,
    tenors: Vec<f64>,
    rates: Vec<f64>,
    day_count: DayCountConvention,
) -> Result<StoredCurve, convex_curves::CurveError> {
    let value_type = ValueType::ZeroRate {
        compounding: Compounding::Continuous,
        day_count,
    };

    let discrete = DiscreteCurve::new(
        reference_date,
        tenors,
        rates,
        value_type,
        InterpolationMethod::MonotoneConvex,
    )?;

    Ok(RateCurve::new(discrete))
}

fn create_fixed_bond(
    id: &str,
    coupon_rate: Decimal,
    maturity: Date,
    issue_date: Date,
    frequency: Frequency,
    day_count: DayCountConvention,
    currency: Currency,
) -> Option<FixedRateBond> {
    FixedRateBond::builder()
        .cusip_unchecked(id)
        .coupon_rate(coupon_rate)
        .maturity(maturity)
        .issue_date(issue_date)
        .frequency(frequency)
        .day_count(day_count)
        .currency(currency)
        .face_value(dec!(100))
        .build()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demo_data_creation() {
        let demo = DemoData::december_2025();

        // Should have bonds
        assert!(!demo.bonds.is_empty());
        assert!(demo.bonds.contains_key("UST.10Y"));
        assert!(demo.bonds.contains_key("AAPL.10Y"));

        // Should have curves
        assert!(!demo.curves.is_empty());
        assert!(demo.curves.contains_key("USD.TSY"));
        assert!(demo.curves.contains_key("EUR.BUND"));

        // Reference date should be December 2025
        assert_eq!(demo.reference_date.year(), 2025);
        assert_eq!(demo.reference_date.month(), 12);
    }

    #[test]
    fn test_list_demo_bonds() {
        let demo = DemoData::december_2025();
        let bonds = demo.list_demo_bonds();

        assert!(!bonds.is_empty());

        // Check a known bond
        let ust_10y = bonds.iter().find(|b| b.id == "UST.10Y");
        assert!(ust_10y.is_some());
        assert_eq!(ust_10y.unwrap().bond_type, "Fixed");
    }

    #[test]
    fn test_list_demo_curves() {
        let demo = DemoData::december_2025();
        let curves = demo.list_demo_curves();

        assert!(!curves.is_empty());

        // Check a known curve
        let usd_tsy = curves.iter().find(|c| c.id == "USD.TSY");
        assert!(usd_tsy.is_some());
        assert_eq!(usd_tsy.unwrap().currency, "USD");
    }
}
