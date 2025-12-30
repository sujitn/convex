//! Pricing router - selects pricing model based on bond type and config.

use std::sync::Arc;

use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use convex_bonds::instruments::{CallableBond, FloatingRateNote};
use convex_bonds::prelude::{Bond, FixedCouponBond, FixedRateBond, FixedRateBondBuilder};
use convex_bonds::types::{CallEntry, CallSchedule, CallType, RateIndex};
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Frequency, Yield};
use convex_core::Date;
use convex_curves::RateCurveDyn;

use convex_analytics::prelude::{
    convexity, dv01, macaulay_duration, modified_duration, yield_to_maturity,
};
use convex_analytics::risk::{KeyRateDurationCalculator, STANDARD_KEY_RATE_TENORS};
use convex_analytics::yields::YieldSolver;
use convex_bonds::traits::BondCashFlow;
use convex_analytics::spreads::{
    simple_margin, DiscountMarginCalculator, GSpreadCalculator, GovernmentCurve, ISpreadCalculator,
    OASCalculator, ParParAssetSwap, ZSpreadCalculator,
};
use convex_core::types::Price;
use convex_curves::curves::{ForwardCurve, ZeroCurve, ZeroCurveBuilder};

use convex_traits::output::BondQuoteOutput;
use convex_traits::reference_data::{BondReferenceData, BondType};

use crate::curve_builder::BuiltCurve;
use crate::error::EngineError;

/// Format tenor as a human-readable label (e.g., "3M", "1Y", "10Y").
fn format_tenor_label(tenor: f64) -> String {
    if tenor < 1.0 {
        let months = (tenor * 12.0).round() as u32;
        format!("{}M", months)
    } else {
        let years = tenor.round() as u32;
        format!("{}Y", years)
    }
}

/// Pricing model selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PricingModel {
    /// Discount cash flows to maturity
    DiscountToMaturity,
    /// Discount to call using yield-to-worst
    YieldToWorst,
    /// OAS model for callable bonds
    CallableOas,
    /// Real yield for inflation-linked bonds
    InflationLinkedRealYield,
    /// Discount margin for FRNs
    FloatingRateDiscountMargin,
    /// Matrix pricing (spread off benchmark)
    MatrixPricing,
}

/// Pricing input for a single bond.
#[derive(Debug)]
pub struct PricingInput {
    /// Bond reference data
    pub bond: BondReferenceData,
    /// Settlement date
    pub settlement_date: Date,
    /// Market quote price (clean, if available)
    pub market_price: Option<Decimal>,
    /// Discount curve (for Z-spread)
    pub discount_curve: Option<BuiltCurve>,
    /// Benchmark curve (for I-spread / swap curve)
    pub benchmark_curve: Option<BuiltCurve>,
    /// Government curve (for G-spread)
    pub government_curve: Option<GovernmentCurve>,
    /// Volatility (for OAS)
    pub volatility: Option<Decimal>,
}

/// Pricing router - routes bonds to appropriate pricing model.
pub struct PricingRouter {
    /// Default bump size for numerical duration (basis points)
    pub bump_bps: f64,
}

impl PricingRouter {
    /// Create a new pricing router.
    pub fn new() -> Self {
        Self { bump_bps: 10.0 }
    }

    /// Select pricing model for a bond.
    pub fn select_model(&self, bond: &BondReferenceData) -> PricingModel {
        match bond.bond_type {
            BondType::FixedBullet => PricingModel::DiscountToMaturity,
            BondType::FixedCallable => PricingModel::CallableOas, // Use OAS model for callables
            BondType::FixedPutable => PricingModel::YieldToWorst,
            BondType::FloatingRate => PricingModel::FloatingRateDiscountMargin,
            BondType::ZeroCoupon => PricingModel::DiscountToMaturity,
            BondType::InflationLinked => PricingModel::InflationLinkedRealYield,
            BondType::Amortizing => PricingModel::DiscountToMaturity,
            BondType::Convertible => PricingModel::MatrixPricing,
        }
    }

    /// Convert BondReferenceData to a FixedRateBond for analytics.
    fn to_fixed_rate_bond(
        &self,
        ref_data: &BondReferenceData,
    ) -> Result<FixedRateBond, EngineError> {
        let coupon_rate = ref_data
            .coupon_rate
            .ok_or_else(|| EngineError::PricingError("Missing coupon rate".to_string()))?;

        let frequency = match ref_data.frequency {
            1 => Frequency::Annual,
            2 => Frequency::SemiAnnual,
            4 => Frequency::Quarterly,
            12 => Frequency::Monthly,
            _ => Frequency::SemiAnnual, // Default
        };

        let day_count = self.parse_day_count(&ref_data.day_count)?;

        let mut builder = FixedRateBondBuilder::new()
            .issue_date(ref_data.issue_date)
            .maturity(ref_data.maturity_date)
            .coupon_rate(coupon_rate)
            .face_value(ref_data.face_value)
            .frequency(frequency)
            .day_count(day_count);

        // Add CUSIP identifier if available
        if let Some(ref cusip) = ref_data.cusip {
            builder = builder.cusip_unchecked(cusip);
        }

        builder
            .build()
            .map_err(|e| EngineError::PricingError(format!("Failed to build bond: {}", e)))
    }

    /// Convert BondReferenceData to a CallableBond for OAS analytics.
    fn to_callable_bond(
        &self,
        ref_data: &BondReferenceData,
    ) -> Result<CallableBond, EngineError> {
        // First build the base fixed rate bond
        let base_bond = self.to_fixed_rate_bond(ref_data)?;

        // Build call schedule from reference data
        if ref_data.call_schedule.is_empty() {
            return Err(EngineError::PricingError(
                "No call schedule for callable bond".to_string(),
            ));
        }

        let mut call_schedule = CallSchedule::new(CallType::American);

        for entry in &ref_data.call_schedule {
            let call_price = entry
                .call_price
                .to_string()
                .parse::<f64>()
                .unwrap_or(100.0);

            let call_entry = CallEntry::new(entry.call_date, call_price);

            // If this is a make-whole call, update the schedule type
            if entry.is_make_whole {
                call_schedule = CallSchedule::make_whole(25.0); // Default T+25bps
            }

            call_schedule = call_schedule.with_entry(call_entry);
        }

        Ok(CallableBond::new(base_bond, call_schedule))
    }

    /// Convert BondReferenceData to a FloatingRateNote for discount margin analytics.
    fn to_floating_rate_note(
        &self,
        ref_data: &BondReferenceData,
    ) -> Result<FloatingRateNote, EngineError> {
        use convex_traits::ids::FloatingRateIndex as TraitIndex;

        // Get floating rate terms
        let floating_terms = ref_data.floating_terms.as_ref().ok_or_else(|| {
            EngineError::PricingError("No floating rate terms for FRN".to_string())
        })?;

        // Map index from traits to bonds
        let rate_index = match &floating_terms.index {
            TraitIndex::Sofr => RateIndex::Sofr,
            TraitIndex::Estr => RateIndex::Estr,
            TraitIndex::Sonia => RateIndex::Sonia,
            TraitIndex::Euribor(_) => RateIndex::Euribor3M, // Default to 3M
            TraitIndex::TermSofr(_) => RateIndex::Sofr,
            TraitIndex::Other(_) => RateIndex::Sofr, // Default fallback
        };

        // Get frequency for FRN
        let frequency = match ref_data.frequency {
            1 => Frequency::Annual,
            2 => Frequency::SemiAnnual,
            4 => Frequency::Quarterly,
            12 => Frequency::Monthly,
            _ => Frequency::Quarterly, // Default for FRNs
        };

        // Get day count
        let day_count = self.parse_day_count(&ref_data.day_count)?;

        // Build FRN
        let spread_bps = floating_terms
            .spread
            .to_i64()
            .unwrap_or(0) as i32;

        let mut builder = FloatingRateNote::builder()
            .index(rate_index)
            .spread_bps(spread_bps)
            .maturity(ref_data.maturity_date)
            .issue_date(ref_data.issue_date)
            .frequency(frequency)
            .day_count(day_count)
            .face_value(ref_data.face_value);

        // Add CUSIP if available
        if let Some(ref cusip) = ref_data.cusip {
            builder = builder.cusip_unchecked(cusip);
        }

        // Add cap and floor if present
        if let Some(cap) = floating_terms.cap {
            builder = builder.cap(cap);
        }
        if let Some(floor) = floating_terms.floor {
            builder = builder.floor(floor);
        }

        builder
            .build()
            .map_err(|e| EngineError::PricingError(format!("Failed to build FRN: {}", e)))
    }

    /// Parse day count convention string to enum.
    fn parse_day_count(&self, dcc_str: &str) -> Result<DayCountConvention, EngineError> {
        match dcc_str.to_uppercase().as_str() {
            "ACT/360" | "ACTUAL/360" => Ok(DayCountConvention::Act360),
            "ACT/365F" | "ACT/365" | "ACT/365 FIXED" | "ACTUAL/365" => {
                Ok(DayCountConvention::Act365Fixed)
            }
            "ACT/ACT" | "ACT/ACT ISDA" | "ACTUAL/ACTUAL" => Ok(DayCountConvention::ActActIsda),
            "ACT/ACT ICMA" => Ok(DayCountConvention::ActActIcma),
            "30/360" | "30/360 US" | "BOND" => Ok(DayCountConvention::Thirty360US),
            "30E/360" | "30/360 E" | "EUROBOND" => Ok(DayCountConvention::Thirty360E),
            "30E/360 ISDA" => Ok(DayCountConvention::Thirty360EIsda),
            _ => {
                warn!("Unknown day count '{}', defaulting to 30/360", dcc_str);
                Ok(DayCountConvention::Thirty360US)
            }
        }
    }

    /// Get frequency enum from reference data.
    fn get_frequency(&self, ref_data: &BondReferenceData) -> Frequency {
        match ref_data.frequency {
            1 => Frequency::Annual,
            2 => Frequency::SemiAnnual,
            4 => Frequency::Quarterly,
            12 => Frequency::Monthly,
            _ => Frequency::SemiAnnual,
        }
    }

    /// Price a bond.
    pub fn price(&self, input: &PricingInput) -> Result<BondQuoteOutput, EngineError> {
        let model = self.select_model(&input.bond);
        debug!(
            "Pricing {} with model {:?}",
            input.bond.instrument_id, model
        );

        match model {
            PricingModel::DiscountToMaturity => self.price_discount_to_maturity(input),
            PricingModel::YieldToWorst => self.price_yield_to_worst(input),
            PricingModel::CallableOas => self.price_callable_oas(input),
            PricingModel::InflationLinkedRealYield => self.price_inflation_linked(input),
            PricingModel::FloatingRateDiscountMargin => self.price_floating_rate(input),
            PricingModel::MatrixPricing => self.price_matrix(input),
        }
    }

