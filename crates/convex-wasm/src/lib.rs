//! WebAssembly bindings for Convex fixed income analytics.
//!
//! This crate provides WASM bindings for the Convex library, enabling
//! Bloomberg YAS-equivalent bond analytics in web browsers.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use convex_bonds::conventions::{ConventionKey, ConventionRegistry, InstrumentType, Market};
use convex_bonds::instruments::CallableBond;
use convex_bonds::prelude::BondIdentifiers;
use convex_bonds::pricing::{StandardYieldEngine, YieldEngine};
use convex_bonds::traits::{Bond, EmbeddedOptionBond, FixedCouponBond};
use convex_bonds::types::{
    CallEntry, CallSchedule, CallType, CompoundingMethod, ExDivAccruedMethod, ExDividendRules,
    SettlementRules, YieldCalculationRules, YieldConvention,
};
use convex_bonds::{FixedRateBond, FixedRateBondBuilder};
use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};
use convex_curves::curves::{DiscountCurve, DiscountCurveBuilder};
use convex_curves::interpolation::InterpolationMethod;
use convex_curves::{ZeroCurve, ZeroCurveBuilder};
use convex_spreads::oas::OASCalculator;
use convex_yas::YASCalculator;

// ============================================================================
// Initialization
// ============================================================================

/// Initialize the WASM module (sets up panic hook for better error messages).
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

// ============================================================================
// Input/Output Types
// ============================================================================

/// Call schedule entry for callable bonds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallScheduleEntry {
    /// Call date as "YYYY-MM-DD"
    pub date: String,
    /// Call price as percentage of par (e.g., 102.0 for 102%)
    pub price: f64,
}

/// Bond parameters for creating a fixed coupon bond.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondParams {
    /// Coupon rate as percentage (e.g., 5.0 for 5%)
    pub coupon_rate: f64,
    /// Maturity date as "YYYY-MM-DD"
    pub maturity_date: String,
    /// Issue date as "YYYY-MM-DD"
    pub issue_date: String,
    /// Settlement date as "YYYY-MM-DD"
    pub settlement_date: String,
    /// Face value (default 100)
    pub face_value: Option<f64>,
    /// Coupon frequency: 1=annual, 2=semi-annual, 4=quarterly, 12=monthly
    pub frequency: Option<u32>,
    /// Day count convention:
    /// - "30/360" or "30/360 US" - US (NASD) method
    /// - "30E/360" or "30/360 EU" - European (ISMA) method
    /// - "ACT/360" - Actual/360
    /// - "ACT/365" - Actual/365 Fixed
    /// - "ACT/ACT" - Actual/Actual ICMA
    pub day_count: Option<String>,
    /// Currency: "USD", "EUR", "GBP", etc.
    pub currency: Option<String>,
    /// First coupon date as "YYYY-MM-DD" (optional)
    pub first_coupon_date: Option<String>,
    /// Call schedule for callable bonds (optional)
    pub call_schedule: Option<Vec<CallScheduleEntry>>,
    /// Interest rate volatility for OAS calculation (as percentage, e.g., 1.0 for 1%)
    /// Default is 1.0% if not provided
    pub volatility: Option<f64>,

    // === New convention parameters ===
    /// Market: "US", "UK", "Germany", "France", "Italy", "Japan", etc.
    pub market: Option<String>,
    /// Instrument type: "GovernmentBond", "Corporate", "Municipal", "Agency", etc.
    pub instrument_type: Option<String>,
    /// Yield convention: "Street", "True", "ISMA", "Japanese", "USMunicipal", etc.
    pub yield_convention: Option<String>,
    /// Compounding method: "SemiAnnual", "Annual", "Quarterly", "Continuous", "Simple"
    pub compounding: Option<String>,
    /// Settlement days (T+N)
    pub settlement_days: Option<u32>,
    /// Ex-dividend days before coupon
    pub ex_dividend_days: Option<u32>,
    /// Whether this market uses business days for settlement
    pub use_business_days: Option<bool>,
}

/// Analysis results returned from bond calculations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    // Price metrics
    pub clean_price: Option<f64>,
    pub dirty_price: Option<f64>,
    pub accrued_interest: Option<f64>,

    // Yield metrics
    pub ytm: Option<f64>,
    pub current_yield: Option<f64>,
    pub simple_yield: Option<f64>,
    pub money_market_yield: Option<f64>,

    // Callable bond yields
    pub ytc: Option<f64>,             // Yield to first call
    pub ytw: Option<f64>,             // Yield to worst
    pub workout_date: Option<String>, // Date for YTW (call date or maturity)
    pub workout_price: Option<f64>,   // Call price or par at workout

    // Risk metrics
    pub modified_duration: Option<f64>,
    pub macaulay_duration: Option<f64>,
    pub convexity: Option<f64>,
    pub dv01: Option<f64>,

    // Spread metrics (in basis points)
    pub g_spread: Option<f64>,
    pub benchmark_spread: Option<f64>,
    pub benchmark_tenor: Option<String>,
    pub z_spread: Option<f64>,
    pub asw_spread: Option<f64>,
    pub oas: Option<f64>, // Option-Adjusted Spread (for callable bonds)

    // OAS-related metrics (for callable bonds)
    pub effective_duration: Option<f64>,
    pub effective_convexity: Option<f64>,
    pub option_value: Option<f64>,

    // Additional info
    pub days_to_maturity: Option<i64>,
    pub years_to_maturity: Option<f64>,
    pub is_callable: Option<bool>,

    // Convention info (returned for display)
    pub market: Option<String>,
    pub instrument_type: Option<String>,
    pub yield_convention: Option<String>,
    pub compounding_method: Option<String>,
    pub settlement_days: Option<u32>,
    pub ex_dividend_days: Option<u32>,
    pub is_ex_dividend: Option<bool>,

    // Error message if calculation failed
    pub error: Option<String>,
}

/// Cash flow entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashFlowEntry {
    pub date: String,
    pub amount: f64,
    pub cf_type: String, // "coupon", "principal", "coupon_and_principal"
}

/// Curve point for yield curve construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePoint {
    /// Date as "YYYY-MM-DD"
    pub date: String,
    /// Rate as percentage (e.g., 4.5 for 4.5%)
    pub rate: f64,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn parse_date(s: &str) -> Result<Date, String> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(format!("Invalid date format: {}. Expected YYYY-MM-DD", s));
    }

    let year: i32 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid year: {}", parts[0]))?;
    let month: u32 = parts[1]
        .parse()
        .map_err(|_| format!("Invalid month: {}", parts[1]))?;
    let day: u32 = parts[2]
        .parse()
        .map_err(|_| format!("Invalid day: {}", parts[2]))?;

    Date::from_ymd(year, month, day).map_err(|e| format!("Invalid date {}: {:?}", s, e))
}

