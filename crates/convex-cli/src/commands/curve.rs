//! Curve command implementation.
//!
//! Builds and displays yield curves.

use anyhow::Result;
use chrono::Datelike;
use clap::{Args, Subcommand, ValueEnum};
use tabled::Tabled;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::Date;
use convex_curves::{DiscreteCurve, InterpolationMethod, TermStructure, ValueType};

use crate::cli::OutputFormat;
use crate::commands::parse_date;
use crate::output::{print_header, KeyValue};

/// Arguments for the curve command.
#[derive(Args, Debug)]
pub struct CurveArgs {
    #[command(subcommand)]
    pub command: CurveCommand,
}

/// Curve subcommands.
#[derive(Subcommand, Debug)]
pub enum CurveCommand {
    /// Build a curve from zero rates
    Build(BuildArgs),

    /// Query a curve at specific tenors
    Query(QueryArgs),

    /// Display curve data as a table
    Show(ShowArgs),
}

/// Arguments for building a curve.
#[derive(Args, Debug)]
pub struct BuildArgs {
    /// Reference date (YYYY-MM-DD). Defaults to today.
    #[arg(short, long)]
    pub reference_date: Option<String>,

    /// Tenors in years (comma-separated, e.g., "0.25,0.5,1,2,3,5,7,10")
    #[arg(short, long)]
    pub tenors: String,

    /// Zero rates in percent (comma-separated, e.g., "4.5,4.6,4.7,4.8,4.9,5.0,5.1,5.2")
    #[arg(short = 'z', long)]
    pub rates: String,

    /// Interpolation method
    #[arg(short, long, value_enum, default_value = "monotone-convex")]
    pub interpolation: InterpolationChoice,

    /// Output tenors to display (comma-separated). If not provided, shows input tenors.
    #[arg(long)]
    pub output_tenors: Option<String>,
}

/// Arguments for querying a curve.
#[derive(Args, Debug)]
pub struct QueryArgs {
    /// Reference date (YYYY-MM-DD). Defaults to today.
    #[arg(short, long)]
    pub reference_date: Option<String>,

    /// Tenors for input curve (comma-separated)
    #[arg(short, long)]
    pub tenors: String,

    /// Zero rates for input curve (comma-separated, in percent)
    #[arg(short = 'z', long)]
    pub rates: String,

    /// Tenor to query (in years)
    #[arg(short = 'q', long)]
    pub query_tenor: f64,

    /// Interpolation method
    #[arg(short, long, value_enum, default_value = "monotone-convex")]
    pub interpolation: InterpolationChoice,

    /// Value type to return
    #[arg(long, value_enum, default_value = "zero")]
    pub value_type: ValueTypeChoice,
}

/// Arguments for showing curve data.
#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Reference date (YYYY-MM-DD). Defaults to today.
    #[arg(short, long)]
    pub reference_date: Option<String>,

    /// Tenors for input curve (comma-separated)
    #[arg(short, long)]
    pub tenors: String,

    /// Zero rates for input curve (comma-separated, in percent)
    #[arg(short = 'z', long)]
    pub rates: String,

    /// Interpolation method
    #[arg(short, long, value_enum, default_value = "monotone-convex")]
    pub interpolation: InterpolationChoice,

    /// Number of points to display
    #[arg(long, default_value = "20")]
    pub points: usize,

    /// Maximum tenor to show (defaults to max input tenor)
    #[arg(long)]
    pub max_tenor: Option<f64>,
}

/// Interpolation method choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum InterpolationChoice {
    /// Linear interpolation
    #[value(name = "linear")]
    Linear,
    /// Log-linear interpolation
    #[value(name = "log-linear")]
    LogLinear,
    /// Cubic spline
    #[value(name = "cubic")]
    Cubic,
    /// Monotone convex (production default)
    #[value(name = "monotone-convex")]
    MonotoneConvex,
}

impl From<InterpolationChoice> for InterpolationMethod {
    fn from(choice: InterpolationChoice) -> Self {
        match choice {
            InterpolationChoice::Linear => InterpolationMethod::Linear,
            InterpolationChoice::LogLinear => InterpolationMethod::LogLinear,
            InterpolationChoice::Cubic => InterpolationMethod::CubicSpline,
            InterpolationChoice::MonotoneConvex => InterpolationMethod::MonotoneConvex,
        }
    }
}