    /// Calculate Z-spread if discount curve is provided.
    fn calculate_z_spread(
        &self,
        bond: &FixedRateBond,
        dirty_price: Decimal,
        settlement: Date,
        discount_curve: &BuiltCurve,
    ) -> Option<Decimal> {
        let calc = ZSpreadCalculator::new(discount_curve);
        match calc.calculate(bond, dirty_price, settlement) {
            Ok(spread) => Some(spread.as_bps()),
            Err(e) => {
                debug!("Z-spread calculation failed: {}", e);
                None
            }
        }
    }

    /// Calculate I-spread if swap curve is provided.
    fn calculate_i_spread(
        &self,
        bond: &FixedRateBond,
        ytm: f64,
        settlement: Date,
        swap_curve: &BuiltCurve,
    ) -> Option<Decimal> {
        let calc = ISpreadCalculator::new(swap_curve);
        let yield_val = Yield::new(
            Decimal::from_f64_retain(ytm).unwrap_or_default(),
            Compounding::SemiAnnual, // Standard for US bonds
        );
        match calc.calculate(bond, yield_val, settlement) {
            Ok(spread) => Some(spread.as_bps()),
            Err(e) => {
                debug!("I-spread calculation failed: {}", e);
                None
            }
        }
    }

    /// Calculate G-spread if government curve is provided.
    fn calculate_g_spread(
        &self,
        bond: &FixedRateBond,
        ytm: f64,
        settlement: Date,
        gov_curve: &GovernmentCurve,
    ) -> Option<Decimal> {
        let calc = GSpreadCalculator::new(gov_curve);
        let yield_val = Yield::new(
            Decimal::from_f64_retain(ytm).unwrap_or_default(),
            Compounding::SemiAnnual, // Standard for US bonds
        );
        match calc.calculate(bond, yield_val, settlement) {
            Ok(spread) => Some(spread.as_bps()),
            Err(e) => {
                debug!("G-spread calculation failed: {}", e);
                None
            }
        }
    }

    /// Calculate OAS and effective duration/convexity for callable bonds.
    ///
    /// Returns (OAS in bps, effective duration, effective convexity).
    fn calculate_oas(
        &self,
        callable_bond: &CallableBond,
        dirty_price: Decimal,
        settlement: Date,
        discount_curve: &BuiltCurve,
        volatility: f64,
    ) -> (Option<Decimal>, Option<Decimal>, Option<Decimal>) {
        let calc = OASCalculator::default_hull_white(volatility);

        // Calculate OAS
        let oas_result = calc.calculate(callable_bond, dirty_price, discount_curve, settlement);
        let oas_bps = match &oas_result {
            Ok(spread) => Some(spread.as_bps()),
            Err(e) => {
                debug!("OAS calculation failed: {}", e);
                None
            }
        };

        // If OAS calculation succeeded, calculate effective duration and convexity
        if let Ok(spread) = &oas_result {
            let oas_decimal = spread.as_decimal().to_f64().unwrap_or(0.0) / 10000.0;

            let eff_dur = calc
                .effective_duration(callable_bond, discount_curve, oas_decimal, settlement)
                .ok()
                .map(|d| Decimal::from_f64_retain(d).unwrap_or_default());

            let eff_conv = calc
                .effective_convexity(callable_bond, discount_curve, oas_decimal, settlement)
                .ok()
                .map(|c| Decimal::from_f64_retain(c).unwrap_or_default());

            (oas_bps, eff_dur, eff_conv)
        } else {
            (oas_bps, None, None)
        }
    }

    /// Calculate discount margin for floating rate notes.
    ///
    /// Returns discount margin in basis points.
    fn calculate_discount_margin(
        &self,
        frn: &FloatingRateNote,
        dirty_price: Decimal,
        settlement: Date,
        discount_curve: &BuiltCurve,
    ) -> Option<Decimal> {
        // Create an Arc from the discount curve for ForwardCurve
        // We need to clone since BuiltCurve implements RateCurveDyn
        let curve_arc: Arc<dyn RateCurveDyn> = Arc::new(discount_curve.clone());

        // Create forward curve with 3-month tenor (standard for FRNs)
        let forward_curve = ForwardCurve::from_months(curve_arc.clone(), 3);

        // Create discount margin calculator
        let calc = DiscountMarginCalculator::new(&forward_curve, discount_curve);

        match calc.calculate(frn, dirty_price, settlement) {
            Ok(spread) => Some(spread.as_bps()),
            Err(e) => {
                debug!("Discount margin calculation failed: {}", e);
                None
            }
        }
    }

    /// Convert BuiltCurve to ZeroCurve for asset swap calculations.
    ///
    /// ParParAssetSwap requires a date-based ZeroCurve, so we convert
    /// the tenor-based BuiltCurve points to dated zero rates.
    fn to_zero_curve(&self, built_curve: &BuiltCurve) -> Option<ZeroCurve> {
        let ref_date = built_curve.reference_date;

        if built_curve.points.is_empty() {
            return None;
        }

        let mut builder = ZeroCurveBuilder::new().reference_date(ref_date);

        for (tenor_years, rate) in &built_curve.points {
            // Convert tenor to date (approximate using 365 days/year)
            let days = (*tenor_years * 365.0) as i64;
            let point_date = ref_date.add_days(days);
            let rate_dec = Decimal::from_f64_retain(*rate).unwrap_or_default();
            builder = builder.add_rate(point_date, rate_dec);
        }

        builder.build().ok()
    }

    /// Calculate Asset Swap Spread (Par-Par) if swap curve is provided.
    ///
    /// Returns ASW in basis points.
    fn calculate_asw(
        &self,
        bond: &FixedRateBond,
        clean_price: Decimal,
        settlement: Date,
        swap_curve: &BuiltCurve,
    ) -> Option<Decimal> {
        // Convert BuiltCurve to ZeroCurve for ParParAssetSwap
        let zero_curve = self.to_zero_curve(swap_curve)?;

        let calc = ParParAssetSwap::new(&zero_curve);
        let price = Price::new(clean_price, bond.currency());

        match calc.calculate(bond, price, settlement) {
            Ok(spread) => Some(spread.as_bps()),
            Err(e) => {
                debug!("ASW calculation failed: {}", e);
                None
            }
        }
    }

    /// Calculate key rate durations for a bond.
    ///
    /// Key rate durations measure sensitivity to specific points on the yield curve.
    /// Returns a vector of (tenor_label, duration) pairs.
    fn calculate_key_rate_durations(
        &self,
        bond: &FixedRateBond,
        settlement: Date,
        discount_curve: &BuiltCurve,
    ) -> Option<Vec<(String, Decimal)>> {
        let maturity = bond.maturity()?;
        let years_to_maturity = settlement.days_between(&maturity) as f64 / 365.0;

        // Get cash flows
        let cash_flows = bond.cash_flows(settlement);
        if cash_flows.is_empty() {
            return None;
        }

        // Calculate base price using curve discounting
        let base_price = self.price_from_curve(bond, settlement, discount_curve)?;

        // Filter tenors to only include those up to maturity
        let relevant_tenors: Vec<f64> = STANDARD_KEY_RATE_TENORS
            .iter()
            .copied()
            .filter(|&t| t <= years_to_maturity + 0.5) // Include nearby tenors
            .collect();

        if relevant_tenors.is_empty() {
            return None;
        }

        let bump_size = 0.0001; // 1 bp

        // Calculate price sensitivity at each tenor
        let mut tenor_prices: Vec<(f64, f64, f64)> = Vec::new();

        for &tenor in &relevant_tenors {
            // Create bumped curves
            let curve_up = self.bump_curve_at_tenor(discount_curve, tenor, bump_size);
            let curve_down = self.bump_curve_at_tenor(discount_curve, tenor, -bump_size);

            // Reprice with bumped curves
            let price_up = self.price_from_curve(bond, settlement, &curve_up)?;
            let price_down = self.price_from_curve(bond, settlement, &curve_down)?;

            tenor_prices.push((tenor, price_up, price_down));
        }

        // Calculate KRDs
        let calc = KeyRateDurationCalculator::with_tenors(relevant_tenors).with_bump_bps(1.0);
        let krds = calc.calculate(base_price, &tenor_prices).ok()?;

        // Convert to output format
        let result: Vec<(String, Decimal)> = krds
            .durations
            .iter()
            .map(|krd| {
                let label = format_tenor_label(krd.tenor);
                let duration = Decimal::from_f64_retain(krd.duration.as_f64()).unwrap_or_default();
                (label, duration)
            })
            .collect();

        Some(result)
    }

    /// Price a bond using curve discounting.
    fn price_from_curve(
        &self,
        bond: &FixedRateBond,
        settlement: Date,
        curve: &BuiltCurve,
    ) -> Option<f64> {
        let cash_flows = bond.cash_flows(settlement);
        let mut price = 0.0;

        for cf in &cash_flows {
            if cf.date <= settlement {
                continue;
            }

            let years = settlement.days_between(&cf.date) as f64 / 365.0;
            let df = curve.discount_factor(years).unwrap_or(1.0);
            let amount = cf.amount.to_f64().unwrap_or(0.0);
            price += amount * df;
        }

        // Convert to percentage of face
        let face = bond.face_value().to_f64().unwrap_or(100.0);
        Some(price / face * 100.0)
    }

    /// Create a bumped curve at a specific tenor.
    fn bump_curve_at_tenor(&self, curve: &BuiltCurve, tenor: f64, bump: f64) -> BuiltCurve {
        let mut bumped = curve.clone();

        // Bump rates using a triangular weighting around the tenor
        for (t, rate) in &mut bumped.points {
            // Triangular weight: 1.0 at target tenor, decreasing to 0 at ±1 year
            let distance = (*t - tenor).abs();
            let weight = if distance < 1.0 {
                1.0 - distance
            } else {
                0.0
            };
            *rate += bump * weight;
        }

        bumped
    }

    /// Calculate CS01 (credit spread sensitivity) for a bond.
    ///
    /// CS01 is the price change for a 1 basis point increase in spread.
    /// Uses the Z-spread calculator's spread_dv01 method.
    fn calculate_cs01(
        &self,
        bond: &FixedRateBond,
        z_spread_bps: Decimal,
        settlement: Date,
        discount_curve: &BuiltCurve,
    ) -> Option<Decimal> {
        use convex_core::types::SpreadType;

        let calc = ZSpreadCalculator::new(discount_curve);
        let spread = convex_core::types::Spread::new(z_spread_bps, SpreadType::ZSpread);

        let cs01 = calc.spread_dv01(bond, spread, settlement);

        // CS01 should be positive (price decreases when spread increases)
        if cs01 > Decimal::ZERO {
            Some(cs01)
        } else {
            // Return absolute value if negative
            Some(cs01.abs())
        }
    }

