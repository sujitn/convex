//! Bond Risk Metrics Implementation Skeleton
//! 
//! Core traits and structures for Bloomberg-parity fixed income analytics.
//! Targets sub-microsecond performance for single-bond calculations.

use std::collections::HashMap;

// ============================================================================
// Core Types
// ============================================================================

/// Day count convention for accrual calculations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayCountConvention {
    /// 30/360 US - US corporate bonds, agencies
    Dc30360Us,
    /// ACT/ACT ICMA - US Treasuries, UK Gilts
    DcActActIcma,
    /// ACT/360 - Money markets, FRNs
    DcAct360,
    /// ACT/365 Fixed - Sterling bonds
    DcAct365Fixed,
    /// 30E/360 - European bonds
    Dc30E360,
}

/// Compounding frequency
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompoundingFrequency {
    Annual,
    SemiAnnual,
    Quarterly,
    Monthly,
    Continuous,
}

/// Bond type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BondType {
    FixedRate,
    Callable,
    Putable,
    FloatingRate,
    InflationLinked,
    ZeroCoupon,
    SinkingFund,
    Amortizing,
    Perpetual,
    Convertible,
}

// ============================================================================
// Risk Metrics Output Structures
// ============================================================================

/// Core duration and convexity metrics
#[derive(Debug, Clone, Default)]
pub struct DurationMetrics {
    /// Macaulay duration (years)
    pub macaulay_duration: f64,
    /// Modified duration (years)
    pub modified_duration: f64,
    /// Effective duration for optioned bonds (years)
    pub effective_duration: Option<f64>,
    /// DV01 - dollar value of 1bp ($ per $100 notional)
    pub dv01: f64,
    /// Dollar duration
    pub dollar_duration: f64,
    /// Standard convexity
    pub convexity: f64,
    /// Effective convexity for optioned bonds
    pub effective_convexity: Option<f64>,
}

/// Key rate duration decomposition
#[derive(Debug, Clone)]
pub struct KeyRateDurations {
    /// Tenor points in years (e.g., [1.0, 2.0, 5.0, 10.0, 30.0])
    pub tenors: Vec<f64>,
    /// KRD at each tenor
    pub durations: Vec<f64>,
    /// Sum of KRDs (should ≈ effective duration)
    pub total: f64,
    /// Bump methodology used
    pub bump_type: BumpType,
}

/// Bump methodology for KRD
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpType {
    /// Triangular (tent) bump - industry standard
    Triangular,
    /// Box/rectangular bump
    Box,
    /// Left-adjusted triangular
    LeftAdjusted,
    /// Right-adjusted triangular
    RightAdjusted,
}

/// Spread-based risk metrics
#[derive(Debug, Clone, Default)]
pub struct SpreadMetrics {
    /// Option-adjusted spread (bps)
    pub oas: Option<f64>,
    /// Z-spread (bps)
    pub z_spread: f64,
    /// G-spread (bps)
    pub g_spread: f64,
    /// I-spread (bps)
    pub i_spread: f64,
    /// Asset swap spread (bps)
    pub asw_spread: f64,
    /// Spread duration (years)
    pub spread_duration: f64,
    /// CS01 - credit spread 01 ($ per $100 notional per bp)
    pub cs01: f64,
}

/// FRN-specific metrics
#[derive(Debug, Clone, Default)]
pub struct FrnMetrics {
    /// Discount margin (bps)
    pub discount_margin: f64,
    /// DM01 ($ per bp DM change)
    pub dm01: f64,
    /// Interest rate duration (very short for FRNs)
    pub rate_duration: f64,
    /// Spread duration (≈ maturity)
    pub spread_duration: f64,
    /// Time to next reset (years)
    pub time_to_reset: f64,
}

