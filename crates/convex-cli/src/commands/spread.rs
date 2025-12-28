//! Spread command implementation.
//!
//! Calculates spread metrics (Z-spread, I-spread, G-spread, OAS).

use anyhow::Result;
use chrono::Datelike;
use clap::{Args, ValueEnum};
use rust_decimal::Decimal;

use convex_bonds::traits::BondAnalytics;
use convex_bonds::types::BondIdentifiers;
use convex_bonds::FixedRateBond;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};
use convex_curves::{DiscreteCurve, InterpolationMethod, RateCurve, ValueType};

use crate::cli::OutputFormat;
use crate::commands::{parse_date, validate_coupon, validate_price};
use crate::output::{print_header, KeyValue};

/// Arguments for the spread command.
#[derive(Args, Debug)]
pub struct SpreadArgs {
    /// Annual coupon rate (as percentage, e.g., 5.0 for 5%)
    #[arg(short, long)]
    pub coupon: f64,

    /// Maturity date (YYYY-MM-DD)
    #[arg(short, long)]
    pub maturity: String,

    /// Settlement date (YYYY-MM-DD). Defaults to today.
    #[arg(short, long)]
    pub settlement: Option<String>,

    /// Issue date (YYYY-MM-DD). Defaults to 10 years before maturity.
    #[arg(short, long)]
    pub issue: Option<String>,

    /// Clean price for spread calculation
    #[arg(short, long)]
    pub price: f64,

    /// Spread type to calculate
    #[arg(long, value_enum, default_value = "z-spread")]
    pub spread_type: SpreadType,

    /// Benchmark curve tenor points (comma-separated, e.g., "0.25,0.5,1,2,3,5,7,10")
    #[arg(long)]
    pub curve_tenors: Option<String>,

    /// Benchmark curve rates (comma-separated, e.g., "4.5,4.6,4.7,4.8,4.9,5.0,5.1,5.2")
    #[arg(long)]
    pub curve_rates: Option<String>,

    /// Face value (default: 100)
    #[arg(long, default_value = "100")]
    pub face: f64,

    /// Coupon frequency: 1=Annual, 2=SemiAnnual, 4=Quarterly, 12=Monthly
    #[arg(long, default_value = "2")]
    pub frequency: u32,

    /// Calculate all spread types
    #[arg(long)]
    pub all: bool,
}

/// Spread type to calculate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SpreadType {
    /// Zero-volatility spread
    #[value(name = "z-spread")]
    ZSpread,
    /// Interpolated spread (vs swap curve)
    #[value(name = "i-spread")]
    ISpread,
    /// Government spread
    #[value(name = "g-spread")]
    GSpread,
    /// Option-adjusted spread
    #[value(name = "oas")]
    Oas,
}