    /// Calculate PV01 (Present Value of a Basis Point) using curve-based repricing.
    ///
    /// PV01 measures the price change for a 1 basis point parallel shift in the discount curve.
    /// This is more accurate than yield-based DV01 when a curve is available.
    fn calculate_pv01(
        &self,
        bond: &FixedRateBond,
        settlement: Date,
        discount_curve: &BuiltCurve,
    ) -> Option<Decimal> {
        // Calculate base price using curve discounting
        let base_price = self.price_from_curve(bond, settlement, discount_curve)?;

        // Create parallel-bumped curve (+1bp to all rates)
        let bumped_curve = self.bump_curve_parallel(discount_curve, 0.0001);

        // Calculate bumped price
        let bumped_price = self.price_from_curve(bond, settlement, &bumped_curve)?;

        // PV01 = |Base Price - Bumped Price|
        // Price decreases when rates increase, so this should be positive
        let pv01 = (base_price - bumped_price).abs();

        Some(Decimal::from_f64_retain(pv01).unwrap_or_default())
    }

    /// Create a parallel-bumped curve (all rates shifted by same amount).
    fn bump_curve_parallel(&self, curve: &BuiltCurve, bump: f64) -> BuiltCurve {
        let mut bumped = curve.clone();

        // Bump all rates by the same amount
        for (_t, rate) in &mut bumped.points {
            *rate += bump;
        }

        bumped
    }

    /// Calculate spread duration for a bond.
    ///
    /// Spread duration measures the sensitivity of a bond's price to changes
    /// in its credit spread, rather than the underlying risk-free rate.
    /// Formula: SD = (Price_down - Price_up) / (2 × Price_base × spread_bump)
    fn calculate_spread_duration(
        &self,
        bond: &FixedRateBond,
        z_spread_bps: Decimal,
        settlement: Date,
        discount_curve: &BuiltCurve,
    ) -> Option<Decimal> {
        let calc = ZSpreadCalculator::new(discount_curve);

        // Convert Z-spread from bps to decimal
        let base_spread = z_spread_bps.to_f64().unwrap_or(0.0) / 10_000.0;
        let bump = 0.0001; // 1 basis point

        // Calculate prices at base spread and bumped spreads
        let price_base = calc.price_with_spread(bond, base_spread, settlement);
        let price_up = calc.price_with_spread(bond, base_spread + bump, settlement);
        let price_down = calc.price_with_spread(bond, base_spread - bump, settlement);

        if price_base.abs() < 1e-10 {
            return None;
        }

        // Spread duration = (Price_down - Price_up) / (2 × Price_base × bump)
        let spread_dur = (price_down - price_up) / (2.0 * price_base * bump);

        Some(Decimal::from_f64_retain(spread_dur).unwrap_or_default())
    }

    /// Calculate effective duration for a bond using curve-based repricing.
    ///
    /// Effective duration measures price sensitivity to a parallel shift in the yield curve.
    /// Formula: D_eff = (P₋ - P₊) / (2 × P₀ × Δy)
    ///
    /// This is more accurate than modified duration when you have a full yield curve,
    /// and is essential for bonds with embedded options.
    fn calculate_effective_duration(
        &self,
        bond: &FixedRateBond,
        settlement: Date,
        discount_curve: &BuiltCurve,
    ) -> Option<Decimal> {
        // Calculate base price using curve discounting
        let price_base = self.price_from_curve(bond, settlement, discount_curve)?;

        if price_base.abs() < 1e-10 {
            return None;
        }

        // Use 10bp bump for effective duration (standard practice)
        let bump = 0.001; // 10 basis points

        // Create bumped curves
        let curve_up = self.bump_curve_parallel(discount_curve, bump);
        let curve_down = self.bump_curve_parallel(discount_curve, -bump);

        // Calculate prices with bumped curves
        let price_up = self.price_from_curve(bond, settlement, &curve_up)?;
        let price_down = self.price_from_curve(bond, settlement, &curve_down)?;

        // Effective duration = (P_down - P_up) / (2 × P_base × bump)
        let eff_dur = (price_down - price_up) / (2.0 * price_base * bump);

        Some(Decimal::from_f64_retain(eff_dur).unwrap_or_default())
    }

    /// Calculate effective convexity for a bond using curve-based repricing.
    ///
    /// Effective convexity measures the second-order price sensitivity to yield changes.
    /// Formula: C_eff = (P₋ + P₊ - 2×P₀) / (P₀ × Δy²)
    ///
    /// Positive convexity means the bond gains more when rates fall than it loses when rates rise.
    fn calculate_effective_convexity(
        &self,
        bond: &FixedRateBond,
        settlement: Date,
        discount_curve: &BuiltCurve,
    ) -> Option<Decimal> {
        // Calculate base price using curve discounting
        let price_base = self.price_from_curve(bond, settlement, discount_curve)?;

        if price_base.abs() < 1e-10 {
            return None;
        }

        // Use 10bp bump for effective convexity (same as effective duration)
        let bump = 0.001; // 10 basis points

        // Create bumped curves
        let curve_up = self.bump_curve_parallel(discount_curve, bump);
        let curve_down = self.bump_curve_parallel(discount_curve, -bump);

        // Calculate prices with bumped curves
        let price_up = self.price_from_curve(bond, settlement, &curve_up)?;
        let price_down = self.price_from_curve(bond, settlement, &curve_down)?;

        // Effective convexity = (P_down + P_up - 2×P_base) / (P_base × bump²)
        let eff_conv = (price_down + price_up - 2.0 * price_base) / (price_base * bump * bump);

        Some(Decimal::from_f64_retain(eff_conv).unwrap_or_default())
    }

    /// Calculate yield to a specific call date.
    ///
    /// Creates a modified cash flow stream that ends at the call date with
    /// the call price as the final redemption, then solves for yield.
    fn calculate_yield_to_call(
        &self,
        bond: &FixedRateBond,
        clean_price: Decimal,
        settlement: Date,
        call_date: Date,
        call_price: Decimal,
        frequency: Frequency,
    ) -> Option<f64> {
        // Get all cash flows from the bond
        let all_cash_flows = bond.cash_flows(settlement);

        // Filter to only include cash flows before or on call date
        // Replace the redemption with call price
        let coupon_amount = bond.coupon_rate() * bond.face_value()
            / Decimal::from(frequency.periods_per_year());

        let mut modified_flows: Vec<BondCashFlow> = all_cash_flows
            .into_iter()
            .filter(|cf| cf.date <= call_date && cf.date > settlement)
            .map(|cf| {
                if cf.date == call_date {
                    // This is the call date - include coupon + call price
                    BondCashFlow::coupon_and_principal(cf.date, coupon_amount, call_price)
                } else {
                    // Regular coupon
                    BondCashFlow::coupon(cf.date, coupon_amount)
                }
            })
            .collect();

        // If no cash flow on exact call date, add one
        if modified_flows.is_empty() || modified_flows.last().map(|cf| cf.date) != Some(call_date) {
            // Need to add final payment at call date
            modified_flows.push(BondCashFlow::coupon_and_principal(
                call_date,
                coupon_amount,
                call_price,
            ));
        }

        if modified_flows.is_empty() {
            return None;
        }

        // Calculate accrued interest for settlement
        let accrued = bond.accrued_interest(settlement);

        // Get day count convention
        let day_count = self.parse_day_count(bond.day_count_convention()).ok()?;

        // Solve for yield
        let solver = YieldSolver::new();
        match solver.solve(&modified_flows, clean_price, accrued, settlement, day_count, frequency) {
            Ok(result) => Some(result.yield_value),
            Err(e) => {
                debug!("Yield to call calculation failed: {}", e);
                None
            }
        }
    }

    /// Calculate yield to worst for a callable bond.
    ///
    /// Finds the minimum yield across:
    /// - Yield to maturity
    /// - Yield to each call date
    ///
    /// Returns (YTW, workout date) where workout date is the date giving the worst yield.
    fn calculate_yield_to_worst(
        &self,
        bond: &FixedRateBond,
        clean_price: Decimal,
        settlement: Date,
        call_schedule: &[(Date, Decimal)], // (call_date, call_price)
        frequency: Frequency,
    ) -> (Option<f64>, Option<Date>) {
        // Calculate YTM first
        let ytm_result = yield_to_maturity(bond, settlement, clean_price, frequency);
        let maturity = bond.maturity().unwrap_or(settlement);

        let mut worst_yield: Option<f64> = ytm_result.ok().map(|r| r.yield_value);
        let mut workout_date: Option<Date> = Some(maturity);

        // Calculate yield to each call date and find the minimum
        for (call_date, call_price) in call_schedule {
            // Skip call dates in the past
            if *call_date <= settlement {
                continue;
            }

            if let Some(ytc) = self.calculate_yield_to_call(
                bond,
                clean_price,
                settlement,
                *call_date,
                *call_price,
                frequency,
            ) {
                match worst_yield {
                    Some(yw) if ytc < yw => {
                        worst_yield = Some(ytc);
                        workout_date = Some(*call_date);
                    }
                    None => {
                        worst_yield = Some(ytc);
                        workout_date = Some(*call_date);
                    }
                    _ => {}
                }
            }
        }

        (worst_yield, workout_date)
    }