fn date_to_naive(date: Date) -> chrono::NaiveDate {
    // Date can be converted to NaiveDate via Into trait
    date.into()
}

fn parse_day_count(s: &str) -> DayCountConvention {
    let normalized = s.to_uppercase().replace(' ', "");
    match normalized.as_str() {
        // 30/360 US (NASD) - default for US bonds
        "30/360" | "30/360US" | "30_360" | "THIRTY_360" | "30/360NASD" => {
            DayCountConvention::Thirty360US
        }
        // 30E/360 European (ISMA)
        "30E/360" | "30/360E" | "30/360EU" | "30/360EURO" | "30/360EUROPEAN" | "30E_360"
        | "THIRTY360E" | "30/360ISMA" => DayCountConvention::Thirty360E,
        // Actual/360
        "ACT/360" | "ACT_360" | "ACTUAL_360" | "ACTUAL/360" => DayCountConvention::Act360,
        // Actual/365 Fixed
        "ACT/365" | "ACT_365" | "ACTUAL_365" | "ACTUAL/365" | "ACT/365F" | "ACT/365FIXED" => {
            DayCountConvention::Act365Fixed
        }
        // Actual/Actual ICMA
        "ACT/ACT" | "ACT_ACT" | "ACTUAL_ACTUAL" | "ACTUAL/ACTUAL" | "ACT/ACTICMA" => {
            DayCountConvention::ActActIcma
        }
        // Default for US bonds
        _ => DayCountConvention::Thirty360US,
    }
}

fn parse_frequency(f: u32) -> Frequency {
    match f {
        1 => Frequency::Annual,
        2 => Frequency::SemiAnnual,
        4 => Frequency::Quarterly,
        12 => Frequency::Monthly,
        _ => Frequency::SemiAnnual, // Default
    }
}