/// Value type choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ValueTypeChoice {
    /// Zero rate
    #[default]
    Zero,
    /// Discount factor
    Discount,
    /// Forward rate
    Forward,
}

/// Execute the curve command.
pub fn execute(args: CurveArgs, format: OutputFormat) -> Result<()> {
    match args.command {
        CurveCommand::Build(build_args) => execute_build(build_args, format),
        CurveCommand::Query(query_args) => execute_query(query_args, format),
        CurveCommand::Show(show_args) => execute_show(show_args, format),
    }
}

/// Execute the build subcommand.
fn execute_build(args: BuildArgs, format: OutputFormat) -> Result<()> {
    let reference_date = get_reference_date(&args.reference_date)?;
    let (tenors, rates) = parse_curve_data(&args.tenors, &args.rates)?;

    let curve = build_curve(&tenors, &rates, reference_date, args.interpolation.into())?;

    // Determine output tenors
    let output_tenors = if let Some(ref ot) = args.output_tenors {
        parse_tenors(ot)?
    } else {
        tenors.clone()
    };

    let mut results = Vec::new();
    results.push(KeyValue::new("Reference Date", reference_date.to_string()));
    results.push(KeyValue::new(
        "Interpolation",
        format!("{:?}", args.interpolation),
    ));
    results.push(KeyValue::new("Points", tenors.len().to_string()));
    results.push(KeyValue::new("", "")); // Separator

    // Show curve values at output tenors using TermStructure trait
    for tenor in &output_tenors {
        let zero = curve.value_at(*tenor);
        let df = (-zero * tenor).exp();
        results.push(KeyValue::new(
            format!("{:.2}Y", tenor),
            format!("Zero: {:.4}%, DF: {:.6}", zero * 100.0, df),
        ));
    }

    output_results(&results, "Yield Curve", format)
}