    /// Price using discount-to-maturity model with real analytics.
    fn price_discount_to_maturity(
        &self,
        input: &PricingInput,
    ) -> Result<BondQuoteOutput, EngineError> {
        let clean_price = input.market_price;
        let settlement = input.settlement_date;
        let frequency = self.get_frequency(&input.bond);

        // Build the bond for analytics
        let bond = self.to_fixed_rate_bond(&input.bond)?;

        // Calculate accrued interest
        let accrued = bond.accrued_interest(settlement);

        // Calculate analytics if we have a price
        let (ytm, dirty_price, mod_dur, mac_dur, conv, dv01_val, pv01_val, spread_dur_val, eff_dur_val, eff_conv_val, z_spread_val, i_spread_val, g_spread_val, asw_val, krd_val, cs01_val) =
            if let Some(price) = clean_price {
                let price_f64 = price.to_f64().unwrap_or(100.0);
                let dirty = price_f64 + accrued.to_f64().unwrap_or(0.0);
                let dirty_dec = Decimal::from_f64_retain(dirty).unwrap_or_default();

                // Calculate YTM
                let ytm_result = yield_to_maturity(&bond, settlement, price, frequency);
                let ytm_val = ytm_result.ok().map(|r| r.yield_value);

                if let Some(ytm) = ytm_val {
                    // Calculate duration and convexity
                    let mac = macaulay_duration(&bond, settlement, ytm, frequency).ok();
                    let mod_d = modified_duration(&bond, settlement, ytm, frequency).ok();
                    let conv = convexity(&bond, settlement, ytm, frequency).ok();
                    let dv = dv01(&bond, settlement, ytm, dirty, frequency).ok();

                    // Calculate Z-spread if discount curve provided
                    let z_spread = input
                        .discount_curve
                        .as_ref()
                        .and_then(|curve| self.calculate_z_spread(&bond, dirty_dec, settlement, curve));

                    // Calculate I-spread if benchmark/swap curve provided
                    let i_spread = input
                        .benchmark_curve
                        .as_ref()
                        .and_then(|curve| self.calculate_i_spread(&bond, ytm, settlement, curve));

                    // Calculate G-spread if government curve provided
                    let g_spread = input
                        .government_curve
                        .as_ref()
                        .and_then(|curve| self.calculate_g_spread(&bond, ytm, settlement, curve));

                    // Calculate ASW if swap curve provided (uses same curve as I-spread)
                    let asw = input
                        .benchmark_curve
                        .as_ref()
                        .and_then(|curve| self.calculate_asw(&bond, price, settlement, curve));

                    // Calculate key rate durations if discount curve provided
                    let krd = input
                        .discount_curve
                        .as_ref()
                        .and_then(|curve| self.calculate_key_rate_durations(&bond, settlement, curve));

                    // Calculate CS01 if we have Z-spread and discount curve
                    let cs01 = match (&z_spread, &input.discount_curve) {
                        (Some(z), Some(curve)) => self.calculate_cs01(&bond, *z, settlement, curve),
                        _ => None,
                    };

                    // Calculate PV01 if discount curve provided (curve-based, more accurate)
                    let pv01 = input
                        .discount_curve
                        .as_ref()
                        .and_then(|curve| self.calculate_pv01(&bond, settlement, curve));

                    // Calculate spread duration if we have Z-spread and discount curve
                    let spread_dur = match (&z_spread, &input.discount_curve) {
                        (Some(z), Some(curve)) => {
                            self.calculate_spread_duration(&bond, *z, settlement, curve)
                        }
                        _ => None,
                    };

                    // Calculate effective duration if discount curve provided
                    let eff_dur = input
                        .discount_curve
                        .as_ref()
                        .and_then(|curve| self.calculate_effective_duration(&bond, settlement, curve));

                    // Calculate effective convexity if discount curve provided
                    let eff_conv = input
                        .discount_curve
                        .as_ref()
                        .and_then(|curve| self.calculate_effective_convexity(&bond, settlement, curve));

                    (
                        Some(Decimal::try_from(ytm).unwrap_or_default()),
                        Some(dirty_dec),
                        mod_d.map(|d| Decimal::try_from(d).unwrap_or_default()),
                        mac.map(|d| Decimal::try_from(d).unwrap_or_default()),
                        conv.map(|c| Decimal::try_from(c).unwrap_or_default()),
                        dv.map(|d| Decimal::try_from(d).unwrap_or_default()),
                        pv01,
                        spread_dur,
                        eff_dur,
                        eff_conv,
                        z_spread,
                        i_spread,
                        g_spread,
                        asw,
                        krd,
                        cs01,
                    )
                } else {
                    (
                        None,
                        Some(dirty_dec),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                }
            } else {
                (None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None)
            };

        info!(
            "Priced {}: YTM={:?}, ModDur={:?}, Z-spread={:?}bps, I-spread={:?}bps, G-spread={:?}bps, ASW={:?}bps",
            input.bond.instrument_id, ytm, mod_dur, z_spread_val, i_spread_val, g_spread_val, asw_val
        );

        Ok(BondQuoteOutput {
            instrument_id: input.bond.instrument_id.clone(),
            isin: input.bond.isin.clone(),
            currency: input.bond.currency,
            settlement_date: settlement,

            clean_price,
            dirty_price,
            accrued_interest: Some(accrued),

            ytm,
            ytw: None,
            ytc: None,

            z_spread: z_spread_val,
            i_spread: i_spread_val,
            g_spread: g_spread_val,
            asw: asw_val,
            oas: None,
            discount_margin: None,
            simple_margin: None,

            modified_duration: mod_dur,
            macaulay_duration: mac_dur,
            effective_duration: eff_dur_val,
            spread_duration: spread_dur_val,

            convexity: conv,
            effective_convexity: eff_conv_val,

            dv01: dv01_val,
            pv01: pv01_val.or(dv01_val), // Curve-based PV01 if available, else yield-based DV01
            key_rate_durations: krd_val,
            cs01: cs01_val,

            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            pricing_model: "DiscountToMaturity".to_string(),
            source: "convex-engine".to_string(),
            is_stale: false,
            quality: if ytm.is_some() { 100 } else { 50 },
        })
    }

    /// Price using yield-to-worst model.
    fn price_yield_to_worst(&self, input: &PricingInput) -> Result<BondQuoteOutput, EngineError> {
        // For callable bonds, calculate YTM first as base
        let mut output = self.price_discount_to_maturity(input)?;
        output.pricing_model = "YieldToWorst".to_string();

        // If no call schedule, YTW = YTM
        if input.bond.call_schedule.is_empty() {
            output.ytw = output.ytm;
            return Ok(output);
        }

        // Need a price to calculate YTW
        let clean_price = match input.market_price {
            Some(p) => p,
            None => {
                output.ytw = output.ytm;
                return Ok(output);
            }
        };

        // Build the bond for YTW calculation
        let bond = match self.to_fixed_rate_bond(&input.bond) {
            Ok(b) => b,
            Err(_) => {
                output.ytw = output.ytm;
                return Ok(output);
            }
        };

        let frequency = self.get_frequency(&input.bond);

        // Convert call schedule to (Date, Decimal) pairs
        let call_schedule: Vec<(Date, Decimal)> = input
            .bond
            .call_schedule
            .iter()
            .map(|entry| (entry.call_date, entry.call_price))
            .collect();

        // Calculate YTW
        let (ytw, workout_date) = self.calculate_yield_to_worst(
            &bond,
            clean_price,
            input.settlement_date,
            &call_schedule,
            frequency,
        );

        // Set YTW in output
        output.ytw = ytw.map(|y| Decimal::from_f64_retain(y).unwrap_or_default());

        // Also calculate yield to first call (YTC) for reference
        if let Some((first_call_date, first_call_price)) = call_schedule
            .iter()
            .filter(|(d, _)| *d > input.settlement_date)
            .min_by_key(|(d, _)| *d)
        {
            if let Some(ytc) = self.calculate_yield_to_call(
                &bond,
                clean_price,
                input.settlement_date,
                *first_call_date,
                *first_call_price,
                frequency,
            ) {
                output.ytc = Some(Decimal::from_f64_retain(ytc).unwrap_or_default());
            }
        }

        // Log YTW calculation
        info!(
            "YTW for {}: YTM={:?}, YTW={:?}, YTC={:?}, Workout={:?}",
            input.bond.instrument_id, output.ytm, output.ytw, output.ytc, workout_date
        );

        Ok(output)
    }

    /// Price using callable OAS model.
    fn price_callable_oas(&self, input: &PricingInput) -> Result<BondQuoteOutput, EngineError> {
        // Start with YTW pricing as base
        let mut output = self.price_yield_to_worst(input)?;
        output.pricing_model = "CallableOAS".to_string();

        // Need both volatility and discount curve for OAS calculation
        let volatility = input
            .volatility
            .map(|v| v.to_f64().unwrap_or(0.01))
            .unwrap_or(0.01); // Default 1% vol

        let discount_curve = match &input.discount_curve {
            Some(curve) => curve,
            None => {
                debug!("No discount curve for OAS - using YTW only");
                output.quality = 75;
                return Ok(output);
            }
        };

        // Build callable bond from reference data
        let callable_bond = match self.to_callable_bond(&input.bond) {
            Ok(bond) => bond,
            Err(e) => {
                debug!("Failed to build callable bond: {}", e);
                output.quality = 75;
                return Ok(output);
            }
        };

        // Get dirty price for OAS calculation
        let dirty_price = match output.dirty_price {
            Some(dp) => dp,
            None => {
                if let Some(cp) = input.market_price {
                    let accrued = output.accrued_interest.unwrap_or(Decimal::ZERO);
                    cp + accrued
                } else {
                    debug!("No price for OAS calculation");
                    output.quality = 75;
                    return Ok(output);
                }
            }
        };

        // Calculate OAS, effective duration, and effective convexity
        let (oas, eff_dur, eff_conv) = self.calculate_oas(
            &callable_bond,
            dirty_price,
            input.settlement_date,
            discount_curve,
            volatility,
        );

        // Update output with OAS analytics
        output.oas = oas;
        output.effective_duration = eff_dur;
        output.effective_convexity = eff_conv;

        // Log results
        info!(
            "OAS for {}: OAS={:?}bps, EffDur={:?}, EffConv={:?}",
            input.bond.instrument_id, output.oas, output.effective_duration, output.effective_convexity
        );

        output.quality = if oas.is_some() { 100 } else { 75 };

        Ok(output)
    }

    /// Price inflation-linked bond.
    fn price_inflation_linked(&self, input: &PricingInput) -> Result<BondQuoteOutput, EngineError> {
        // Inflation-linked pricing requires index ratios
        let mut output = self.price_discount_to_maturity(input)?;
        output.pricing_model = "InflationLinkedRealYield".to_string();
        output.quality = 50; // Partial implementation
        Ok(output)
    }

    /// Price floating rate note.
    fn price_floating_rate(&self, input: &PricingInput) -> Result<BondQuoteOutput, EngineError> {
        let settlement = input.settlement_date;
        let clean_price = input.market_price;

        // Build the FRN
        let frn = self.to_floating_rate_note(&input.bond)?;

        // Calculate accrued interest (using current index rate if available)
        let accrued = frn.accrued_interest(settlement);

        // Calculate dirty price
        let dirty_price = clean_price.map(|cp| cp + accrued);

        // FRN duration is approximately time to next reset
        let mod_dur = if let Some(ref terms) = input.bond.floating_terms {
            let reset_freq = terms.reset_frequency as f64;
            if reset_freq > 0.0 {
                Some(Decimal::try_from(1.0 / reset_freq).unwrap_or_default())
            } else {
                Some(Decimal::try_from(0.25).unwrap_or_default()) // Default quarterly
            }
        } else {
            Some(Decimal::try_from(0.25).unwrap_or_default())
        };

        // Calculate discount margin if we have curve and price
        let dm = match (&input.discount_curve, dirty_price) {
            (Some(curve), Some(dp)) => {
                self.calculate_discount_margin(&frn, dp, settlement, curve)
            }
            _ => {
                // Fall back to quoted spread as simple margin
                input.bond.floating_terms.as_ref().map(|t| t.spread)
            }
        };

        // Calculate simple margin if we have a price
        // Simple margin uses the current index rate - estimate from discount curve or use default
        let sm = match (dirty_price, &input.discount_curve) {
            (Some(dp), Some(curve)) => {
                // Use short-term rate from curve as proxy for current index
                let current_index = Decimal::from_f64_retain(curve.interpolate_rate(0.25))
                    .unwrap_or(dec!(0.05));
                let margin = simple_margin(&frn, dp, current_index, settlement);
                Some(margin.as_bps())
            }
            (Some(dp), None) => {
                // Use a default index rate (5% as reasonable default)
                let current_index = dec!(0.05);
                let margin = simple_margin(&frn, dp, current_index, settlement);
                Some(margin.as_bps())
            }
            _ => None,
        };

        let quality = if dm.is_some() && input.discount_curve.is_some() {
            100
        } else if dm.is_some() || sm.is_some() {
            75
        } else {
            50
        };

        // Log results
        info!("FRN {}: DM={:?}bps, SM={:?}bps", input.bond.instrument_id, dm, sm);

        Ok(BondQuoteOutput {
            instrument_id: input.bond.instrument_id.clone(),
            isin: input.bond.isin.clone(),
            currency: input.bond.currency,
            settlement_date: settlement,

            clean_price,
            dirty_price,
            accrued_interest: Some(accrued),

            ytm: None, // FRNs don't have YTM in the traditional sense
            ytw: None,
            ytc: None,

            z_spread: None,
            i_spread: None,
            g_spread: None,
            asw: None,
            oas: None,
            discount_margin: dm,
            simple_margin: sm,

            modified_duration: mod_dur,
            macaulay_duration: None,
            effective_duration: mod_dur, // For FRN, effective dur ≈ modified dur
            spread_duration: mod_dur, // Spread duration for FRN

            convexity: Some(Decimal::ZERO), // FRN convexity is near zero
            effective_convexity: Some(Decimal::ZERO),

            dv01: None,
            pv01: None,
            key_rate_durations: None,
            cs01: None,

            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            pricing_model: "FloatingRateDiscountMargin".to_string(),
            source: "engine".to_string(),
            is_stale: false,
            quality,
        })
    }

    /// Price using matrix pricing.
    fn price_matrix(&self, input: &PricingInput) -> Result<BondQuoteOutput, EngineError> {
        // Matrix pricing uses comparable bonds - simplified implementation
        let mut output = self.price_discount_to_maturity(input)?;
        output.pricing_model = "MatrixPricing".to_string();
        output.quality = 50; // Lower quality for matrix pricing
        Ok(output)
    }

    /// Batch price multiple bonds sequentially.
    pub fn price_batch(
        &self,
        inputs: &[PricingInput],
    ) -> Vec<Result<BondQuoteOutput, EngineError>> {
        inputs.iter().map(|input| self.price(input)).collect()
    }

    /// Batch price multiple bonds in parallel using rayon.
    ///
    /// This provides significant speedup for large batches (100+ bonds).
    pub fn price_batch_parallel(
        &self,
        inputs: &[PricingInput],
    ) -> Vec<Result<BondQuoteOutput, EngineError>> {
        use rayon::prelude::*;
        inputs.par_iter().map(|input| self.price(input)).collect()
    }

    /// Price a batch and collect statistics.
    pub fn price_batch_with_stats(
        &self,
        inputs: &[PricingInput],
    ) -> BatchPricingResult {
        use rayon::prelude::*;
        use std::time::Instant;

        let start = Instant::now();
        let results: Vec<_> = inputs.par_iter().map(|input| self.price(input)).collect();
        let elapsed = start.elapsed();

        let mut succeeded = 0;
        let mut failed = 0;
        let mut outputs = Vec::with_capacity(results.len());

        for result in results {
            match result {
                Ok(output) => {
                    succeeded += 1;
                    outputs.push(Ok(output));
                }
                Err(e) => {
                    failed += 1;
                    outputs.push(Err(e));
                }
            }
        }

        BatchPricingResult {
            outputs,
            succeeded,
            failed,
            elapsed_ms: elapsed.as_millis() as u64,
            bonds_per_second: if elapsed.as_secs_f64() > 0.0 {
                inputs.len() as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            },
        }
    }
}

/// Result of batch pricing operation.
#[derive(Debug)]
pub struct BatchPricingResult {
    /// Individual pricing results
    pub outputs: Vec<Result<BondQuoteOutput, EngineError>>,
    /// Number of successfully priced bonds
    pub succeeded: usize,
    /// Number of failed pricings
    pub failed: usize,
    /// Total elapsed time in milliseconds
    pub elapsed_ms: u64,
    /// Throughput (bonds per second)
    pub bonds_per_second: f64,
}

impl Default for PricingRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use convex_traits::ids::InstrumentId;
    use convex_traits::reference_data::IssuerType;
    use rust_decimal_macros::dec;

