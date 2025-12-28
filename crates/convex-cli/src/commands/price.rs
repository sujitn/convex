//! Price command implementation.
//!
//! Calculates bond price from yield or yield from price.

use anyhow::Result;
use chrono::Datelike;
use clap::Args;
use rust_decimal::Decimal;
use serde::Serialize;
use tabled::Tabled;

use convex_bonds::traits::{Bond, BondAnalytics};
use convex_bonds::types::BondIdentifiers;
use convex_bonds::FixedRateBond;
use convex_core::types::{Currency, Date, Frequency};

use crate::cli::OutputFormat;
use crate::commands::{parse_date, validate_coupon, validate_price, validate_yield};
use crate::output::{print_header, KeyValue};

/// Arguments for the price command.
#[derive(Args, Debug)]
pub struct PriceArgs {
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

    /// Yield to maturity (as percentage). If provided, calculates price.
    #[arg(short, long, group = "calc_mode")]
    pub yield_value: Option<f64>,

    /// Clean price. If provided, calculates yield.
    #[arg(short, long, group = "calc_mode")]
    pub price: Option<f64>,

    /// Face value (default: 100)
    #[arg(long, default_value = "100")]
    pub face: f64,

    /// Coupon frequency: 1=Annual, 2=SemiAnnual, 4=Quarterly, 12=Monthly
    #[arg(long, default_value = "2")]
    pub frequency: u32,

    /// Currency (USD, EUR, GBP, etc.)
    #[arg(long, default_value = "USD")]
    pub currency: String,
}

/// Price calculation result.
#[derive(Debug, Serialize, Tabled)]
#[allow(dead_code)]
pub struct PriceResult {
    #[tabled(rename = "Metric")]
    pub metric: String,
    #[tabled(rename = "Value")]
    pub value: String,
}

/// Execute the price command.
pub fn execute(args: PriceArgs, format: OutputFormat) -> Result<()> {
    // Validate inputs
    let coupon = validate_coupon(args.coupon)?;

    let maturity = parse_date(&args.maturity)?;

    let settlement = if let Some(ref s) = args.settlement {
        parse_date(s)?
    } else {
        // Default to today
        let today = chrono::Utc::now().date_naive();
        Date::from_ymd(today.year(), today.month(), today.day())
            .map_err(|e| anyhow::anyhow!("Invalid today date: {}", e))?
    };

    let issue = if let Some(ref i) = args.issue {
        parse_date(i)?
    } else {
        // Default to 10 years before maturity
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

    let currency = match args.currency.to_uppercase().as_str() {
        "USD" => Currency::USD,
        "EUR" => Currency::EUR,
        "GBP" => Currency::GBP,
        "JPY" => Currency::JPY,
        "CHF" => Currency::CHF,
        "CAD" => Currency::CAD,
        "AUD" => Currency::AUD,
        _ => return Err(anyhow::anyhow!("Unsupported currency: {}", args.currency)),
    };

    // Create the bond
    let coupon_decimal = Decimal::from_f64_retain(coupon / 100.0)
        .ok_or_else(|| anyhow::anyhow!("Invalid coupon"))?;
    let face_decimal = Decimal::from_f64_retain(args.face)
        .ok_or_else(|| anyhow::anyhow!("Invalid face value"))?;

    let bond = FixedRateBond::builder()
        .identifiers(BondIdentifiers::new())
        .coupon_rate(coupon_decimal)
        .maturity(maturity)
        .issue_date(issue)
        .frequency(frequency)
        .face_value(face_decimal)
        .currency(currency)
        .build()?;

    let mut results = Vec::new();

    // Add bond details
    results.push(KeyValue::new("Bond Type", "Fixed Rate"));
    results.push(KeyValue::new("Coupon", format!("{}%", coupon)));
    results.push(KeyValue::new("Maturity", maturity.to_string()));
    results.push(KeyValue::new("Settlement", settlement.to_string()));
    results.push(KeyValue::new("Frequency", format!("{:?}", frequency)));
    results.push(KeyValue::new("Face Value", format!("{}", args.face)));

    // Calculate based on mode
    if let Some(yield_pct) = args.yield_value {
        validate_yield(yield_pct)?;
        let ytm = yield_pct / 100.0;

        let clean_price = bond.clean_price_from_yield(settlement, ytm, frequency)?;
        let dirty_price = bond.dirty_price_from_yield(settlement, ytm, frequency)?;
        let accrued = bond.accrued_interest(settlement);

        results.push(KeyValue::new("", "")); // Separator
        results.push(KeyValue::new("Yield (Input)", format!("{}%", yield_pct)));
        results.push(KeyValue::new("Clean Price", format!("{:.6}", clean_price)));
        results.push(KeyValue::new("Dirty Price", format!("{:.6}", dirty_price)));
        results.push(KeyValue::from_decimal("Accrued Interest", accrued, 6));
    } else if let Some(price) = args.price {
        validate_price(price)?;
        let price_decimal =
            Decimal::from_f64_retain(price).ok_or_else(|| anyhow::anyhow!("Invalid price"))?;

        let ytm_result = bond.yield_to_maturity(settlement, price_decimal, frequency)?;
        let accrued = bond.accrued_interest(settlement);
        let accrued_f64: f64 = accrued.try_into().unwrap_or(0.0);
        let dirty_price = price + accrued_f64;

        results.push(KeyValue::new("", "")); // Separator
        results.push(KeyValue::new("Clean Price (Input)", format!("{:.6}", price)));
        results.push(KeyValue::new("Dirty Price", format!("{:.6}", dirty_price)));
        results.push(KeyValue::from_decimal("Accrued Interest", accrued, 6));
        results.push(KeyValue::new("Yield to Maturity", format!("{:.4}%", ytm_result.yield_value * 100.0)));
    } else {
        // Default: calculate at par (yield = coupon)
        let ytm = coupon / 100.0;

        let clean_price = bond.clean_price_from_yield(settlement, ytm, frequency)?;
        let dirty_price = bond.dirty_price_from_yield(settlement, ytm, frequency)?;
        let accrued = bond.accrued_interest(settlement);

        results.push(KeyValue::new("", "")); // Separator
        results.push(KeyValue::new(
            "Yield (Par)",
            format!("{}%", coupon),
        ));
        results.push(KeyValue::new("Clean Price", format!("{:.6}", clean_price)));
        results.push(KeyValue::new("Dirty Price", format!("{:.6}", dirty_price)));
        results.push(KeyValue::from_decimal("Accrued Interest", accrued, 6));
    }

    // Output results
    match format {
        OutputFormat::Table => {
            print_header("Bond Pricing Results");
            crate::output::print_output(&results, format)?;
        }
        OutputFormat::Json => {
            let output: std::collections::HashMap<String, String> =
                results.iter().filter(|r| !r.key.is_empty()).map(|r| (r.key.clone(), r.value.clone())).collect();
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Csv => {
            crate::output::print_output(&results, format)?;
        }
        OutputFormat::Minimal => {
            // Output just the calculated value
            if args.yield_value.is_some() {
                if let Some(r) = results.iter().find(|r| r.key == "Clean Price") {
                    println!("{}", r.value);
                }
            } else if args.price.is_some() {
                if let Some(r) = results.iter().find(|r| r.key == "Yield to Maturity") {
                    println!("{}", r.value);
                }
            }
        }
    }

    Ok(())
}