/// Execute the spread command.
pub fn execute(args: SpreadArgs, format: OutputFormat) -> Result<()> {
    // Validate inputs
    let coupon = validate_coupon(args.coupon)?;
    validate_price(args.price)?;

    let maturity = parse_date(&args.maturity)?;

    let settlement = if let Some(ref s) = args.settlement {
        parse_date(s)?
    } else {
        let today = chrono::Utc::now().date_naive();
        Date::from_ymd(today.year(), today.month(), today.day())
            .map_err(|e| anyhow::anyhow!("Invalid today date: {}", e))?
    };

    let issue = if let Some(ref i) = args.issue {
        parse_date(i)?
    } else {
        Date::from_ymd(maturity.year() - 10, maturity.month(), maturity.day())
            .map_err(|e| anyhow::anyhow!("Invalid issue date: {}", e))?
    };

    let frequency = match args.frequency {
        1 => Frequency::Annual,
        2 => Frequency::SemiAnnual,
        4 => Frequency::Quarterly,
        12 => Frequency::Monthly,
        _ => {
            return Err(anyhow::anyhow!(
                "Invalid frequency: {}. Use 1, 2, 4, or 12.",
                args.frequency
            ))
        }
    };

    // Create the bond
    let coupon_decimal = Decimal::from_f64_retain(coupon / 100.0)
        .ok_or_else(|| anyhow::anyhow!("Invalid coupon"))?;
    let face_decimal = Decimal::from_f64_retain(args.face)
        .ok_or_else(|| anyhow::anyhow!("Invalid face value"))?;
    let price_decimal = Decimal::from_f64_retain(args.price)
        .ok_or_else(|| anyhow::anyhow!("Invalid price"))?;

    let bond = FixedRateBond::builder()
        .identifiers(BondIdentifiers::new())
        .coupon_rate(coupon_decimal)
        .maturity(maturity)
        .issue_date(issue)
        .frequency(frequency)
        .face_value(face_decimal)
        .currency(Currency::USD)
        .build()?;

    // Parse curve data if provided
    let _curve = if let (Some(tenors_str), Some(rates_str)) =
        (&args.curve_tenors, &args.curve_rates)
    {
        let tenors: Vec<f64> = tenors_str
            .split(',')
            .map(|s| s.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Invalid tenor: {}", e))?;

        let rates: Vec<f64> = rates_str
            .split(',')
            .map(|s| {
                s.trim().parse::<f64>()
                    .map(|r| r / 100.0) // Convert from percentage
                    .map_err(|e| anyhow::anyhow!("Invalid rate: {}", e))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if tenors.len() != rates.len() {
            return Err(anyhow::anyhow!(
                "Number of tenors ({}) must match number of rates ({})",
                tenors.len(),
                rates.len()
            ));
        }

        Some(build_rate_curve(&tenors, &rates, settlement)?)
    } else {
        None
    };

    // Calculate YTM for reference
    let ytm_result = bond.yield_to_maturity(settlement, price_decimal, frequency)?;

    let mut results = Vec::new();

    // Bond details
    results.push(KeyValue::new("Bond Type", "Fixed Rate"));
    results.push(KeyValue::new("Coupon", format!("{}%", coupon)));
    results.push(KeyValue::new("Maturity", maturity.to_string()));
    results.push(KeyValue::new("Settlement", settlement.to_string()));
    results.push(KeyValue::from_decimal("Price", price_decimal, 6));
    results.push(KeyValue::new("YTM", format!("{:.4}%", ytm_result.yield_value * 100.0)));
    results.push(KeyValue::new("", "")); // Separator

    // Calculate spreads
    if args.all {
        // Calculate all spread types - note that full implementation would
        // require proper curve integration
        results.push(KeyValue::new(
            "Note",
            "Full spread calculation requires convex-analytics integration",
        ));

        if _curve.is_some() {
            results.push(KeyValue::new(
                "Z-Spread",
                "Use convex-analytics z_spread() for full calculation",
            ));
            results.push(KeyValue::new(
                "I-Spread",
                "Use convex-analytics i_spread() for full calculation",
            ));
            results.push(KeyValue::new(
                "G-Spread",
                "Use convex-analytics g_spread() for full calculation",
            ));
        } else {
            results.push(KeyValue::new(
                "Hint",
                "Provide --curve-tenors and --curve-rates for spread calculations",
            ));
        }
    } else {
        // Calculate single spread type
        match args.spread_type {
            SpreadType::ZSpread => {
                if _curve.is_some() {
                    results.push(KeyValue::new(
                        "Z-Spread",
                        "Use convex-analytics z_spread() for full calculation",
                    ));
                } else {
                    return Err(anyhow::anyhow!(
                        "Z-spread requires curve data. Use --curve-tenors and --curve-rates."
                    ));
                }
            }
            SpreadType::ISpread => {
                if _curve.is_some() {
                    results.push(KeyValue::new(
                        "I-Spread",
                        "Use convex-analytics i_spread() for full calculation",
                    ));
                } else {
                    return Err(anyhow::anyhow!(
                        "I-spread requires curve data. Use --curve-tenors and --curve-rates."
                    ));
                }
            }
            SpreadType::GSpread => {
                if _curve.is_some() {
                    results.push(KeyValue::new(
                        "G-Spread",
                        "Use convex-analytics g_spread() for full calculation",
                    ));
                } else {
                    return Err(anyhow::anyhow!(
                        "G-spread requires curve data. Use --curve-tenors and --curve-rates."
                    ));
                }
            }
            SpreadType::Oas => {
                results.push(KeyValue::new(
                    "OAS",
                    "OAS calculation requires volatility model (not yet implemented)",
                ));
            }
        }
    }

    // Output results
    match format {
        OutputFormat::Table => {
            print_header("Spread Analysis");
            crate::output::print_output(&results, format)?;
        }
        OutputFormat::Json => {
            let output: std::collections::HashMap<String, String> = results
                .iter()
                .filter(|r| !r.key.is_empty())
                .map(|r| (r.key.clone(), r.value.clone()))
                .collect();
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Csv => {
            crate::output::print_output(&results, format)?;
        }
        OutputFormat::Minimal => {
            // Output just the spread value
            if let Some(r) = results.iter().find(|r| r.key.contains("Spread")) {
                println!("{}", r.value);
            }
        }
    }

    Ok(())
}

/// Builds a rate curve from tenors and rates.
fn build_rate_curve(tenors: &[f64], rates: &[f64], reference_date: Date) -> Result<RateCurve<DiscreteCurve>> {
    let curve = DiscreteCurve::new(
        reference_date,
        tenors.to_vec(),
        rates.to_vec(),
        ValueType::continuous_zero(DayCountConvention::Act365Fixed),
        InterpolationMethod::MonotoneConvex,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    Ok(RateCurve::new(curve))
}