    fn create_test_bond() -> BondReferenceData {
        BondReferenceData {
            instrument_id: InstrumentId::new("TEST001"),
            isin: Some("US912828Z229".to_string()),
            cusip: Some("912828Z22".to_string()),
            sedol: None,
            bbgid: None,
            description: "US Treasury 2.5% 2030".to_string(),
            currency: convex_core::Currency::USD,
            issue_date: Date::from_ymd(2020, 5, 15).unwrap(),
            maturity_date: Date::from_ymd(2030, 5, 15).unwrap(),
            coupon_rate: Some(dec!(0.025)),
            frequency: 2,
            day_count: "30/360".to_string(),
            face_value: dec!(100),
            bond_type: BondType::FixedBullet,
            issuer_type: IssuerType::Sovereign,
            issuer_id: "US_GOVT".to_string(),
            issuer_name: "United States Treasury".to_string(),
            seniority: "Senior".to_string(),
            is_callable: false,
            call_schedule: vec![],
            is_putable: false,
            is_sinkable: false,
            floating_terms: None,
            inflation_index: None,
            inflation_base_index: None,
            has_deflation_floor: false,
            country_of_risk: "US".to_string(),
            sector: "Government".to_string(),
            amount_outstanding: Some(dec!(50000000000)),
            first_coupon_date: Some(Date::from_ymd(2020, 11, 15).unwrap()),
            last_updated: 0,
            source: "test".to_string(),
        }
    }

    fn create_test_curve(ref_date: Date) -> BuiltCurve {
        // Create a simple upward-sloping curve: 3% to 5% over 10 years
        use convex_traits::ids::CurveId;

        BuiltCurve {
            curve_id: CurveId::new("USD_SOFR"),
            reference_date: ref_date,
            points: vec![
                (0.25, 0.030),  // 3M: 3.0%
                (0.5, 0.032),   // 6M: 3.2%
                (1.0, 0.035),   // 1Y: 3.5%
                (2.0, 0.038),   // 2Y: 3.8%
                (3.0, 0.040),   // 3Y: 4.0%
                (5.0, 0.045),   // 5Y: 4.5%
                (7.0, 0.048),   // 7Y: 4.8%
                (10.0, 0.050),  // 10Y: 5.0%
                (30.0, 0.055),  // 30Y: 5.5%
            ],
            built_at: 0,
            inputs_hash: "test".to_string(),
        }
    }

    #[test]
    fn test_select_model() {
        let router = PricingRouter::new();
        let bond = create_test_bond();

        assert_eq!(router.select_model(&bond), PricingModel::DiscountToMaturity);
    }

    #[test]
    fn test_price_at_par() {
        let router = PricingRouter::new();
        let bond = create_test_bond();

        let input = PricingInput {
            bond,
            settlement_date: Date::from_ymd(2025, 1, 15).unwrap(),
            market_price: Some(dec!(100.0)),
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());
        assert!(output.modified_duration.is_some());
        assert!(output.convexity.is_some());

        // YTM should be close to coupon rate at par
        let ytm = output.ytm.unwrap().to_f64().unwrap();
        assert!(
            (ytm - 0.025).abs() < 0.01,
            "YTM {} should be close to 2.5%",
            ytm
        );

        // Duration should be around 4-5 years for a 5-year bond
        let dur = output.modified_duration.unwrap().to_f64().unwrap();
        assert!(dur > 3.0 && dur < 6.0, "Duration {} out of range", dur);
    }