fn parse_currency(s: &str) -> Currency {
    match s.to_uppercase().as_str() {
        "USD" => Currency::USD,
        "EUR" => Currency::EUR,
        "GBP" => Currency::GBP,
        "JPY" => Currency::JPY,
        "CHF" => Currency::CHF,
        "AUD" => Currency::AUD,
        "CAD" => Currency::CAD,
        "NZD" => Currency::NZD,
        _ => Currency::USD, // Default
    }
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

fn f64_to_decimal(f: f64) -> Decimal {
    Decimal::from_f64_retain(f).unwrap_or(Decimal::ZERO)
}

fn parse_market(s: &str) -> Market {
    match s.to_uppercase().as_str() {
        "US" | "USA" | "UNITED STATES" => Market::US,
        "UK" | "GB" | "UNITED KINGDOM" => Market::UK,
        "GERMANY" | "DE" | "GER" => Market::Germany,
        "FRANCE" | "FR" | "FRA" => Market::France,
        "ITALY" | "IT" | "ITA" => Market::Italy,
        "SPAIN" | "ES" | "ESP" => Market::Spain,
        "JAPAN" | "JP" | "JPN" => Market::Japan,
        "SWITZERLAND" | "CH" | "CHE" => Market::Switzerland,
        "AUSTRALIA" | "AU" | "AUS" => Market::Australia,
        "CANADA" | "CA" | "CAN" => Market::Canada,
        "NETHERLANDS" | "NL" | "NLD" => Market::Netherlands,
        "BELGIUM" | "BE" | "BEL" => Market::Belgium,
        "AUSTRIA" | "AT" | "AUT" => Market::Austria,
        "PORTUGAL" | "PT" | "PRT" => Market::Portugal,
        "IRELAND" | "IE" | "IRL" => Market::Ireland,
        "SWEDEN" | "SE" | "SWE" => Market::Sweden,
        "NORWAY" | "NO" | "NOR" => Market::Norway,
        "DENMARK" | "DK" | "DNK" => Market::Denmark,
        "FINLAND" | "FI" | "FIN" => Market::Finland,
        "EUROZONE" | "EUR" | "EU" => Market::Eurozone,
        _ => Market::US, // Default
    }
}

fn parse_instrument_type(s: &str) -> InstrumentType {
    match s.to_uppercase().replace(['_', '-', ' '], "").as_str() {
        "GOVERNMENTBOND" | "GOVT" | "SOVEREIGN" | "GOV" => InstrumentType::GovernmentBond,
        "TREASURYBILL" | "TBILL" | "BILL" => InstrumentType::TreasuryBill,
        "CORPORATEIG" | "IG" | "INVESTMENTGRADE" | "CORPORATE" => InstrumentType::CorporateIG,
        "CORPORATEHY" | "HY" | "HIGHYIELD" | "JUNK" => InstrumentType::CorporateHY,
        "MUNICIPAL" | "MUNI" => InstrumentType::Municipal,
        "AGENCY" | "GSE" => InstrumentType::Agency,
        "INFLATIONLINKED" | "TIPS" | "LINKER" | "ILB" => InstrumentType::InflationLinked,
        "GOVERNMENTFRN" | "GOVFRN" => InstrumentType::GovernmentFRN,
        "CORPORATEFRN" | "CORPFRN" | "FRN" | "FLOATER" => InstrumentType::CorporateFRN,
        "COVERED" | "COVEREDBOND" | "PFANDBRIEF" => InstrumentType::CoveredBond,
        "ABS" | "ASSETBACKED" => InstrumentType::ABS,
        "MBS" | "MORTGAGEBACKED" => InstrumentType::MBS,
        "SUPRANATIONAL" | "SUPRA" => InstrumentType::Supranational,
        "COMMERCIALPAPER" | "CP" => InstrumentType::CommercialPaper,
        "CONVERTIBLE" | "CONV" => InstrumentType::Convertible,
        "STRIPS" | "STRIP" => InstrumentType::Strips,
        _ => InstrumentType::GovernmentBond, // Default
    }
}

fn parse_yield_convention(s: &str) -> YieldConvention {
    match s.to_uppercase().replace(['_', '-', ' '], "").as_str() {
        "STREET" | "STREETCONVENTION" | "US" => YieldConvention::StreetConvention,
        "TRUE" | "TRUEYIELD" => YieldConvention::TrueYield,
        "ISMA" | "ICMA" => YieldConvention::ISMA,
        "SIMPLE" | "SIMPLEYIELD" | "JAPANESE" | "JGB" => YieldConvention::SimpleYield,
        "DISCOUNT" | "DISCOUNTYIELD" => YieldConvention::DiscountYield,
        "BONDEQUIVALENT" | "BEY" => YieldConvention::BondEquivalentYield,
        "MUNICIPAL" | "MUNI" | "TAXEQUIVALENT" => YieldConvention::MunicipalYield,
        "MOOSMULLER" | "GERMAN" => YieldConvention::Moosmuller,
        "BRAESSFANGMEYER" | "BRAESS" => YieldConvention::BraessFangmeyer,
        "ANNUAL" => YieldConvention::Annual,
        "CONTINUOUS" | "CONT" => YieldConvention::Continuous,
        _ => YieldConvention::StreetConvention, // Default
    }
}

fn parse_compounding(s: &str) -> CompoundingMethod {
    match s.to_uppercase().replace(['_', '-', ' '], "").as_str() {
        "SEMIANNUAL" | "SEMI" | "2" => CompoundingMethod::Periodic { frequency: 2 },
        "ANNUAL" | "1" => CompoundingMethod::Periodic { frequency: 1 },
        "QUARTERLY" | "4" => CompoundingMethod::Periodic { frequency: 4 },
        "MONTHLY" | "12" => CompoundingMethod::Periodic { frequency: 12 },
        "CONTINUOUS" | "CONT" => CompoundingMethod::Continuous,
        "SIMPLE" | "NONE" => CompoundingMethod::Simple,
        "DISCOUNT" => CompoundingMethod::Discount,
        _ => CompoundingMethod::Periodic { frequency: 2 }, // Default semi-annual
    }
}

/// Get yield calculation rules from parameters, using registry if market/type specified
fn get_yield_rules(params: &BondParams) -> YieldCalculationRules {
    // If market and instrument type specified, try to get from registry
    if let (Some(market_str), Some(inst_str)) = (&params.market, &params.instrument_type) {
        let market = parse_market(market_str);
        let instrument_type = parse_instrument_type(inst_str);

        let registry = ConventionRegistry::global();
        let key = ConventionKey::new(market, instrument_type);

        if let Some(rules) = registry.rules(&key) {
            return rules.clone();
        }

        // Fallback to market default if specific type not found
        return registry.default_rules_for_market(market);
    }

    // If only market specified, get default rules for that market
    if let Some(market_str) = &params.market {
        let market = parse_market(market_str);
        let registry = ConventionRegistry::global();
        return registry.default_rules_for_market(market);
    }

    // Build rules from individual parameters if specified
    let convention = params
        .yield_convention
        .as_ref()
        .map(|s| parse_yield_convention(s))
        .unwrap_or(YieldConvention::StreetConvention);

    let compounding = params
        .compounding
        .as_ref()
        .map(|s| parse_compounding(s))
        .unwrap_or(CompoundingMethod::Periodic { frequency: 2 });

    let day_count = parse_day_count(params.day_count.as_deref().unwrap_or("30/360"));

    let settlement_rules = SettlementRules {
        days: params.settlement_days.unwrap_or(1),
        use_business_days: params.use_business_days.unwrap_or(true),
        ..Default::default()
    };

    let ex_dividend_rules = params.ex_dividend_days.map(|days| ExDividendRules {
        days,
        day_type: convex_bonds::types::DayType::BusinessDays,
        accrued_method: ExDivAccruedMethod::NegativeAccrued,
    });

    YieldCalculationRules {
        convention,
        compounding,
        accrual_day_count: day_count,
        discount_day_count: day_count,
        settlement_rules,
        ex_dividend_rules,
        ..Default::default()
    }
}

/// Calculate yield using StandardYieldEngine with convention rules.
/// Returns yield as a decimal (e.g., 0.05 for 5%).
fn calculate_convention_yield(
    bond: &FixedRateBond,
    settlement: Date,
    clean_price: f64,
    rules: &YieldCalculationRules,
) -> Option<f64> {
    // Get cash flows from the bond (from settlement date onwards)
    let cash_flows = bond.cash_flows(settlement);

    if cash_flows.is_empty() {
        log("Convention yield: No cash flows");
        return None;
    }

    // Calculate accrued interest using the convention rules
    let accrued = bond.accrued_interest(settlement);

    // Use StandardYieldEngine for convention-aware yield calculation
    let engine = StandardYieldEngine::default();
    let clean_price_dec = f64_to_decimal(clean_price);

    log(&format!(
        "Convention yield calc: compounding={:?}, convention={:?}, cash_flows={}, price={}, accrued={}",
        rules.compounding, rules.convention, cash_flows.len(), clean_price, decimal_to_f64(accrued)
    ));

    match engine.yield_from_price(&cash_flows, clean_price_dec, accrued, settlement, rules) {
        Ok(result) => {
            log(&format!(
                "Convention yield result: {:.6} ({:.4}%)",
                result.yield_value,
                result.yield_value * 100.0
            ));
            Some(result.yield_value)
        }
        Err(e) => {
            log(&format!("Convention yield error: {:?}", e));
            // Fall back to YASCalculator result if convention engine fails
            None
        }
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// Format market name for display
fn format_market_name(market: Market) -> String {
    format!("{:?}", market)
}

/// Format instrument type for display
fn format_instrument_type(inst: InstrumentType) -> String {
    match inst {
        InstrumentType::GovernmentBond => "Government Bond".to_string(),
        InstrumentType::TreasuryBill => "Treasury Bill".to_string(),
        InstrumentType::GovernmentFRN => "Government FRN".to_string(),
        InstrumentType::InflationLinked => "Inflation Linked".to_string(),
        InstrumentType::Strips => "STRIPS".to_string(),
        InstrumentType::CorporateIG => "Corporate IG".to_string(),
        InstrumentType::CorporateHY => "Corporate HY".to_string(),
        InstrumentType::CorporateFRN => "Corporate FRN".to_string(),
        InstrumentType::Convertible => "Convertible".to_string(),
        InstrumentType::CommercialPaper => "Commercial Paper".to_string(),
        InstrumentType::Municipal => "Municipal".to_string(),
        InstrumentType::Agency => "Agency".to_string(),
        InstrumentType::Supranational => "Supranational".to_string(),
        InstrumentType::CoveredBond => "Covered Bond".to_string(),
        InstrumentType::ABS => "ABS".to_string(),
        InstrumentType::MBS => "MBS".to_string(),
    }
}

/// Format yield convention for display
fn format_yield_convention(conv: YieldConvention) -> String {
    match conv {
        YieldConvention::StreetConvention => "Street Convention".to_string(),
        YieldConvention::TrueYield => "True Yield".to_string(),
        YieldConvention::ISMA => "ISMA/ICMA".to_string(),
        YieldConvention::SimpleYield => "Simple Yield".to_string(),
        YieldConvention::DiscountYield => "Discount Yield".to_string(),
        YieldConvention::BondEquivalentYield => "Bond Equivalent".to_string(),
        YieldConvention::MunicipalYield => "Municipal (Tax-Equiv)".to_string(),
        YieldConvention::Moosmuller => "Moosmuller".to_string(),
        YieldConvention::BraessFangmeyer => "Braess-Fangmeyer".to_string(),
        YieldConvention::Annual => "Annual".to_string(),
        YieldConvention::Continuous => "Continuous".to_string(),
    }
}

/// Format compounding method for display
fn format_compounding(method: CompoundingMethod) -> String {
    match method {
        CompoundingMethod::Periodic { frequency: 1 } => "Annual".to_string(),
        CompoundingMethod::Periodic { frequency: 2 } => "Semi-Annual".to_string(),
        CompoundingMethod::Periodic { frequency: 4 } => "Quarterly".to_string(),
        CompoundingMethod::Periodic { frequency: 12 } => "Monthly".to_string(),
        CompoundingMethod::Periodic { frequency: n } => format!("{n}x/year"),
        CompoundingMethod::Continuous => "Continuous".to_string(),
        CompoundingMethod::Simple => "Simple".to_string(),
        CompoundingMethod::Discount => "Discount".to_string(),
        CompoundingMethod::ActualPeriod { .. } => "Actual Period".to_string(),
    }
}

// ============================================================================
// Core WASM Functions
// ============================================================================

/// Calculate bond analytics given price and yield curve.
///
/// Takes bond parameters, a clean price, and curve points, returns comprehensive analytics.
#[wasm_bindgen]
pub fn analyze_bond(params: JsValue, clean_price: f64, curve_points: JsValue) -> JsValue {
    let result = analyze_bond_impl(params, clean_price, curve_points);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn analyze_bond_impl(params: JsValue, clean_price: f64, curve_points: JsValue) -> AnalysisResult {
    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return AnalysisResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Parse curve points
    let points: Vec<CurvePoint> = match serde_wasm_bindgen::from_value(curve_points) {
        Ok(p) => p,
        Err(e) => {
            return AnalysisResult {
                error: Some(format!("Failed to parse curve points: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Build the curve
    let curve = match create_curve(settlement, &points) {
        Ok(c) => c,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Get yield calculation rules from parameters (using registry if market/type specified)
    let yield_rules = get_yield_rules(&bond_params);

    // Create calculator and analyze (for spreads, duration, etc.)
    let calculator = YASCalculator::new(&curve);
    let settlement_naive = date_to_naive(settlement);

    let yas_result = match calculator.analyze(&bond, settlement_naive, f64_to_decimal(clean_price))
    {
        Ok(result) => result,
        Err(e) => {
            return AnalysisResult {
                error: Some(format!("Analysis failed: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Convert base result with convention info
    let mut result = convert_yas_result(&yas_result, &bond, settlement, &yield_rules, &bond_params);

    // Calculate convention-aware yield using StandardYieldEngine
    // This properly applies compounding method, day count, and other convention rules
    if let Some(convention_ytm) =
        calculate_convention_yield(&bond, settlement, clean_price, &yield_rules)
    {
        result.ytm = Some(convention_ytm * 100.0); // Convert to percentage
    }

    // Handle callable bond yields if call schedule is provided
    if let Some(ref call_entries) = bond_params.call_schedule {
        if !call_entries.is_empty() {
            result.is_callable = Some(true);

            // Build call schedule
            let mut call_schedule = CallSchedule::new(CallType::American);
            for entry in call_entries {
                if let Ok(call_date) = parse_date(&entry.date) {
                    call_schedule =
                        call_schedule.with_entry(CallEntry::new(call_date, entry.price));
                }
            }

            // Create callable bond
            let callable = CallableBond::new(bond.clone(), call_schedule);
            let price_decimal = f64_to_decimal(clean_price);

            // Calculate yield to first call
            if let Ok(ytc) = callable.yield_to_first_call(price_decimal, settlement) {
                result.ytc = Some(decimal_to_f64(ytc) * 100.0); // Convert to percentage
            }

            // Calculate yield to worst with workout date
            if let Ok((ytw, workout_date)) =
                callable.yield_to_worst_with_date(price_decimal, settlement)
            {
                result.ytw = Some(decimal_to_f64(ytw) * 100.0); // Convert to percentage
                result.workout_date = Some(format!("{}", workout_date));

                // Get workout price (call price or par if maturity)
                if let Some(maturity) = bond.maturity() {
                    if workout_date == maturity {
                        result.workout_price = Some(100.0); // Par at maturity
                    } else if let Some(call_schedule) = callable.call_schedule() {
                        result.workout_price = call_schedule.call_price_on(workout_date);
                    }
                }
            }

            // Calculate OAS using Hull-White model
            // Volatility: default 1% if not provided
            let volatility = bond_params.volatility.unwrap_or(1.0) / 100.0;
            let oas_calc = OASCalculator::default_hull_white(volatility);
            let accrued = decimal_to_f64(bond.accrued_interest(settlement));
            let dirty_price_f64 = clean_price + accrued;
            let dirty_price = f64_to_decimal(dirty_price_f64);

            // Create discount curve for OAS calculation (implements Curve trait)
            match create_discount_curve(settlement, &points) {
                Ok(discount_curve) => {
                    match oas_calc.calculate(&callable, dirty_price, &discount_curve, settlement) {
                        Ok(oas) => {
                            result.oas = Some(decimal_to_f64(oas.as_bps()));

                            // Calculate effective duration and convexity
                            let oas_decimal = decimal_to_f64(oas.as_bps()) / 10000.0;
                            if let Ok(eff_dur) = oas_calc.effective_duration(
                                &callable,
                                &discount_curve,
                                oas_decimal,
                                settlement,
                            ) {
                                result.effective_duration = Some(eff_dur);
                            }
                            if let Ok(eff_conv) = oas_calc.effective_convexity(
                                &callable,
                                &discount_curve,
                                oas_decimal,
                                settlement,
                            ) {
                                result.effective_convexity = Some(eff_conv);
                            }
                            if let Ok(opt_val) = oas_calc.option_value(
                                &callable,
                                &discount_curve,
                                oas_decimal,
                                settlement,
                            ) {
                                result.option_value = Some(opt_val);
                            }
                        }
                        Err(_e) => {
                            // OAS calculation failed - this can happen if the model price
                            // cannot match the market price within the search bounds
                            // Return Z-spread as a fallback indicator
                            result.oas = result.z_spread;
                        }
                    }
                }
                Err(_e) => {
                    // Discount curve creation failed
                    result.oas = None;
                }
            }
        }
    } else {
        result.is_callable = Some(false);
    }

    result
}

/// Get bond cash flows.
///
/// Returns all future cash flows from settlement date.
#[wasm_bindgen]
pub fn get_cash_flows(params: JsValue) -> JsValue {
    let result = get_cash_flows_impl(params);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn get_cash_flows_impl(params: JsValue) -> Vec<CashFlowEntry> {
    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(_) => return vec![],
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    // Get cash flows
    bond.cash_flows(settlement)
        .iter()
        .map(|cf| {
            let cf_type = if cf.is_principal() && decimal_to_f64(cf.amount) > 50.0 {
                if decimal_to_f64(cf.amount) > 100.0 {
                    "coupon_and_principal"
                } else {
                    "principal"
                }
            } else {
                "coupon"
            };

            CashFlowEntry {
                date: format!("{}", cf.date),
                amount: decimal_to_f64(cf.amount),
                cf_type: cf_type.to_string(),
            }
        })
        .collect()
}

/// Calculate accrued interest.
#[wasm_bindgen]
pub fn calculate_accrued(params: JsValue) -> JsValue {
    let result = calculate_accrued_impl(params);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn calculate_accrued_impl(params: JsValue) -> Result<f64, String> {
    // Parse parameters
    let bond_params: BondParams = serde_wasm_bindgen::from_value(params)
        .map_err(|e| format!("Failed to parse bond parameters: {:?}", e))?;

    // Build the bond
    let bond = create_bond(&bond_params)?;

    // Parse settlement date
    let settlement = parse_date(&bond_params.settlement_date)?;

    // Calculate accrued
    let accrued = bond.accrued_interest(settlement);
    Ok(decimal_to_f64(accrued))
}

/// Simple yield calculation without curve (only basic metrics).
#[wasm_bindgen]
pub fn calculate_simple_metrics(params: JsValue, clean_price: f64) -> JsValue {
    let result = calculate_simple_metrics_impl(params, clean_price);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn calculate_simple_metrics_impl(params: JsValue, clean_price: f64) -> AnalysisResult {
    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return AnalysisResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return AnalysisResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Calculate basic metrics
    let accrued = decimal_to_f64(bond.accrued_interest(settlement));
    let dirty_price = clean_price + accrued;

    let (days_to_mat, years_to_mat) = match bond.maturity() {
        Some(maturity) => {
            let days = settlement.days_between(&maturity);
            (days, days as f64 / 365.0)
        }
        None => (0, 0.0),
    };

    // Current yield = annual coupon / clean price
    let annual_coupon =
        decimal_to_f64(bond.coupon_rate()) * decimal_to_f64(bond.face_value()) / 100.0;
    let current_yield = if clean_price > 0.0 {
        Some(annual_coupon / clean_price * 100.0)
    } else {
        None
    };

    AnalysisResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(accrued),
        current_yield,
        days_to_maturity: Some(days_to_mat),
        years_to_maturity: Some(years_to_mat),
        error: None,
        ..Default::default()
    }
}

// ============================================================================
// Internal Helpers
// ============================================================================

fn create_bond(params: &BondParams) -> Result<FixedRateBond, String> {
    let issue_date = parse_date(&params.issue_date)?;
    let maturity_date = parse_date(&params.maturity_date)?;

    // Convert coupon rate from percentage to decimal (e.g., 5.0% -> 0.05)
    let coupon = f64_to_decimal(params.coupon_rate / 100.0);
    let face = f64_to_decimal(params.face_value.unwrap_or(100.0));
    let frequency = parse_frequency(params.frequency.unwrap_or(2));
    let day_count = parse_day_count(params.day_count.as_deref().unwrap_or("30/360"));
    let currency = parse_currency(params.currency.as_deref().unwrap_or("USD"));

    let first_coupon = params
        .first_coupon_date
        .as_ref()
        .and_then(|s| parse_date(s).ok());

    // Create empty identifiers (WASM users don't need bond identifiers)
    let identifiers = BondIdentifiers::new();

    let mut builder = FixedRateBondBuilder::new()
        .identifiers(identifiers)
        .issue_date(issue_date)
        .maturity(maturity_date)
        .coupon_rate(coupon)
        .face_value(face)
        .frequency(frequency)
        .day_count(day_count)
        .currency(currency)
        .business_day_convention(BusinessDayConvention::ModifiedFollowing);

    if let Some(fc) = first_coupon {
        builder = builder.first_coupon_date(fc);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to create bond: {:?}", e))
}

fn create_curve(reference_date: Date, points: &[CurvePoint]) -> Result<ZeroCurve, String> {
    if points.is_empty() {
        return Err("Curve must have at least one point".to_string());
    }

    let mut builder = ZeroCurveBuilder::new()
        .reference_date(reference_date)
        .interpolation(InterpolationMethod::Linear);

    for point in points {
        let date = parse_date(&point.date)?;
        // Convert percentage to decimal (e.g., 4.5% -> 0.045)
        let rate = f64_to_decimal(point.rate / 100.0);
        builder = builder.add_rate(date, rate);
    }

    builder
        .build()
        .map_err(|e| format!("Failed to create curve: {:?}", e))
}

/// Create a DiscountCurve for OAS calculations (implements Curve trait)
fn create_discount_curve(
    reference_date: Date,
    points: &[CurvePoint],
) -> Result<DiscountCurve, String> {
    if points.is_empty() {
        return Err("Curve must have at least one point".to_string());
    }

    let mut builder = DiscountCurveBuilder::new(reference_date);

    // Always add t=0 pillar with df=1.0 (spot date)
    builder = builder.add_pillar(0.0, 1.0);

    // Collect and sort pillars by time
    let mut pillars: Vec<(f64, f64)> = Vec::new();

    for point in points {
        let date = parse_date(&point.date)?;
        // Convert from zero rate to discount factor
        // DF(t) = exp(-r * t)
        let rate = point.rate / 100.0; // Convert percentage to decimal
        let t = reference_date.days_between(&date) as f64 / 365.0;

        // Skip points at or before reference date
        if t <= 0.0 {
            continue;
        }

        let df = (-rate * t).exp();
        pillars.push((t, df));
    }

    // Sort by time
    pillars.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Add sorted pillars
    for (t, df) in pillars {
        builder = builder.add_pillar(t, df);
    }

    builder
        .with_extrapolation()
        .build()
        .map_err(|e| format!("Failed to create discount curve: {:?}", e))
}

fn convert_yas_result(
    result: &convex_yas::YASResult,
    bond: &FixedRateBond,
    settlement: Date,
    rules: &YieldCalculationRules,
    bond_params: &BondParams,
) -> AnalysisResult {
    let (days_to_mat, years_to_mat) = match bond.maturity() {
        Some(maturity) => {
            let days = settlement.days_between(&maturity);
            (days, days as f64 / 365.0)
        }
        None => (0, 0.0),
    };

    // Get invoice details for clean/dirty price
    let clean_price = decimal_to_f64(result.invoice.clean_price);
    let accrued = decimal_to_f64(result.invoice.accrued_interest);
    let dirty_price = decimal_to_f64(result.invoice.dirty_price);

    // Determine market and instrument type for display
    let market_display = bond_params
        .market
        .as_ref()
        .map(|s| format_market_name(parse_market(s)));
    let instrument_display = bond_params
        .instrument_type
        .as_ref()
        .map(|s| format_instrument_type(parse_instrument_type(s)));

    // Check if in ex-dividend period
    let is_ex_dividend = if let Some(ref ex_rules) = rules.ex_dividend_rules {
        // Check if settlement is within ex-div days of next coupon
        if let Some(next_coupon) = bond.next_coupon_date(settlement) {
            let days_to_coupon = settlement.days_between(&next_coupon);
            days_to_coupon <= ex_rules.days as i64
        } else {
            false
        }
    } else {
        false
    };

    AnalysisResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(accrued),

        ytm: Some(decimal_to_f64(result.ytm)),
        current_yield: Some(decimal_to_f64(result.current_yield)),
        simple_yield: Some(decimal_to_f64(result.simple_yield)),
        money_market_yield: result.money_market_yield.map(decimal_to_f64),

        // Callable bond fields - populated later if applicable
        ytc: None,
        ytw: None,
        workout_date: None,
        workout_price: None,

        modified_duration: Some(decimal_to_f64(result.modified_duration())),
        macaulay_duration: Some(decimal_to_f64(result.risk.macaulay_duration.years())),
        convexity: Some(decimal_to_f64(result.convexity())),
        dv01: Some(decimal_to_f64(result.dv01())),

        g_spread: Some(decimal_to_f64(result.g_spread.as_bps())),
        benchmark_spread: Some(decimal_to_f64(result.benchmark_spread.as_bps())),
        benchmark_tenor: Some(result.benchmark_tenor.clone()),
        z_spread: Some(decimal_to_f64(result.z_spread.as_bps())),
        asw_spread: result
            .asw_spread
            .as_ref()
            .map(|s| decimal_to_f64(s.as_bps())),
        oas: None, // Set by caller for callable bonds

        // OAS-related metrics - set by caller for callable bonds
        effective_duration: None,
        effective_convexity: None,
        option_value: None,

        days_to_maturity: Some(days_to_mat),
        years_to_maturity: Some(years_to_mat),
        is_callable: None, // Set by caller

        // Convention info
        market: market_display,
        instrument_type: instrument_display,
        yield_convention: Some(format_yield_convention(rules.convention)),
        compounding_method: Some(format_compounding(rules.compounding)),
        settlement_days: Some(rules.settlement_rules.days),
        ex_dividend_days: rules.ex_dividend_rules.as_ref().map(|r| r.days),
        is_ex_dividend: Some(is_ex_dividend),

        error: None,
    }
}

// ============================================================================
// Solve-for Functions (Price from Yield/Spread)
// ============================================================================

/// Calculate clean price from target yield.
///
/// Given a target YTM, calculates the clean price that would produce that yield.
#[wasm_bindgen]
pub fn price_from_yield(params: JsValue, target_ytm: f64, curve_points: JsValue) -> JsValue {
    let result = price_from_yield_impl(params, target_ytm, curve_points);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

/// Result from price-from-yield calculation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PriceFromYieldResult {
    pub clean_price: Option<f64>,
    pub dirty_price: Option<f64>,
    pub accrued_interest: Option<f64>,
    pub error: Option<String>,
}

fn price_from_yield_impl(
    params: JsValue,
    target_ytm: f64,
    _curve_points: JsValue,
) -> PriceFromYieldResult {
    use convex_bonds::pricing::YieldSolver;

    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Get cash flows and accrued interest
    let cash_flows = bond.cash_flows(settlement);
    let accrued = bond.accrued_interest(settlement);
    let day_count = parse_day_count(bond_params.day_count.as_deref().unwrap_or("30/360"));
    let frequency = bond.frequency();

    // Convert target yield from percentage to decimal
    let yield_decimal = target_ytm / 100.0;

    // Calculate clean price from yield
    let solver = YieldSolver::new();
    let clean_price = solver.clean_price_from_yield(
        &cash_flows,
        yield_decimal,
        accrued,
        settlement,
        day_count,
        frequency,
    );

    let dirty_price = clean_price + decimal_to_f64(accrued);

    PriceFromYieldResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(decimal_to_f64(accrued)),
        error: None,
    }
}

/// Calculate clean price from target Z-spread.
///
/// Given a target Z-spread (in basis points), calculates the clean price.
#[wasm_bindgen]
pub fn price_from_spread(
    params: JsValue,
    target_spread_bps: f64,
    curve_points: JsValue,
) -> JsValue {
    let result = price_from_spread_impl(params, target_spread_bps, curve_points);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn price_from_spread_impl(
    params: JsValue,
    target_spread_bps: f64,
    curve_points: JsValue,
) -> PriceFromYieldResult {
    use convex_spreads::ZSpreadCalculator;

    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Parse curve points
    let points: Vec<CurvePoint> = match serde_wasm_bindgen::from_value(curve_points) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse curve points: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Build the curve
    let curve = match create_curve(settlement, &points) {
        Ok(c) => c,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Get cash flows and accrued interest
    let cash_flows = bond.cash_flows(settlement);
    let accrued = bond.accrued_interest(settlement);

    // Convert spread from bps to decimal (e.g., 100 bps = 0.01)
    let spread_decimal = target_spread_bps / 10000.0;

    // Calculate price from Z-spread
    let calculator = ZSpreadCalculator::new(&curve);
    let dirty_price = calculator.price_with_spread(&cash_flows, spread_decimal, settlement);
    let clean_price = dirty_price - decimal_to_f64(accrued);

    PriceFromYieldResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(decimal_to_f64(accrued)),
        error: None,
    }
}

/// Calculate clean price from target G-spread.
///
/// Given a target G-spread (in basis points), calculates the clean price.
/// G-spread = YTM - interpolated benchmark rate at maturity.
#[wasm_bindgen]
pub fn price_from_g_spread(
    params: JsValue,
    target_g_spread_bps: f64,
    curve_points: JsValue,
) -> JsValue {
    let result = price_from_g_spread_impl(params, target_g_spread_bps, curve_points);
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn price_from_g_spread_impl(
    params: JsValue,
    target_g_spread_bps: f64,
    curve_points: JsValue,
) -> PriceFromYieldResult {
    use convex_bonds::pricing::YieldSolver;

    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Parse curve points
    let points: Vec<CurvePoint> = match serde_wasm_bindgen::from_value(curve_points) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse curve points: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Build the curve
    let curve = match create_curve(settlement, &points) {
        Ok(c) => c,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Get maturity date
    let maturity = match bond.maturity() {
        Some(m) => m,
        None => {
            return PriceFromYieldResult {
                error: Some("Bond has no maturity date".to_string()),
                ..Default::default()
            }
        }
    };

    // Get interpolated benchmark rate at maturity
    let benchmark_rate = match curve.zero_rate_at(maturity) {
        Ok(r) => decimal_to_f64(r),
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to get benchmark rate: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Calculate target YTM from G-spread: YTM = G-spread + benchmark_rate
    // G-spread is in bps, benchmark_rate is decimal (0.04 = 4%)
    let target_ytm = (target_g_spread_bps / 100.0) + (benchmark_rate * 100.0);

    // Get cash flows and accrued interest
    let cash_flows = bond.cash_flows(settlement);
    let accrued = bond.accrued_interest(settlement);
    let day_count = parse_day_count(bond_params.day_count.as_deref().unwrap_or("30/360"));
    let frequency = bond.frequency();

    // Convert target yield from percentage to decimal
    let yield_decimal = target_ytm / 100.0;

    // Calculate clean price from yield
    let solver = YieldSolver::new();
    let clean_price = solver.clean_price_from_yield(
        &cash_flows,
        yield_decimal,
        accrued,
        settlement,
        day_count,
        frequency,
    );

    let dirty_price = clean_price + decimal_to_f64(accrued);

    PriceFromYieldResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(decimal_to_f64(accrued)),
        error: None,
    }
}

/// Calculate clean price from target benchmark spread.
///
/// Given a target benchmark spread (in basis points), calculates the clean price.
/// Benchmark spread = YTM - nearest on-the-run tenor rate.
#[wasm_bindgen]
pub fn price_from_benchmark_spread(
    params: JsValue,
    target_benchmark_spread_bps: f64,
    benchmark_tenor: String,
    curve_points: JsValue,
) -> JsValue {
    let result = price_from_benchmark_spread_impl(
        params,
        target_benchmark_spread_bps,
        benchmark_tenor,
        curve_points,
    );
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

fn price_from_benchmark_spread_impl(
    params: JsValue,
    target_benchmark_spread_bps: f64,
    benchmark_tenor: String,
    curve_points: JsValue,
) -> PriceFromYieldResult {
    use convex_bonds::pricing::YieldSolver;

    // Parse parameters
    let bond_params: BondParams = match serde_wasm_bindgen::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse bond parameters: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Parse curve points
    let points: Vec<CurvePoint> = match serde_wasm_bindgen::from_value(curve_points) {
        Ok(p) => p,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to parse curve points: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Build the bond
    let bond = match create_bond(&bond_params) {
        Ok(b) => b,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Parse settlement date
    let settlement = match parse_date(&bond_params.settlement_date) {
        Ok(d) => d,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Build the curve
    let curve = match create_curve(settlement, &points) {
        Ok(c) => c,
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(e),
                ..Default::default()
            }
        }
    };

    // Parse tenor string to years (e.g., "5Y" -> 5.0, "10Y" -> 10.0, "6M" -> 0.5)
    let tenor_years = parse_tenor_to_years(&benchmark_tenor);

    // Calculate benchmark date from settlement
    let benchmark_days = (tenor_years * 365.25) as i64;
    let benchmark_date = settlement.add_days(benchmark_days);

    // Get benchmark tenor rate
    let benchmark_rate = match curve.zero_rate_at(benchmark_date) {
        Ok(r) => decimal_to_f64(r),
        Err(e) => {
            return PriceFromYieldResult {
                error: Some(format!("Failed to get benchmark rate: {:?}", e)),
                ..Default::default()
            }
        }
    };

    // Calculate target YTM from benchmark spread: YTM = benchmark_spread + benchmark_tenor_rate
    let target_ytm = (target_benchmark_spread_bps / 100.0) + (benchmark_rate * 100.0);

    // Get cash flows and accrued interest
    let cash_flows = bond.cash_flows(settlement);
    let accrued = bond.accrued_interest(settlement);
    let day_count = parse_day_count(bond_params.day_count.as_deref().unwrap_or("30/360"));
    let frequency = bond.frequency();

    // Convert target yield from percentage to decimal
    let yield_decimal = target_ytm / 100.0;

    // Calculate clean price from yield
    let solver = YieldSolver::new();
    let clean_price = solver.clean_price_from_yield(
        &cash_flows,
        yield_decimal,
        accrued,
        settlement,
        day_count,
        frequency,
    );

    let dirty_price = clean_price + decimal_to_f64(accrued);

    PriceFromYieldResult {
        clean_price: Some(clean_price),
        dirty_price: Some(dirty_price),
        accrued_interest: Some(decimal_to_f64(accrued)),
        error: None,
    }
}

/// Parse a tenor string like "5Y", "10Y", "6M", "3M" to years
fn parse_tenor_to_years(tenor: &str) -> f64 {
    let tenor = tenor.trim().to_uppercase();
    if tenor.ends_with('Y') {
        tenor[..tenor.len() - 1].parse::<f64>().unwrap_or(10.0)
    } else if tenor.ends_with('M') {
        tenor[..tenor.len() - 1]
            .parse::<f64>()
            .map(|m| m / 12.0)
            .unwrap_or(1.0)
    } else {
        // Try parsing as just a number (years)
        tenor.parse::<f64>().unwrap_or(10.0)
    }
}

// ============================================================================
// Convention Options for UI
// ============================================================================

/// Available convention options for UI dropdowns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionOptions {
    pub markets: Vec<ConventionOption>,
    pub instrument_types: Vec<ConventionOption>,
    pub yield_conventions: Vec<ConventionOption>,
    pub compounding_methods: Vec<ConventionOption>,
}

/// Single option for dropdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionOption {
    pub value: String,
    pub label: String,
}

/// Get available convention options for UI dropdowns.
#[wasm_bindgen]
pub fn get_convention_options() -> JsValue {
    let options = ConventionOptions {
        markets: vec![
            ConventionOption {
                value: "US".to_string(),
                label: "United States".to_string(),
            },
            ConventionOption {
                value: "UK".to_string(),
                label: "United Kingdom".to_string(),
            },
            ConventionOption {
                value: "Germany".to_string(),
                label: "Germany".to_string(),
            },
            ConventionOption {
                value: "France".to_string(),
                label: "France".to_string(),
            },
            ConventionOption {
                value: "Italy".to_string(),
                label: "Italy".to_string(),
            },
            ConventionOption {
                value: "Spain".to_string(),
                label: "Spain".to_string(),
            },
            ConventionOption {
                value: "Japan".to_string(),
                label: "Japan".to_string(),
            },
            ConventionOption {
                value: "Switzerland".to_string(),
                label: "Switzerland".to_string(),
            },
            ConventionOption {
                value: "Australia".to_string(),
                label: "Australia".to_string(),
            },
            ConventionOption {
                value: "Canada".to_string(),
                label: "Canada".to_string(),
            },
            ConventionOption {
                value: "Netherlands".to_string(),
                label: "Netherlands".to_string(),
            },
            ConventionOption {
                value: "Belgium".to_string(),
                label: "Belgium".to_string(),
            },
            ConventionOption {
                value: "Austria".to_string(),
                label: "Austria".to_string(),
            },
            ConventionOption {
                value: "Eurozone".to_string(),
                label: "Eurozone".to_string(),
            },
        ],
        instrument_types: vec![
            ConventionOption {
                value: "GovernmentBond".to_string(),
                label: "Government Bond".to_string(),
            },
            ConventionOption {
                value: "TreasuryBill".to_string(),
                label: "Treasury Bill".to_string(),
            },
            ConventionOption {
                value: "CorporateIG".to_string(),
                label: "Corporate IG".to_string(),
            },
            ConventionOption {
                value: "CorporateHY".to_string(),
                label: "Corporate HY".to_string(),
            },
            ConventionOption {
                value: "Municipal".to_string(),
                label: "Municipal".to_string(),
            },
            ConventionOption {
                value: "Agency".to_string(),
                label: "Agency".to_string(),
            },
            ConventionOption {
                value: "InflationLinked".to_string(),
                label: "Inflation Linked".to_string(),
            },
            ConventionOption {
                value: "CorporateFRN".to_string(),
                label: "Corporate FRN".to_string(),
            },
            ConventionOption {
                value: "Supranational".to_string(),
                label: "Supranational".to_string(),
            },
            ConventionOption {
                value: "CoveredBond".to_string(),
                label: "Covered Bond".to_string(),
            },
        ],
        yield_conventions: vec![
            ConventionOption {
                value: "Street".to_string(),
                label: "Street Convention".to_string(),
            },
            ConventionOption {
                value: "True".to_string(),
                label: "True Yield".to_string(),
            },
            ConventionOption {
                value: "ISMA".to_string(),
                label: "ISMA/ICMA".to_string(),
            },
            ConventionOption {
                value: "Simple".to_string(),
                label: "Simple Yield".to_string(),
            },
            ConventionOption {
                value: "Municipal".to_string(),
                label: "Municipal (Tax-Equiv)".to_string(),
            },
            ConventionOption {
                value: "Discount".to_string(),
                label: "Discount Yield".to_string(),
            },
            ConventionOption {
                value: "BondEquivalent".to_string(),
                label: "Bond Equivalent".to_string(),
            },
            ConventionOption {
                value: "Annual".to_string(),
                label: "Annual".to_string(),
            },
            ConventionOption {
                value: "Continuous".to_string(),
                label: "Continuous".to_string(),
            },
        ],
        compounding_methods: vec![
            ConventionOption {
                value: "SemiAnnual".to_string(),
                label: "Semi-Annual".to_string(),
            },
            ConventionOption {
                value: "Annual".to_string(),
                label: "Annual".to_string(),
            },
            ConventionOption {
                value: "Quarterly".to_string(),
                label: "Quarterly".to_string(),
            },
            ConventionOption {
                value: "Monthly".to_string(),
                label: "Monthly".to_string(),
            },
            ConventionOption {
                value: "Continuous".to_string(),
                label: "Continuous".to_string(),
            },
            ConventionOption {
                value: "Simple".to_string(),
                label: "Simple".to_string(),
            },
        ],
    };
    serde_wasm_bindgen::to_value(&options).unwrap_or(JsValue::NULL)
}

/// Get default conventions for a given market and instrument type.
#[wasm_bindgen]
pub fn get_default_conventions(market: String, instrument_type: String) -> JsValue {
    let market_enum = parse_market(&market);
    let inst_enum = parse_instrument_type(&instrument_type);

    let registry = ConventionRegistry::global();
    let key = ConventionKey::new(market_enum, inst_enum);

    // Try to get specific rules for this market/instrument combination
    let rules = if let Some(specific_rules) = registry.rules(&key) {
        specific_rules.clone()
    } else {
        // Fallback to market default
        registry.default_rules_for_market(market_enum)
    };

    let defaults = DefaultConventions {
        day_count: format!("{:?}", rules.accrual_day_count),
        yield_convention: format_yield_convention(rules.convention),
        compounding: format_compounding(rules.compounding),
        settlement_days: rules.settlement_rules.days,
        ex_dividend_days: rules.ex_dividend_rules.as_ref().map(|r| r.days),
        use_business_days: rules.settlement_rules.use_business_days,
    };

    serde_wasm_bindgen::to_value(&defaults).unwrap_or(JsValue::NULL)
}

/// Default convention values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultConventions {
    pub day_count: String,
    pub yield_convention: String,
    pub compounding: String,
    pub settlement_days: u32,
    pub ex_dividend_days: Option<u32>,
    pub use_business_days: bool,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date() {
        let date = parse_date("2024-06-15").unwrap();
        assert_eq!(date, Date::from_ymd(2024, 6, 15).unwrap());
    }

    #[test]
    fn test_parse_date_invalid() {
        assert!(parse_date("invalid").is_err());
        assert!(parse_date("2024/06/15").is_err());
    }

    #[test]
    fn test_parse_day_count() {
        // US 30/360
        assert!(matches!(
            parse_day_count("30/360"),
            DayCountConvention::Thirty360US
        ));
        assert!(matches!(
            parse_day_count("30/360 US"),
            DayCountConvention::Thirty360US
        ));
        // EU 30E/360
        assert!(matches!(
            parse_day_count("30E/360"),
            DayCountConvention::Thirty360E
        ));
        assert!(matches!(
            parse_day_count("30/360 EU"),
            DayCountConvention::Thirty360E
        ));
        assert!(matches!(
            parse_day_count("30/360E"),
            DayCountConvention::Thirty360E
        ));
        // Other conventions
        assert!(matches!(
            parse_day_count("ACT/365"),
            DayCountConvention::Act365Fixed
        ));
        assert!(matches!(
            parse_day_count("act/act"),
            DayCountConvention::ActActIcma
        ));
    }

    #[test]
    fn test_parse_frequency() {
        assert!(matches!(parse_frequency(1), Frequency::Annual));
        assert!(matches!(parse_frequency(2), Frequency::SemiAnnual));
        assert!(matches!(parse_frequency(4), Frequency::Quarterly));
    }

    #[test]
    fn test_create_bond() {
        let params = BondParams {
            coupon_rate: 5.0, // 5% as percentage
            maturity_date: "2030-06-15".to_string(),
            issue_date: "2020-06-15".to_string(),
            settlement_date: "2024-06-15".to_string(),
            face_value: Some(100.0),
            frequency: Some(2),
            day_count: Some("30/360".to_string()),
            currency: Some("USD".to_string()),
            first_coupon_date: None,
            call_schedule: None,
            volatility: None,
            market: None,
            instrument_type: None,
            yield_convention: None,
            compounding: None,
            settlement_days: None,
            ex_dividend_days: None,
            use_business_days: None,
        };

        let bond = create_bond(&params).unwrap();
        // Coupon rate stored as decimal (0.05 for 5%)
        assert_eq!(decimal_to_f64(bond.coupon_rate()), 0.05);
    }

    #[test]
    fn test_create_curve() {
        let reference = Date::from_ymd(2024, 6, 15).unwrap();
        let points = vec![
            CurvePoint {
                date: "2025-06-15".to_string(),
                rate: 4.0,
            },
            CurvePoint {
                date: "2026-06-15".to_string(),
                rate: 4.5,
            },
            CurvePoint {
                date: "2029-06-15".to_string(),
                rate: 5.0,
            },
        ];

        let curve = create_curve(reference, &points).unwrap();
        assert_eq!(curve.reference_date(), reference);
    }
}
