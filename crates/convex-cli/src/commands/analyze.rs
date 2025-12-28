//! Analyze command implementation.
//!
//! Calculates risk metrics for a bond.

use anyhow::Result;
use chrono::Datelike;
use clap::Args;
use rust_decimal::Decimal;
use tabled::Tabled;

use convex_bonds::traits::{Bond, BondAnalytics};
use convex_bonds::types::BondIdentifiers;
use convex_bonds::FixedRateBond;
use convex_core::types::{Currency, Date, Frequency};

use crate::cli::OutputFormat;
use crate::commands::{parse_date, validate_coupon, validate_price, validate_yield};
use crate::output::{print_header, KeyValue};

/// Arguments for the analyze command.
#[derive(Args, Debug)]
pub struct AnalyzeArgs {
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

    /// Clean price for analysis
    #[arg(short, long)]
    pub price: Option<f64>,

    /// Yield for analysis (as percentage). Used if price not provided.
    #[arg(short, long)]
    pub yield_value: Option<f64>,

    /// Face value (default: 100)
    #[arg(long, default_value = "100")]
    pub face: f64,

    /// Coupon frequency: 1=Annual, 2=SemiAnnual, 4=Quarterly, 12=Monthly
    #[arg(long, default_value = "2")]
    pub frequency: u32,

    /// Show cashflows
    #[arg(long)]
    pub cashflows: bool,
}

/// Execute the analyze command.
pub fn execute(args: AnalyzeArgs, format: OutputFormat) -> Result<()> {
    // Validate inputs
    let coupon = validate_coupon(args.coupon)?;

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

    let bond = FixedRateBond::builder()
        .identifiers(BondIdentifiers::new())
        .coupon_rate(coupon_decimal)
        .maturity(maturity)
        .issue_date(issue)
        .frequency(frequency)
        .face_value(face_decimal)
        .currency(Currency::USD)
        .build()?;

    // Determine yield
    let ytm: f64 = if let Some(price) = args.price {
        validate_price(price)?;
        let price_decimal =
            Decimal::from_f64_retain(price).ok_or_else(|| anyhow::anyhow!("Invalid price"))?;
        let ytm_result = bond.yield_to_maturity(settlement, price_decimal, frequency)?;
        ytm_result.yield_value
    } else if let Some(y) = args.yield_value {
        validate_yield(y)?;
        y / 100.0
    } else {
        // Default to coupon rate
        coupon / 100.0
    };

    // Calculate all metrics
    let accrued = bond.accrued_interest(settlement);
    let clean_price = bond.clean_price_from_yield(settlement, ytm, frequency)?;
    let dirty_price = bond.dirty_price_from_yield(settlement, ytm, frequency)?;
    let mac_duration = bond.macaulay_duration(settlement, ytm, frequency)?;
    let mod_duration = bond.modified_duration(settlement, ytm, frequency)?;
    let convexity = bond.convexity(settlement, ytm, frequency)?;
    let dv01 = bond.dv01(settlement, ytm, 100.0, frequency)?;

    let mut results = Vec::new();

    // Bond details
    results.push(KeyValue::new("Bond Type", "Fixed Rate"));
    results.push(KeyValue::new("Coupon", format!("{}%", coupon)));
    results.push(KeyValue::new("Maturity", maturity.to_string()));
    results.push(KeyValue::new("Settlement", settlement.to_string()));
    results.push(KeyValue::new("", "")); // Separator

    // Pricing
    results.push(KeyValue::new("Yield to Maturity", format!("{:.4}%", ytm * 100.0)));
    results.push(KeyValue::new("Clean Price", format!("{:.6}", clean_price)));
    results.push(KeyValue::new("Dirty Price", format!("{:.6}", dirty_price)));
    results.push(KeyValue::from_decimal("Accrued Interest", accrued, 6));
    results.push(KeyValue::new("", "")); // Separator

    // Risk metrics
    results.push(KeyValue::new("Macaulay Duration", format!("{:.4}", mac_duration)));
    results.push(KeyValue::new("Modified Duration", format!("{:.4}", mod_duration)));
    results.push(KeyValue::new("Convexity", format!("{:.4}", convexity)));
    results.push(KeyValue::new("DV01 (per $100)", format!("{:.6}", dv01)));

    // Output results
    match format {
        OutputFormat::Table => {
            print_header("Bond Analytics");
            crate::output::print_output(&results, format)?;

            if args.cashflows {
                print_cashflows(&bond, settlement)?;
            }
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
            // Output just the key metrics
            println!(
                "Duration: {:.4}, Convexity: {:.4}, DV01: {:.6}",
                mod_duration, convexity, dv01
            );
        }
    }

    Ok(())
}

/// Print cashflow schedule.
fn print_cashflows(bond: &FixedRateBond, settlement: Date) -> Result<()> {
    use tabled::{Table, settings::Style};

    #[derive(Tabled)]
    struct CashflowRow {
        #[tabled(rename = "Date")]
        date: String,
        #[tabled(rename = "Cashflow")]
        cashflow: String,
        #[tabled(rename = "Type")]
        cf_type: String,
    }

    let cashflows = bond.cash_flows(settlement);
    let rows: Vec<CashflowRow> = cashflows
        .iter()
        .map(|cf| CashflowRow {
            date: cf.date.to_string(),
            cashflow: format!("{:.4}", cf.amount),
            cf_type: format!("{:?}", cf.flow_type),
        })
        .collect();

    if !rows.is_empty() {
        print_header("Cashflow Schedule");
        let table = Table::new(&rows).with(Style::rounded()).to_string();
        println!("{}", table);
    }

    Ok(())
}