/// Inflation-linked bond metrics
#[derive(Debug, Clone, Default)]
pub struct InflationMetrics {
    /// Real yield (%)
    pub real_yield: f64,
    /// Breakeven inflation rate (%)
    pub breakeven_inflation: f64,
    /// Real duration (years)
    pub real_duration: f64,
    /// BEI01 - breakeven inflation 01 ($ per bp)
    pub bei01: f64,
    /// Inflation DV01 ($ per bp inflation expectation)
    pub inflation_dv01: f64,
    /// Current index ratio
    pub index_ratio: f64,
}

/// Callable/putable bond specific metrics
#[derive(Debug, Clone, Default)]
pub struct OptionMetrics {
    /// Option-adjusted duration
    pub oad: f64,
    /// Option-adjusted convexity
    pub oac: f64,
    /// Vega - sensitivity to rate volatility
    pub vega: f64,
    /// Duration to call (if callable)
    pub duration_to_call: Option<f64>,
    /// Duration to put (if putable)
    pub duration_to_put: Option<f64>,
    /// One-sided duration (rates up)
    pub duration_up: f64,
    /// One-sided duration (rates down)
    pub duration_down: f64,
    /// Is bond exhibiting negative convexity?
    pub is_negative_convexity: bool,
}

/// Complete risk analytics output
#[derive(Debug, Clone)]
pub struct BondAnalytics {
    pub duration: DurationMetrics,
    pub key_rate: Option<KeyRateDurations>,
    pub spreads: SpreadMetrics,
    pub frn: Option<FrnMetrics>,
    pub inflation: Option<InflationMetrics>,
    pub options: Option<OptionMetrics>,
}

// ============================================================================
// Core Traits
// ============================================================================

/// Basic risk metrics applicable to all bonds
pub trait RiskMetrics {
    /// Calculate DV01 using central finite difference
    fn dv01(&self, curve: &dyn YieldCurve, shift_bp: f64) -> f64;
    
    /// Calculate modified duration
    fn modified_duration(&self, curve: &dyn YieldCurve) -> f64;
    
    /// Calculate Macaulay duration
    fn macaulay_duration(&self, curve: &dyn YieldCurve) -> f64;
    
    /// Calculate convexity
    fn convexity(&self, curve: &dyn YieldCurve, shift_bp: f64) -> f64;
    
    /// Calculate all duration metrics
    fn duration_metrics(&self, curve: &dyn YieldCurve) -> DurationMetrics;
}

/// Spread risk metrics for credit instruments
pub trait SpreadRisk {
    /// Calculate Z-spread
    fn z_spread(&self, curve: &dyn YieldCurve, dirty_price: f64) -> f64;
    
    /// Calculate G-spread (vs government curve)
    fn g_spread(&self, govt_curve: &dyn YieldCurve, ytm: f64) -> f64;
    
    /// Calculate I-spread (vs swap curve)
    fn i_spread(&self, swap_curve: &dyn YieldCurve, ytm: f64) -> f64;
    
    /// Calculate asset swap spread
    fn asw_spread(&self, swap_curve: &dyn YieldCurve, dirty_price: f64) -> f64;
    
    /// Calculate CS01
    fn cs01(&self, curve: &dyn YieldCurve) -> f64;
    
    /// Calculate spread duration
    fn spread_duration(&self, curve: &dyn YieldCurve) -> f64;
}

/// Key rate duration decomposition
pub trait KeyRateRisk {
    /// Calculate key rate durations at specified tenors
    fn key_rate_durations(
        &self,
        curve: &dyn YieldCurve,
        tenors: &[f64],
        bump_bp: f64,
        bump_type: BumpType,
    ) -> KeyRateDurations;
    
    /// Calculate partial DV01s
    fn partial_dv01s(
        &self,
        curve: &dyn YieldCurve,
        tenors: &[f64],
    ) -> Vec<f64>;
}

/// OAS-based metrics for bonds with embedded options
pub trait OptionAdjustedRisk {
    /// Calculate option-adjusted spread
    fn oas(
        &self,
        curve: &dyn YieldCurve,
        vol_surface: &dyn VolatilitySurface,
        dirty_price: f64,
    ) -> f64;
    