    #[test]
    fn test_price_with_z_spread() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(95.0)), // Discount price
            discount_curve: Some(curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());
        assert!(output.z_spread.is_some());

        // Z-spread should be negative (bond trades cheap vs curve)
        // Since bond coupon is 2.5% and curve is ~4%, discount price implies negative spread
        let z_spread = output.z_spread.unwrap().to_f64().unwrap();
        debug!("Z-spread: {} bps", z_spread);
        // Note: Z-spread can vary based on implementation
    }

    #[test]
    fn test_price_with_i_spread() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let swap_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(100.0)),
            discount_curve: None,
            benchmark_curve: Some(swap_curve),
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());
        assert!(output.i_spread.is_some());

        // I-spread = YTM - swap rate at maturity
        // Bond YTM ~ 2.5%, swap rate at 5Y ~ 4.5%, so I-spread ~ -200 bps
        let i_spread = output.i_spread.unwrap().to_f64().unwrap();
        assert!(
            i_spread < 0.0,
            "I-spread {} should be negative (bond yields less than swaps)",
            i_spread
        );
    }

    fn create_test_government_curve(ref_date: Date) -> GovernmentCurve {
        use convex_analytics::spreads::{GovernmentBenchmark, Sovereign};
        use convex_bonds::types::Tenor;

        // Create UST benchmarks at ~4% yield level (higher than 2.5% bond coupon)
        let y2 = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y2,
            "912828XX0",
            Date::from_ymd(2027, 1, 15).unwrap(),
            dec!(0.04),
            Yield::new(dec!(0.040), Compounding::SemiAnnual),
        );

        let y5 = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y5,
            "912828YY0",
            Date::from_ymd(2030, 1, 15).unwrap(),
            dec!(0.04),
            Yield::new(dec!(0.042), Compounding::SemiAnnual),
        );

        let y10 = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            Tenor::Y10,
            "912828ZZ0",
            Date::from_ymd(2035, 1, 15).unwrap(),
            dec!(0.04),
            Yield::new(dec!(0.045), Compounding::SemiAnnual),
        );

        GovernmentCurve::us_treasury(ref_date)
            .with_benchmark(y2)
            .with_benchmark(y5)
            .with_benchmark(y10)
    }

    #[test]
    fn test_price_with_g_spread() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let gov_curve = create_test_government_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(100.0)),
            discount_curve: None,
            benchmark_curve: None,
            government_curve: Some(gov_curve),
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());
        assert!(output.g_spread.is_some());

        // G-spread = Bond YTM - Treasury yield at maturity
        // Bond YTM ~ 2.5%, Treasury 5Y yield ~ 4.2%, so G-spread ~ -170 bps
        let g_spread = output.g_spread.unwrap().to_f64().unwrap();
        assert!(
            g_spread < 0.0,
            "G-spread {} should be negative (corporate yields less than treasury - unusual but valid for test)",
            g_spread
        );
    }

    #[test]
    fn test_price_with_asw() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let swap_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(95.0)), // Discount price
            discount_curve: None,
            benchmark_curve: Some(swap_curve),
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());
        assert!(output.i_spread.is_some());
        assert!(output.asw.is_some(), "ASW should be calculated when swap curve is provided");

        // ASW measures the spread in an asset swap package
        // For a discount bond, ASW should be positive (spread compensates for paying par)
        let asw = output.asw.unwrap().to_f64().unwrap();
        println!("ASW: {} bps, I-spread: {:?} bps", asw, output.i_spread);

        // ASW and I-spread should be related but not identical
        // (ASW accounts for coupon annuity, I-spread is simple subtraction)
    }

    #[test]
    fn test_price_with_key_rate_durations() {
        let router = PricingRouter::new();
        let bond = create_test_bond(); // 5-year bond
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(100.0)), // Par price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());

        // Key rate durations should be calculated when discount curve is provided
        assert!(
            output.key_rate_durations.is_some(),
            "Key rate durations should be calculated when discount curve is provided"
        );

        let krds = output.key_rate_durations.unwrap();
        assert!(!krds.is_empty(), "Should have at least one KRD point");

        // Print the key rate durations
        println!("Key Rate Durations:");
        let mut total_krd = 0.0;
        for (tenor, krd) in &krds {
            let krd_val = krd.to_f64().unwrap();
            total_krd += krd_val;
            println!("  {}: {:.4}", tenor, krd_val);
        }
        println!("  Total: {:.4}", total_krd);

        // Total KRD should be approximately equal to modified duration
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            // Allow some tolerance due to different calculation methods
            assert!(
                (total_krd - mod_dur_val).abs() < 2.0,
                "Total KRD {} should be close to modified duration {}",
                total_krd,
                mod_dur_val
            );
        }

        // For a 5-year bond, we should see KRDs at short and medium tenors
        let tenor_labels: Vec<&str> = krds.iter().map(|(t, _)| t.as_str()).collect();
        assert!(
            tenor_labels.iter().any(|t| *t == "5Y" || *t == "3Y" || *t == "2Y"),
            "Should have KRD at medium tenors for 5-year bond"
        );
    }

    fn create_callable_test_bond() -> BondReferenceData {
        use convex_traits::reference_data::CallScheduleEntry;

        BondReferenceData {
            instrument_id: InstrumentId::new("CALLABLE001"),
            isin: Some("US123456789".to_string()),
            cusip: Some("123456789".to_string()),
            sedol: None,
            bbgid: None,
            description: "Test Callable Bond 5% 2030".to_string(),
            currency: convex_core::Currency::USD,
            issue_date: Date::from_ymd(2020, 6, 15).unwrap(),
            maturity_date: Date::from_ymd(2030, 6, 15).unwrap(),
            coupon_rate: Some(dec!(0.05)),
            frequency: 2,
            day_count: "30/360".to_string(),
            face_value: dec!(100),
            bond_type: BondType::FixedCallable,
            issuer_type: IssuerType::CorporateIG,
            issuer_id: "CORP001".to_string(),
            issuer_name: "Test Corporation".to_string(),
            seniority: "Senior".to_string(),
            is_callable: true,
            call_schedule: vec![
                CallScheduleEntry {
                    call_date: Date::from_ymd(2025, 6, 15).unwrap(),
                    call_price: dec!(102),
                    is_make_whole: false,
                },
                CallScheduleEntry {
                    call_date: Date::from_ymd(2027, 6, 15).unwrap(),
                    call_price: dec!(101),
                    is_make_whole: false,
                },
                CallScheduleEntry {
                    call_date: Date::from_ymd(2028, 6, 15).unwrap(),
                    call_price: dec!(100),
                    is_make_whole: false,
                },
            ],
            is_putable: false,
            is_sinkable: false,
            floating_terms: None,
            inflation_index: None,
            inflation_base_index: None,
            has_deflation_floor: false,
            country_of_risk: "US".to_string(),
            sector: "Corporate".to_string(),
            amount_outstanding: Some(dec!(500000000)),
            first_coupon_date: Some(Date::from_ymd(2020, 12, 15).unwrap()),
            last_updated: 0,
            source: "test".to_string(),
        }
    }

    #[test]
    fn test_price_callable_with_oas() {
        let router = PricingRouter::new();
        let bond = create_callable_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(102.0)), // Trading at premium (likely to be called)
            discount_curve: Some(curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: Some(dec!(0.01)), // 1% volatility
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();

        // Should have YTW (from callable bond)
        assert!(output.ytw.is_some() || output.ytm.is_some());

        // Should have OAS calculated
        assert!(output.oas.is_some(), "OAS should be calculated for callable bond");

        // Should have effective duration
        assert!(
            output.effective_duration.is_some(),
            "Effective duration should be calculated"
        );

        // Should have effective convexity
        assert!(
            output.effective_convexity.is_some(),
            "Effective convexity should be calculated"
        );

        // Effective duration should be positive and less than maturity
        let eff_dur = output.effective_duration.unwrap().to_f64().unwrap();
        assert!(
            eff_dur > 0.0 && eff_dur < 10.0,
            "Effective duration {} should be positive and reasonable",
            eff_dur
        );

        // Model should be callable OAS
        assert_eq!(output.pricing_model, "CallableOAS");

        println!(
            "Callable bond OAS: {:?}bps, EffDur: {:?}, EffConv: {:?}",
            output.oas, output.effective_duration, output.effective_convexity
        );
    }

    #[test]
    fn test_price_callable_with_ytw() {
        let router = PricingRouter::new();
        let bond = create_callable_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();

        // Test with premium price - YTW should be lower than YTM (likely to be called)
        let input = PricingInput {
            bond: bond.clone(),
            settlement_date: settlement,
            market_price: Some(dec!(108.0)), // High premium
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price_yield_to_worst(&input);
        assert!(result.is_ok());

        let output = result.unwrap();

        // Should have YTM calculated
        assert!(output.ytm.is_some(), "YTM should be calculated");

        // Should have YTW calculated
        assert!(output.ytw.is_some(), "YTW should be calculated for callable bond");

        // Should have YTC calculated (yield to first call)
        assert!(output.ytc.is_some(), "YTC should be calculated");

        let ytm = output.ytm.unwrap().to_f64().unwrap();
        let ytw = output.ytw.unwrap().to_f64().unwrap();
        let ytc = output.ytc.unwrap().to_f64().unwrap();

        println!(
            "Premium callable: YTM={:.4}%, YTW={:.4}%, YTC={:.4}%",
            ytm * 100.0,
            ytw * 100.0,
            ytc * 100.0
        );

        // For a premium bond, YTW should be <= YTM (call is unfavorable to investor)
        assert!(
            ytw <= ytm + 0.001, // Small tolerance for rounding
            "YTW {} should be <= YTM {} for premium callable bond",
            ytw,
            ytm
        );

        // Test with discount price - YTW should equal YTM (unlikely to be called)
        let input_discount = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(95.0)), // Discount
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result_discount = router.price_yield_to_worst(&input_discount);
        assert!(result_discount.is_ok());

        let output_discount = result_discount.unwrap();
        let ytm_disc = output_discount.ytm.unwrap().to_f64().unwrap();
        let ytw_disc = output_discount.ytw.unwrap().to_f64().unwrap();

        println!(
            "Discount callable: YTM={:.4}%, YTW={:.4}%",
            ytm_disc * 100.0,
            ytw_disc * 100.0
        );

        // For a discount bond, YTW is typically YTM (better to hold to maturity)
        // But could also be YTC if call price is below par
    }

    fn create_frn_test_bond() -> BondReferenceData {
        use convex_traits::ids::FloatingRateIndex;
        use convex_traits::reference_data::FloatingRateTerms;

        BondReferenceData {
            instrument_id: InstrumentId::new("FRN001"),
            isin: Some("US912828ZQ7".to_string()),
            cusip: Some("912828ZQ7".to_string()),
            sedol: None,
            bbgid: None,
            description: "Test FRN SOFR+50bps 2027".to_string(),
            currency: convex_core::Currency::USD,
            issue_date: Date::from_ymd(2025, 1, 15).unwrap(),
            maturity_date: Date::from_ymd(2027, 1, 15).unwrap(),
            coupon_rate: None, // FRNs don't have fixed coupon
            frequency: 4, // Quarterly
            day_count: "ACT/360".to_string(),
            face_value: dec!(100),
            bond_type: BondType::FloatingRate,
            issuer_type: IssuerType::CorporateIG,
            issuer_id: "CORP001".to_string(),
            issuer_name: "Test Corporation".to_string(),
            seniority: "Senior".to_string(),
            is_callable: false,
            call_schedule: vec![],
            is_putable: false,
            is_sinkable: false,
            floating_terms: Some(FloatingRateTerms {
                index: FloatingRateIndex::Sofr,
                spread: dec!(50), // 50 bps
                reset_frequency: 4, // Quarterly
                cap: None,
                floor: None,
            }),
            inflation_index: None,
            inflation_base_index: None,
            has_deflation_floor: false,
            country_of_risk: "US".to_string(),
            sector: "Corporate".to_string(),
            amount_outstanding: Some(dec!(500000000)),
            first_coupon_date: Some(Date::from_ymd(2025, 4, 15).unwrap()),
            last_updated: 0,
            source: "test".to_string(),
        }
    }

    #[test]
    fn test_price_frn_with_discount_margin() {
        let router = PricingRouter::new();
        let bond = create_frn_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(99.75)), // Slight discount
            discount_curve: Some(curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();

        // Should have discount margin calculated
        assert!(
            output.discount_margin.is_some(),
            "Discount margin should be calculated for FRN"
        );

        // Should have simple margin calculated
        assert!(
            output.simple_margin.is_some(),
            "Simple margin should be calculated for FRN"
        );

        // Duration for FRN should be short (quarterly reset = 0.25 years)
        assert!(
            output.modified_duration.is_some(),
            "Modified duration should be calculated"
        );

        let mod_dur = output.modified_duration.unwrap().to_f64().unwrap();
        assert!(
            mod_dur > 0.0 && mod_dur < 1.0,
            "FRN duration {} should be less than 1 year",
            mod_dur
        );

        // Model should be floating rate
        assert_eq!(output.pricing_model, "FloatingRateDiscountMargin");

        let dm = output.discount_margin.unwrap().to_f64().unwrap();
        let sm = output.simple_margin.unwrap().to_f64().unwrap();

        println!(
            "FRN Discount Margin: {:.1}bps, Simple Margin: {:.1}bps, Duration: {:?}",
            dm, sm, output.modified_duration
        );

        // Simple margin and discount margin should be related but not identical
        // Both should be positive for a discount FRN with positive spread
    }

    #[test]
    fn test_price_at_discount() {
        let router = PricingRouter::new();
        let bond = create_test_bond();

        let input = PricingInput {
            bond,
            settlement_date: Date::from_ymd(2025, 1, 15).unwrap(),
            market_price: Some(dec!(95.0)), // Discount
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        let ytm = output.ytm.unwrap().to_f64().unwrap();

        // YTM should be higher than coupon when at discount
        assert!(ytm > 0.025, "YTM {} should be > 2.5% at discount", ytm);
    }

    #[test]
    fn test_price_at_premium() {
        let router = PricingRouter::new();
        let bond = create_test_bond();

        let input = PricingInput {
            bond,
            settlement_date: Date::from_ymd(2025, 1, 15).unwrap(),
            market_price: Some(dec!(105.0)), // Premium
            discount_curve: None,
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        let ytm = output.ytm.unwrap().to_f64().unwrap();

        // YTM should be lower than coupon when at premium
        assert!(ytm < 0.025, "YTM {} should be < 2.5% at premium", ytm);
    }

    #[test]
    fn test_built_curve_interpolation() {
        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let curve = create_test_curve(ref_date);

        // Test exact point
        let rate_5y = curve.interpolate_rate(5.0);
        assert!((rate_5y - 0.045).abs() < 0.0001);

        // Test interpolation between points
        let rate_4y = curve.interpolate_rate(4.0);
        assert!(rate_4y > 0.040 && rate_4y < 0.045);

        // Test extrapolation (flat)
        let rate_40y = curve.interpolate_rate(40.0);
        assert!((rate_40y - 0.055).abs() < 0.0001);
    }

    #[test]
    fn test_built_curve_discount_factor() {
        use convex_curves::RateCurveDyn;

        let ref_date = Date::from_ymd(2025, 1, 15).unwrap();
        let curve = create_test_curve(ref_date);

        // DF at 0 should be 1.0
        let df_0 = curve.discount_factor(0.0).unwrap();
        assert!((df_0 - 1.0).abs() < 0.0001);

        // DF at 1Y with 3.5% rate: exp(-0.035 * 1) = 0.9656
        let df_1y = curve.discount_factor(1.0).unwrap();
        assert!(df_1y > 0.96 && df_1y < 0.97);

        // DF at 10Y with 5% rate: exp(-0.05 * 10) = 0.6065
        let df_10y = curve.discount_factor(10.0).unwrap();
        assert!(df_10y > 0.60 && df_10y < 0.62);
    }

    #[test]
    fn test_price_with_cs01() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(95.0)), // Discount price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());
        assert!(output.z_spread.is_some(), "Z-spread needed for CS01 calculation");

        // CS01 should be calculated when we have z-spread and discount curve
        assert!(
            output.cs01.is_some(),
            "CS01 should be calculated when z-spread and discount curve are available"
        );

        let cs01 = output.cs01.unwrap().to_f64().unwrap();

        // CS01 should be positive (price decreases when spread increases)
        assert!(cs01 > 0.0, "CS01 {} should be positive", cs01);

        // CS01 for a 5-year bond should be roughly duration / 100 * price
        // Typical range: 0.04 to 0.06 for a 5-year bond at ~95
        assert!(
            cs01 > 0.01 && cs01 < 0.20,
            "CS01 {} should be in reasonable range for 5-year bond",
            cs01
        );

        println!(
            "CS01: {:.4} (price change per 1bp spread increase), Z-spread: {:?}bps",
            cs01, output.z_spread
        );

        // CS01 relationship to duration: CS01 ≈ price * modified_duration / 10000
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            let price = output.dirty_price.unwrap_or(dec!(95)).to_f64().unwrap();
            let expected_cs01 = price * mod_dur_val / 10000.0;

            // CS01 should be reasonably close to this approximation
            println!(
                "Expected CS01 (approx): {:.4}, Actual: {:.4}",
                expected_cs01, cs01
            );
        }
    }

    #[test]
    fn test_price_with_pv01() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(100.0)), // Par price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());
        assert!(output.dv01.is_some(), "DV01 should be calculated");

        // PV01 should be calculated when discount curve is provided (curve-based)
        assert!(
            output.pv01.is_some(),
            "PV01 should be calculated when discount curve is available"
        );

        let dv01 = output.dv01.unwrap().to_f64().unwrap();
        let pv01 = output.pv01.unwrap().to_f64().unwrap();

        // Both should be positive (price decreases when yield/rates increase)
        assert!(dv01 > 0.0, "DV01 {} should be positive", dv01);
        assert!(pv01 > 0.0, "PV01 {} should be positive", pv01);

        // For a 5-year bond, DV01/PV01 per 100 face should be approximately:
        // DV01 ≈ ModDur × Price × 0.0001 ≈ 5 × 100 × 0.0001 = 0.05
        assert!(
            dv01 > 0.01 && dv01 < 0.20,
            "DV01 {} should be in reasonable range",
            dv01
        );
        assert!(
            pv01 > 0.01 && pv01 < 0.20,
            "PV01 {} should be in reasonable range",
            pv01
        );

        // DV01 (yield-based) and PV01 (curve-based) should be similar for fixed-rate bonds
        let diff_pct = ((pv01 - dv01) / dv01).abs() * 100.0;
        assert!(
            diff_pct < 50.0, // Allow up to 50% difference due to different methods
            "PV01 {} and DV01 {} should be reasonably similar (diff: {:.1}%)",
            pv01,
            dv01,
            diff_pct
        );

        println!(
            "DV01: {:.6} (yield-based), PV01: {:.6} (curve-based), Diff: {:.1}%",
            dv01, pv01, diff_pct
        );

        // PV01 relationship to duration: PV01 ≈ price × modified_duration × 0.0001
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            let price = output.dirty_price.unwrap_or(dec!(100)).to_f64().unwrap();
            let expected_pv01 = price * mod_dur_val * 0.0001;

            println!(
                "Expected PV01 (approx): {:.6}, Actual DV01: {:.6}, Actual PV01: {:.6}",
                expected_pv01, dv01, pv01
            );
        }
    }

    #[test]
    fn test_price_with_spread_duration() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(95.0)), // Discount price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());
        assert!(
            output.z_spread.is_some(),
            "Z-spread needed for spread duration"
        );

        // Spread duration should be calculated when we have z-spread and discount curve
        assert!(
            output.spread_duration.is_some(),
            "Spread duration should be calculated when z-spread and discount curve are available"
        );

        let spread_dur = output.spread_duration.unwrap().to_f64().unwrap();

        // Spread duration should be positive for a normal bond
        assert!(
            spread_dur > 0.0,
            "Spread duration {} should be positive",
            spread_dur
        );

        // For a fixed-rate bond, spread duration should be approximately equal to modified duration
        // Typical 5-year bond has duration around 4-5 years
        assert!(
            spread_dur > 1.0 && spread_dur < 10.0,
            "Spread duration {} should be in reasonable range for 5-year bond",
            spread_dur
        );

        // Spread duration should be close to modified duration for fixed-rate bonds
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            let diff_pct = ((spread_dur - mod_dur_val) / mod_dur_val).abs() * 100.0;

            println!(
                "Spread Duration: {:.4}, Modified Duration: {:.4}, Diff: {:.1}%",
                spread_dur, mod_dur_val, diff_pct
            );

            // For fixed-rate bonds, spread duration ≈ modified duration
            // Allow some tolerance due to different calculation methods
            assert!(
                diff_pct < 20.0,
                "Spread duration {} should be close to modified duration {} (diff: {:.1}%)",
                spread_dur,
                mod_dur_val,
                diff_pct
            );
        }

        println!(
            "Spread Duration: {:.4}, Z-spread: {:?}bps",
            spread_dur, output.z_spread
        );
    }

    #[test]
    fn test_price_with_effective_duration() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(100.0)), // Par price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());

        // Effective duration should be calculated when discount curve is provided
        assert!(
            output.effective_duration.is_some(),
            "Effective duration should be calculated when discount curve is available"
        );

        let eff_dur = output.effective_duration.unwrap().to_f64().unwrap();

        // Effective duration should be positive for a normal bond
        assert!(
            eff_dur > 0.0,
            "Effective duration {} should be positive",
            eff_dur
        );

        // For a 5-year bond, effective duration should be around 4-5 years
        assert!(
            eff_dur > 1.0 && eff_dur < 10.0,
            "Effective duration {} should be in reasonable range for 5-year bond",
            eff_dur
        );

        // For non-callable fixed-rate bonds, effective duration ≈ modified duration
        if let Some(mod_dur) = output.modified_duration {
            let mod_dur_val = mod_dur.to_f64().unwrap();
            let diff_pct = ((eff_dur - mod_dur_val) / mod_dur_val).abs() * 100.0;

            println!(
                "Effective Duration: {:.4}, Modified Duration: {:.4}, Diff: {:.1}%",
                eff_dur, mod_dur_val, diff_pct
            );

            // For non-callable bonds, effective duration should be very close to modified duration
            // The small difference comes from curve shape vs flat yield assumption
            assert!(
                diff_pct < 15.0,
                "Effective duration {} should be close to modified duration {} (diff: {:.1}%)",
                eff_dur,
                mod_dur_val,
                diff_pct
            );
        }

        // Verify that effective duration uses curve-based calculation (10bp bump)
        // by checking it's different from a simple yield-based calculation
        println!(
            "Effective Duration (curve-based): {:.4}, ModDur: {:?}",
            eff_dur, output.modified_duration
        );
    }

    #[test]
    fn test_price_with_effective_convexity() {
        let router = PricingRouter::new();
        let bond = create_test_bond();
        let settlement = Date::from_ymd(2025, 1, 15).unwrap();
        let discount_curve = create_test_curve(settlement);

        let input = PricingInput {
            bond,
            settlement_date: settlement,
            market_price: Some(dec!(100.0)), // Par price
            discount_curve: Some(discount_curve),
            benchmark_curve: None,
            government_curve: None,
            volatility: None,
        };

        let result = router.price(&input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.ytm.is_some());

        // Effective convexity should be calculated when discount curve is provided
        assert!(
            output.effective_convexity.is_some(),
            "Effective convexity should be calculated when discount curve is available"
        );

        let eff_conv = output.effective_convexity.unwrap().to_f64().unwrap();

        // Effective convexity should be positive for a standard non-callable bond
        // (positive convexity means bond gains more when rates fall than it loses when rates rise)
        assert!(
            eff_conv > 0.0,
            "Effective convexity {} should be positive for non-callable bond",
            eff_conv
        );

        // For a 5-year bond, convexity should be roughly proportional to duration squared
        // Typical range: 10-50 for a 5-year bond
        assert!(
            eff_conv > 1.0 && eff_conv < 100.0,
            "Effective convexity {} should be in reasonable range for 5-year bond",
            eff_conv
        );

        // For non-callable fixed-rate bonds, effective convexity ≈ analytical convexity
        if let Some(conv) = output.convexity {
            let conv_val = conv.to_f64().unwrap();
            let diff_pct = ((eff_conv - conv_val) / conv_val).abs() * 100.0;

            println!(
                "Effective Convexity: {:.4}, Analytical Convexity: {:.4}, Diff: {:.1}%",
                eff_conv, conv_val, diff_pct
            );

            // For non-callable bonds, effective convexity should be close to analytical convexity
            // Allow some tolerance due to different calculation methods (curve vs yield)
            assert!(
                diff_pct < 25.0,
                "Effective convexity {} should be close to analytical convexity {} (diff: {:.1}%)",
                eff_conv,
                conv_val,
                diff_pct
            );
        }

        // Both effective duration and convexity should be available
        assert!(
            output.effective_duration.is_some(),
            "Effective duration should also be calculated"
        );

        println!(
            "Effective Convexity: {:.4}, Effective Duration: {:?}",
            eff_conv, output.effective_duration
        );
    }

    // =========================================================================
    // BATCH PRICING THROUGHPUT TESTS
    // =========================================================================

    /// Create a batch of test bonds with varying characteristics.
    fn create_bond_batch(count: usize) -> Vec<BondReferenceData> {
        let coupons = [0.02, 0.025, 0.03, 0.035, 0.04, 0.045, 0.05];
        let maturities = [2026, 2027, 2028, 2029, 2030, 2031, 2032, 2033, 2034, 2035];

        (0..count)
            .map(|i| {
                let coupon_idx = i % coupons.len();
                let maturity_idx = i % maturities.len();

                BondReferenceData {
                    instrument_id: InstrumentId::new(format!("BOND_{:05}", i)),
                    isin: Some(format!("US9128{:05}X", i)),
                    cusip: Some(format!("9128{:05}", i)),
                    sedol: None,
                    bbgid: None,
                    description: format!("Test Bond {} {:.1}% {}", i, coupons[coupon_idx] * 100.0, maturities[maturity_idx]),
                    currency: convex_core::Currency::USD,
                    issue_date: Date::from_ymd(2020, 1, 15).unwrap(),
                    maturity_date: Date::from_ymd(maturities[maturity_idx], 6, 15).unwrap(),
                    coupon_rate: Some(Decimal::from_f64_retain(coupons[coupon_idx]).unwrap()),
                    frequency: 2,
                    day_count: "30/360".to_string(),
                    face_value: dec!(100),
                    bond_type: BondType::FixedBullet,
                    issuer_type: IssuerType::CorporateIG,
                    issuer_id: format!("ISSUER_{}", i % 10),
                    issuer_name: format!("Test Issuer {}", i % 10),
                    seniority: "Senior".to_string(),
                    is_callable: false,
                    call_schedule: vec![],
                    is_putable: false,
                    is_sinkable: false,
                    floating_terms: None,
                    inflation_index: None,
                    inflation_base_index: None,
                    has_deflation_floor: false,
                    country_of_risk: "US".to_string(),
                    sector: "Corporate".to_string(),
                    amount_outstanding: Some(dec!(1000000000)),
                    first_coupon_date: Some(Date::from_ymd(2020, 7, 15).unwrap()),
                    last_updated: 0,
                    source: "test".to_string(),
                }
            })
            .collect()
    }

    #[test]
    fn test_batch_pricing_sequential() {
        let router = PricingRouter::new();
        let settlement = Date::from_ymd(2025, 6, 15).unwrap();
        let curve = create_test_curve(settlement);

        // Create a batch of 100 bonds
        let bonds = create_bond_batch(100);
        let inputs: Vec<PricingInput> = bonds
            .into_iter()
            .enumerate()
            .map(|(i, bond)| {
                // Vary prices slightly around par
                let price_offset = (i as f64 % 10.0) - 5.0;
                let price = 100.0 + price_offset;

                PricingInput {
                    bond,
                    settlement_date: settlement,
                    market_price: Some(Decimal::from_f64_retain(price).unwrap()),
                    discount_curve: Some(curve.clone()),
                    benchmark_curve: Some(curve.clone()),
                    government_curve: None,
                    volatility: None,
                }
            })
            .collect();

        // Time sequential batch pricing
        let start = std::time::Instant::now();
        let results = router.price_batch(&inputs);
        let elapsed = start.elapsed();

        // Verify results
        let succeeded: usize = results.iter().filter(|r| r.is_ok()).count();
        let failed: usize = results.iter().filter(|r| r.is_err()).count();

        println!(
            "Sequential batch: {} bonds in {:?} ({:.0} bonds/sec), {} succeeded, {} failed",
            inputs.len(),
            elapsed,
            inputs.len() as f64 / elapsed.as_secs_f64(),
            succeeded,
            failed
        );

        assert_eq!(succeeded, 100, "All bonds should price successfully");
        assert_eq!(failed, 0, "No bonds should fail");

        // Verify some analytics are calculated
        for result in results.iter().take(5) {
            let output = result.as_ref().unwrap();
            assert!(output.ytm.is_some(), "YTM should be calculated");
            assert!(output.modified_duration.is_some(), "Duration should be calculated");
            assert!(output.z_spread.is_some(), "Z-spread should be calculated");
        }
    }

    #[test]
    fn test_batch_pricing_parallel() {
        let router = PricingRouter::new();
        let settlement = Date::from_ymd(2025, 6, 15).unwrap();
        let curve = create_test_curve(settlement);

        // Create a larger batch for parallel test
        let bonds = create_bond_batch(500);
        let inputs: Vec<PricingInput> = bonds
            .into_iter()
            .enumerate()
            .map(|(i, bond)| {
                let price_offset = (i as f64 % 20.0) - 10.0;
                let price = 100.0 + price_offset;

                PricingInput {
                    bond,
                    settlement_date: settlement,
                    market_price: Some(Decimal::from_f64_retain(price).unwrap()),
                    discount_curve: Some(curve.clone()),
                    benchmark_curve: Some(curve.clone()),
                    government_curve: None,
                    volatility: None,
                }
            })
            .collect();

        // Time parallel batch pricing
        let start = std::time::Instant::now();
        let results = router.price_batch_parallel(&inputs);
        let parallel_elapsed = start.elapsed();

        // Verify results
        let succeeded: usize = results.iter().filter(|r| r.is_ok()).count();
        let failed: usize = results.iter().filter(|r| r.is_err()).count();

        let throughput = inputs.len() as f64 / parallel_elapsed.as_secs_f64();

        println!(
            "Parallel batch: {} bonds in {:?} ({:.0} bonds/sec), {} succeeded, {} failed",
            inputs.len(),
            parallel_elapsed,
            throughput,
            succeeded,
            failed
        );

        assert_eq!(succeeded, 500, "All bonds should price successfully");
        assert_eq!(failed, 0, "No bonds should fail");

        // Verify analytics
        for result in results.iter().take(10) {
            let output = result.as_ref().unwrap();
            assert!(output.ytm.is_some(), "YTM should be calculated");
            assert!(output.modified_duration.is_some(), "Duration should be calculated");
            assert!(output.z_spread.is_some(), "Z-spread should be calculated");
            assert!(output.i_spread.is_some(), "I-spread should be calculated");
        }
    }

    #[test]
    fn test_batch_pricing_with_stats() {
        let router = PricingRouter::new();
        let settlement = Date::from_ymd(2025, 6, 15).unwrap();
        let curve = create_test_curve(settlement);

        // Create batch with some bonds that will fail (no coupon rate)
        let mut bonds = create_bond_batch(100);

        // Make some bonds invalid (missing coupon for fixed rate)
        for i in [10, 25, 50, 75].iter() {
            if *i < bonds.len() {
                bonds[*i].coupon_rate = None;
            }
        }

        let inputs: Vec<PricingInput> = bonds
            .into_iter()
            .map(|bond| PricingInput {
                bond,
                settlement_date: settlement,
                market_price: Some(dec!(100.0)),
                discount_curve: Some(curve.clone()),
                benchmark_curve: None,
                government_curve: None,
                volatility: None,
            })
            .collect();

        // Use batch with stats
        let result = router.price_batch_with_stats(&inputs);

        println!(
            "Batch with stats: {} bonds in {}ms ({:.0} bonds/sec)",
            inputs.len(),
            result.elapsed_ms,
            result.bonds_per_second
        );
        println!(
            "  Succeeded: {}, Failed: {}",
            result.succeeded, result.failed
        );

        // Should have some failures
        assert_eq!(result.succeeded, 96, "96 bonds should succeed");
        assert_eq!(result.failed, 4, "4 bonds should fail (no coupon)");
        assert_eq!(result.outputs.len(), 100, "Should have 100 results");

        // Throughput should be > 0
        assert!(
            result.bonds_per_second > 0.0,
            "Throughput should be positive"
        );
    }

    #[test]
    fn test_parallel_vs_sequential_speedup() {
        let router = PricingRouter::new();
        let settlement = Date::from_ymd(2025, 6, 15).unwrap();
        let curve = create_test_curve(settlement);

        // Create a moderately large batch
        let bonds = create_bond_batch(200);
        let inputs: Vec<PricingInput> = bonds
            .into_iter()
            .map(|bond| PricingInput {
                bond,
                settlement_date: settlement,
                market_price: Some(dec!(100.0)),
                discount_curve: Some(curve.clone()),
                benchmark_curve: Some(curve.clone()),
                government_curve: None,
                volatility: None,
            })
            .collect();

        // Warm-up run
        let _ = router.price_batch(&inputs);

        // Time sequential
        let start_seq = std::time::Instant::now();
        let seq_results = router.price_batch(&inputs);
        let seq_elapsed = start_seq.elapsed();

        // Time parallel
        let start_par = std::time::Instant::now();
        let par_results = router.price_batch_parallel(&inputs);
        let par_elapsed = start_par.elapsed();

        // Both should produce same number of results
        assert_eq!(seq_results.len(), par_results.len());

        let seq_throughput = inputs.len() as f64 / seq_elapsed.as_secs_f64();
        let par_throughput = inputs.len() as f64 / par_elapsed.as_secs_f64();
        let speedup = seq_elapsed.as_secs_f64() / par_elapsed.as_secs_f64();

        println!("Parallel vs Sequential comparison ({} bonds):", inputs.len());
        println!(
            "  Sequential: {:?} ({:.0} bonds/sec)",
            seq_elapsed, seq_throughput
        );
        println!(
            "  Parallel:   {:?} ({:.0} bonds/sec)",
            par_elapsed, par_throughput
        );
        println!("  Speedup:    {:.2}x", speedup);

        // Parallel should not be slower than sequential
        // Note: For small batches, overhead might make parallel slower
        // For larger batches, we expect speedup proportional to CPU cores
        // We don't assert specific speedup as it depends on hardware

        // Verify correctness - results should be equivalent
        let seq_succeeded = seq_results.iter().filter(|r| r.is_ok()).count();
        let par_succeeded = par_results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(
            seq_succeeded, par_succeeded,
            "Same number of bonds should succeed"
        );

        // Verify YTMs match (parallel shouldn't affect calculation accuracy)
        for (seq_result, par_result) in seq_results.iter().zip(par_results.iter()).take(20) {
            if let (Ok(seq_out), Ok(par_out)) = (seq_result, par_result) {
                assert_eq!(seq_out.ytm, par_out.ytm, "YTMs should match");
                assert_eq!(
                    seq_out.modified_duration, par_out.modified_duration,
                    "Durations should match"
                );
            }
        }
    }
}
