//! Application state.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use convex_bonds::prelude::ZeroCouponBond;
use convex_bonds::{CallableBond, FixedRateBond, FloatingRateNote};
use convex_curves::{DiscreteCurve, RateCurve};

/// Stored bond variants.
#[derive(Debug, Clone)]
pub enum StoredBond {
    Fixed(FixedRateBond),
    Zero(ZeroCouponBond),
    Callable(CallableBond),
    Floating(FloatingRateNote),
}

impl StoredBond {
    /// Get the bond type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            StoredBond::Fixed(_) => "Fixed Rate",
            StoredBond::Zero(_) => "Zero Coupon",
            StoredBond::Callable(_) => "Callable",
            StoredBond::Floating(_) => "Floating Rate Note",
        }
    }
}

/// Stored curve type.
pub type StoredCurve = RateCurve<DiscreteCurve>;

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    /// Stored bonds.
    pub bonds: Arc<RwLock<HashMap<String, StoredBond>>>,

    /// Stored curves.
    pub curves: Arc<RwLock<HashMap<String, StoredCurve>>>,

    /// Whether demo mode is enabled.
    pub demo_mode: bool,
}

impl AppState {
    /// Create a new empty state.
    pub fn new() -> Self {
        Self {
            bonds: Arc::new(RwLock::new(HashMap::new())),
            curves: Arc::new(RwLock::new(HashMap::new())),
            demo_mode: false,
        }
    }

    /// Create state with demo mode enabled.
    pub fn with_demo_mode() -> Self {
        let mut state = Self::new();
        state.demo_mode = true;
        state.load_demo_data();
        state
    }

    /// Load demo data.
    fn load_demo_data(&mut self) {
        use convex_bonds::types::BondIdentifiers;
        use convex_core::daycounts::DayCountConvention;
        use convex_core::types::{Date, Frequency};
        use convex_curves::{InterpolationMethod, ValueType};
        use rust_decimal::Decimal;

        // Reference date: December 2025
        let ref_date = Date::from_ymd(2025, 12, 20).unwrap();

        // Create demo bonds
        let bonds = vec![
            (
                "UST.10Y",
                FixedRateBond::builder()
                    .identifiers(BondIdentifiers::new())
                    .coupon_rate(Decimal::new(425, 4)) // 4.25%
                    .maturity(Date::from_ymd(2034, 11, 15).unwrap())
                    .issue_date(Date::from_ymd(2024, 11, 15).unwrap())
                    .frequency(Frequency::SemiAnnual)
                    .us_treasury()
                    .build()
                    .unwrap(),
            ),
            (
                "UST.5Y",
                FixedRateBond::builder()
                    .identifiers(BondIdentifiers::new())
                    .coupon_rate(Decimal::new(400, 4)) // 4.00%
                    .maturity(Date::from_ymd(2029, 12, 15).unwrap())
                    .issue_date(Date::from_ymd(2024, 12, 15).unwrap())
                    .frequency(Frequency::SemiAnnual)
                    .us_treasury()
                    .build()
                    .unwrap(),
            ),
            (
                "CORP.AAPL",
                FixedRateBond::builder()
                    .identifiers(BondIdentifiers::new())
                    .coupon_rate(Decimal::new(475, 4)) // 4.75%
                    .maturity(Date::from_ymd(2030, 5, 15).unwrap())
                    .issue_date(Date::from_ymd(2020, 5, 15).unwrap())
                    .frequency(Frequency::SemiAnnual)
                    .us_corporate()
                    .build()
                    .unwrap(),
            ),
            (
                "CORP.MSFT",
                FixedRateBond::builder()
                    .identifiers(BondIdentifiers::new())
                    .coupon_rate(Decimal::new(350, 4)) // 3.50%
                    .maturity(Date::from_ymd(2028, 2, 15).unwrap())
                    .issue_date(Date::from_ymd(2018, 2, 15).unwrap())
                    .frequency(Frequency::SemiAnnual)
                    .us_corporate()
                    .build()
                    .unwrap(),
            ),
        ];

        for (id, bond) in bonds {
            self.bonds
                .write()
                .unwrap()
                .insert(id.to_string(), StoredBond::Fixed(bond));
        }

        // Create demo curves
        let tsy_tenors = vec![0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 20.0, 30.0];
        let tsy_rates = vec![
            0.0435, 0.0432, 0.0425, 0.0418, 0.0415, 0.0420, 0.0428, 0.0435, 0.0455, 0.0460,
        ];

        let tsy_curve = DiscreteCurve::new(
            ref_date,
            tsy_tenors,
            tsy_rates,
            ValueType::continuous_zero(DayCountConvention::Act365Fixed),
            InterpolationMethod::MonotoneConvex,
        )
        .unwrap();

        self.curves
            .write()
            .unwrap()
            .insert("UST".to_string(), RateCurve::new(tsy_curve));

        let sofr_tenors = vec![0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0];
        let sofr_rates = vec![0.0440, 0.0438, 0.0432, 0.0428, 0.0430, 0.0435, 0.0442, 0.0448];

        let sofr_curve = DiscreteCurve::new(
            ref_date,
            sofr_tenors,
            sofr_rates,
            ValueType::continuous_zero(DayCountConvention::Act365Fixed),
            InterpolationMethod::MonotoneConvex,
        )
        .unwrap();

        self.curves
            .write()
            .unwrap()
            .insert("SOFR".to_string(), RateCurve::new(sofr_curve));

        tracing::info!(
            "Loaded demo data: {} bonds, {} curves",
            self.bonds.read().unwrap().len(),
            self.curves.read().unwrap().len()
        );
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