    /// Calculate effective duration (OAS constant)
    fn effective_duration(
        &self,
        curve: &dyn YieldCurve,
        vol_surface: &dyn VolatilitySurface,
        oas: f64,
        shift_bp: f64,
    ) -> f64;
    
    /// Calculate effective convexity (OAS constant)
    fn effective_convexity(
        &self,
        curve: &dyn YieldCurve,
        vol_surface: &dyn VolatilitySurface,
        oas: f64,
        shift_bp: f64,
    ) -> f64;
    
    /// Calculate vega (volatility sensitivity)
    fn vega(
        &self,
        curve: &dyn YieldCurve,
        vol_surface: &dyn VolatilitySurface,
        oas: f64,
    ) -> f64;
    
    /// Get complete option-adjusted metrics
    fn option_metrics(
        &self,
        curve: &dyn YieldCurve,
        vol_surface: &dyn VolatilitySurface,
        dirty_price: f64,
    ) -> OptionMetrics;
}

/// FRN-specific analytics
pub trait FrnRisk {
    /// Calculate discount margin
    fn discount_margin(&self, index_rate: f64, dirty_price: f64) -> f64;
    
    /// Calculate DM01
    fn dm01(&self, index_rate: f64) -> f64;
    
    /// Calculate interest rate duration (time to reset)
    fn rate_duration(&self) -> f64;
    
    /// Get complete FRN metrics
    fn frn_metrics(&self, index_rate: f64, dirty_price: f64) -> FrnMetrics;
}

/// Inflation-linked bond analytics
pub trait InflationRisk {
    /// Calculate real yield
    fn real_yield(&self, real_curve: &dyn YieldCurve, dirty_price: f64, index_ratio: f64) -> f64;
    
    /// Calculate real duration
    fn real_duration(&self, real_curve: &dyn YieldCurve) -> f64;
    
    /// Calculate BEI01
    fn bei01(&self, nominal_curve: &dyn YieldCurve, real_curve: &dyn YieldCurve) -> f64;
    
    /// Get complete inflation metrics
    fn inflation_metrics(
        &self,
        nominal_curve: &dyn YieldCurve,
        real_curve: &dyn YieldCurve,
        dirty_price: f64,
        index_ratio: f64,
    ) -> InflationMetrics;
}

// ============================================================================
// Supporting Traits (Curve Interfaces)
// ============================================================================

/// Yield curve interface
pub trait YieldCurve {
    /// Get zero rate at time t (continuous compounding)
    fn zero_rate(&self, t: f64) -> f64;
    
    /// Get discount factor at time t
    fn discount_factor(&self, t: f64) -> f64;
    
    /// Get forward rate between t1 and t2
    fn forward_rate(&self, t1: f64, t2: f64) -> f64;
    
    /// Get par rate for maturity t
    fn par_rate(&self, t: f64, frequency: CompoundingFrequency) -> f64;
    
    /// Apply parallel shift (returns new curve)
    fn parallel_shift(&self, shift: f64) -> Box<dyn YieldCurve>;
    
    /// Apply triangular bump at tenor (returns new curve)
    fn triangular_bump(&self, tenor: f64, shift: f64, adjacent_tenors: (f64, f64)) -> Box<dyn YieldCurve>;
}

/// Volatility surface for option pricing
pub trait VolatilitySurface {
    /// Get volatility at expiry and tenor
    fn volatility(&self, expiry: f64, tenor: f64) -> f64;
    
    /// Apply parallel shift to vol surface
    fn shift(&self, delta_vol: f64) -> Box<dyn VolatilitySurface>;
}

// ============================================================================
// Calculation Constants
// ============================================================================

/// Default calculation parameters (Bloomberg conventions)
pub mod defaults {
    /// Standard DV01 shift (1bp)
    pub const DV01_SHIFT_BP: f64 = 1.0;
    
    /// Effective duration shift for optioned bonds (25bp)
    pub const EFFECTIVE_DUR_SHIFT_BP: f64 = 25.0;
    