/// Execute the query subcommand.
fn execute_query(args: QueryArgs, format: OutputFormat) -> Result<()> {
    let reference_date = get_reference_date(&args.reference_date)?;
    let (tenors, rates) = parse_curve_data(&args.tenors, &args.rates)?;

    let curve = build_curve(&tenors, &rates, reference_date, args.interpolation.into())?;

    let result = match args.value_type {
        ValueTypeChoice::Zero => {
            let zero = curve.value_at(args.query_tenor);
            KeyValue::new(
                format!("Zero Rate @ {:.2}Y", args.query_tenor),
                format!("{:.4}%", zero * 100.0),
            )
        }
        ValueTypeChoice::Discount => {
            let zero = curve.value_at(args.query_tenor);
            let df = (-zero * args.query_tenor).exp();
            KeyValue::new(
                format!("Discount Factor @ {:.2}Y", args.query_tenor),
                format!("{:.8}", df),
            )
        }
        ValueTypeChoice::Forward => {
            // Calculate 3-month forward rate at the query tenor
            let t1 = args.query_tenor;
            let t2 = args.query_tenor + 0.25;
            let z1 = curve.value_at(t1);
            let z2 = curve.value_at(t2);
            let fwd = (z2 * t2 - z1 * t1) / (t2 - t1);
            KeyValue::new(
                format!("3M Forward @ {:.2}Y", args.query_tenor),
                format!("{:.4}%", fwd * 100.0),
            )
        }
    };

    match format {
        OutputFormat::Table => {
            print_header("Curve Query Result");
            println!("{}: {}", result.key, result.value);
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "tenor": args.query_tenor,
                "value_type": format!("{:?}", args.value_type),
                "value": result.value
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Csv => {
            println!("tenor,value_type,value");
            println!(
                "{},{:?},{}",
                args.query_tenor, args.value_type, result.value
            );
        }
        OutputFormat::Minimal => {
            println!("{}", result.value);
        }
    }

    Ok(())
}

/// Execute the show subcommand.
fn execute_show(args: ShowArgs, format: OutputFormat) -> Result<()> {
    let reference_date = get_reference_date(&args.reference_date)?;
    let (tenors, rates) = parse_curve_data(&args.tenors, &args.rates)?;

    let curve = build_curve(&tenors, &rates, reference_date, args.interpolation.into())?;

    let max_tenor = args.max_tenor.unwrap_or_else(|| {
        *tenors.last().unwrap_or(&10.0)
    });

    #[derive(Tabled, serde::Serialize)]
    struct CurvePoint {
        #[tabled(rename = "Tenor")]
        tenor: String,
        #[tabled(rename = "Zero Rate (%)")]
        zero_rate: String,
        #[tabled(rename = "Discount Factor")]
        discount_factor: String,
        #[tabled(rename = "Forward (3M)")]
        forward_rate: String,
    }

    let mut points = Vec::new();
    let step = max_tenor / args.points as f64;

    for i in 1..=args.points {
        let t = step * i as f64;
        let zero = curve.value_at(t);
        let df = (-zero * t).exp();

        // Calculate 3M forward
        let t2 = t + 0.25;
        let z2 = curve.value_at(t2);
        let fwd = if t2 > t { (z2 * t2 - zero * t) / (t2 - t) } else { zero };

        points.push(CurvePoint {
            tenor: format!("{:.2}Y", t),
            zero_rate: format!("{:.4}", zero * 100.0),
            discount_factor: format!("{:.6}", df),
            forward_rate: format!("{:.4}", fwd * 100.0),
        });
    }

    match format {
        OutputFormat::Table => {
            print_header("Yield Curve");
            println!("Reference Date: {}", reference_date);
            println!("Interpolation: {:?}", args.interpolation);
            println!();

            use tabled::{settings::Style, Table};
            let table = Table::new(&points).with(Style::rounded()).to_string();
            println!("{}", table);
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&points)?);
        }
        OutputFormat::Csv => {
            let mut wtr = csv::Writer::from_writer(std::io::stdout());
            for point in &points {
                wtr.serialize(point)?;
            }
            wtr.flush()?;
        }
        OutputFormat::Minimal => {
            for point in &points {
                println!("{} {}", point.tenor, point.zero_rate);
            }
        }
    }

    Ok(())
}

/// Gets the reference date from an optional string.
fn get_reference_date(date_str: &Option<String>) -> Result<Date> {
    if let Some(ref s) = date_str {
        parse_date(s).map_err(|e| anyhow::anyhow!("{}", e))
    } else {
        let today = chrono::Utc::now().date_naive();
        Date::from_ymd(today.year(), today.month(), today.day())
            .map_err(|e| anyhow::anyhow!("Invalid today date: {}", e))
    }
}

/// Parses curve data from comma-separated strings.
fn parse_curve_data(tenors_str: &str, rates_str: &str) -> Result<(Vec<f64>, Vec<f64>)> {
    let tenors = parse_tenors(tenors_str)?;
    let rates: Vec<f64> = rates_str
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<f64>()
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

    Ok((tenors, rates))
}

/// Parses tenors from comma-separated string.
fn parse_tenors(tenors_str: &str) -> Result<Vec<f64>> {
    tenors_str
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<f64>()
                .map_err(|e| anyhow::anyhow!("Invalid tenor: {}", e))
        })
        .collect()
}

/// Builds a discrete curve from tenors and rates.
fn build_curve(
    tenors: &[f64],
    rates: &[f64],
    reference_date: Date,
    interpolation: InterpolationMethod,
) -> Result<DiscreteCurve> {
    DiscreteCurve::new(
        reference_date,
        tenors.to_vec(),
        rates.to_vec(),
        ValueType::continuous_zero(DayCountConvention::Act365Fixed),
        interpolation,
    )
    .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Outputs results in the specified format.
fn output_results(results: &[KeyValue], title: &str, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Table => {
            print_header(title);
            crate::output::print_output(results, format)?;
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
            crate::output::print_output(results, format)?;
        }
        OutputFormat::Minimal => {
            for r in results {
                if !r.key.is_empty() && !r.value.is_empty() {
                    println!("{}", r.value);
                }
            }
        }
    }
    Ok(())
}