    /// Key rate duration shift (1bp)
    pub const KRD_SHIFT_BP: f64 = 1.0;
    
    /// Vega bump (1% = 100bp)
    pub const VEGA_SHIFT_VOL: f64 = 0.01;
    
    /// OAS solver tolerance (bps)
    pub const OAS_TOLERANCE_BP: f64 = 0.01;
    
    /// Maximum OAS solver iterations
    pub const OAS_MAX_ITERATIONS: usize = 100;
    
    /// Standard key rate tenors
    pub const STANDARD_KRD_TENORS: &[f64] = &[
        0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 15.0, 20.0, 25.0, 30.0
    ];
    
    /// Portfolio reporting tenors
    pub const PORTFOLIO_KRD_TENORS: &[f64] = &[2.0, 5.0, 10.0, 30.0];
    
    /// ISDA SIMM IR delta tenors
    pub const SIMM_IR_TENORS: &[f64] = &[
        0.038, 0.083, 0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 10.0, 15.0, 20.0, 30.0
    ];
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Central finite difference for first derivative
#[inline]
pub fn central_diff_first(f_down: f64, f_up: f64, h: f64) -> f64 {
    (f_down - f_up) / (2.0 * h)
}

/// Central finite difference for second derivative
#[inline]
pub fn central_diff_second(f_down: f64, f_0: f64, f_up: f64, h: f64) -> f64 {
    (f_up + f_down - 2.0 * f_0) / (h * h)
}

/// Convert basis points to decimal
#[inline]
pub fn bp_to_decimal(bp: f64) -> f64 {
    bp / 10_000.0
}

/// Convert decimal to basis points
#[inline]
pub fn decimal_to_bp(decimal: f64) -> f64 {
    decimal * 10_000.0
}

/// Convert modified duration to DV01
#[inline]
pub fn duration_to_dv01(modified_duration: f64, price: f64) -> f64 {
    modified_duration * price * 0.0001
}

/// Convert DV01 to modified duration
#[inline]
pub fn dv01_to_duration(dv01: f64, price: f64) -> f64 {
    dv01 / (price * 0.0001)
}

// ============================================================================
// Validation Utilities
// ============================================================================

/// Validate KRD sum against effective duration
pub fn validate_krd_sum(krd: &KeyRateDurations, effective_duration: f64, tolerance: f64) -> bool {
    (krd.total - effective_duration).abs() / effective_duration < tolerance
}

/// Validate convexity is positive for vanilla bond
pub fn validate_positive_convexity(convexity: f64, bond_type: BondType) -> bool {
    match bond_type {
        BondType::FixedRate | BondType::ZeroCoupon => convexity > 0.0,
        BondType::Callable => true, // May be negative
        _ => true,
    }
}

/// Validate OAS/Z-spread relationship for callable
pub fn validate_callable_spreads(oas: f64, z_spread: f64) -> bool {
    z_spread >= oas // Z-spread should be >= OAS for callable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bp_conversions() {
        assert!((bp_to_decimal(100.0) - 0.01).abs() < 1e-10);
        assert!((decimal_to_bp(0.01) - 100.0).abs() < 1e-10);
    }

    #[test]
    fn test_duration_dv01_conversion() {
        let duration = 5.0;
        let price = 100.0;
        let dv01 = duration_to_dv01(duration, price);
        assert!((dv01 - 0.05).abs() < 1e-10);
        
        let recovered_duration = dv01_to_duration(dv01, price);
        assert!((recovered_duration - duration).abs() < 1e-10);
    }

    #[test]
    fn test_central_diff() {
        // Test with f(x) = x^2, derivative = 2x at x=3
        let h = 0.01;
        let f_down = (3.0 - h).powi(2);
        let f_up = (3.0 + h).powi(2);
        let derivative = central_diff_first(f_down, f_up, h);
        assert!((derivative - 6.0).abs() < 0.001);
    }
}
