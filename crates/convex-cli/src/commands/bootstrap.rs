//! Bootstrap command implementation.
//!
//! Bootstraps yield curves from market instruments.

use anyhow::Result;
use chrono::Datelike;
use clap::{Args, Subcommand, ValueEnum};
use tabled::Tabled;

use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Date, Frequency};
use convex_curves::calibration::{
    CalibrationResult, Deposit, FitterConfig, GlobalFitter, InstrumentSet, Ois, SequentialBootstrapper, Swap,
};
use convex_curves::TermStructure;

use crate::cli::OutputFormat;
use crate::commands::parse_date;
use crate::output::print_header;

/// Arguments for the bootstrap command.
#[derive(Args, Debug)]
pub struct BootstrapArgs {
    #[command(subcommand)]
    pub command: BootstrapCommand,
}

/// Bootstrap subcommands.
#[derive(Subcommand, Debug)]
pub enum BootstrapCommand {
    /// Bootstrap from deposits and swaps
    Mixed(MixedArgs),

    /// Bootstrap OIS curve
    Ois(OisArgs),

    /// Bootstrap from a simple term structure file
    File(FileArgs),
}

/// Arguments for mixed instrument bootstrap.
#[derive(Args, Debug)]
pub struct MixedArgs {
    /// Reference date (YYYY-MM-DD). Defaults to today.
    #[arg(short, long)]
    pub reference_date: Option<String>,

    /// Deposit tenors (comma-separated, e.g., "0.25,0.5,1")
    #[arg(long)]
    pub deposit_tenors: Option<String>,

    /// Deposit rates in percent (comma-separated, e.g., "4.5,4.6,4.7")
    #[arg(long)]
    pub deposit_rates: Option<String>,

    /// Swap tenors (comma-separated, e.g., "2,3,5,7,10")
    #[arg(long)]
    pub swap_tenors: Option<String>,

    /// Swap rates in percent (comma-separated, e.g., "4.8,4.9,5.0,5.1,5.2")
    #[arg(long)]
    pub swap_rates: Option<String>,

    /// Calibration method
    #[arg(long, value_enum, default_value = "global")]
    pub method: CalibrationMethod,

    /// Day count convention for deposits
    #[arg(long, value_enum, default_value = "act360")]
    pub deposit_daycount: DayCountChoice,

    /// Day count convention for swaps
    #[arg(long, value_enum, default_value = "thirty360")]
    pub swap_daycount: DayCountChoice,

    /// Swap frequency
    #[arg(long, value_enum, default_value = "semi-annual")]
    pub swap_frequency: FrequencyChoice,

    /// Show calibration residuals
    #[arg(long)]
    pub show_residuals: bool,
}

/// Arguments for OIS bootstrap.
#[derive(Args, Debug)]
pub struct OisArgs {
    /// Reference date (YYYY-MM-DD). Defaults to today.
    #[arg(short, long)]
    pub reference_date: Option<String>,

    /// OIS tenors (comma-separated, e.g., "0.25,0.5,1,2,3,5,7,10")
    #[arg(short, long)]
    pub tenors: String,

    /// OIS rates in percent (comma-separated)
    #[arg(short = 'z', long)]
    pub rates: String,

    /// Day count convention
    #[arg(long, value_enum, default_value = "act360")]
    pub daycount: DayCountChoice,

    /// Calibration method
    #[arg(long, value_enum, default_value = "global")]
    pub method: CalibrationMethod,
}

/// Arguments for file-based bootstrap.
#[derive(Args, Debug)]
pub struct FileArgs {
    /// Path to CSV file with columns: tenor,rate,instrument_type
    #[arg(short, long)]
    pub file: String,

    /// Reference date (YYYY-MM-DD). Defaults to today.
    #[arg(short, long)]
    pub reference_date: Option<String>,
}

/// Calibration method choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum CalibrationMethod {
    /// Global fit (Levenberg-Marquardt optimization)
    #[default]
    Global,
    /// Sequential bootstrap (iterative)
    Sequential,
}

/// Day count convention choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum DayCountChoice {
    /// Act/360
    #[default]
    #[value(name = "act360")]
    Act360,
    /// Act/365 Fixed
    #[value(name = "act365")]
    Act365,
    /// 30/360 US
    #[value(name = "thirty360")]
    Thirty360,
}

impl From<DayCountChoice> for DayCountConvention {
    fn from(choice: DayCountChoice) -> Self {
        match choice {
            DayCountChoice::Act360 => DayCountConvention::Act360,
            DayCountChoice::Act365 => DayCountConvention::Act365Fixed,
            DayCountChoice::Thirty360 => DayCountConvention::Thirty360US,
        }
    }
}

/// Frequency choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum FrequencyChoice {
    /// Annual (1/year)
    Annual,
    /// Semi-annual (2/year)
    #[default]
    #[value(name = "semi-annual")]
    SemiAnnual,
    /// Quarterly (4/year)
    Quarterly,
}

impl From<FrequencyChoice> for Frequency {
    fn from(choice: FrequencyChoice) -> Self {
        match choice {
            FrequencyChoice::Annual => Frequency::Annual,
            FrequencyChoice::SemiAnnual => Frequency::SemiAnnual,
            FrequencyChoice::Quarterly => Frequency::Quarterly,
        }
    }
}

/// Execute the bootstrap command.
pub fn execute(args: BootstrapArgs, format: OutputFormat) -> Result<()> {
    match args.command {
        BootstrapCommand::Mixed(mixed_args) => execute_mixed(mixed_args, format),
        BootstrapCommand::Ois(ois_args) => execute_ois(ois_args, format),
        BootstrapCommand::File(file_args) => execute_file(file_args, format),
    }
}

/// Execute mixed instrument bootstrap.
fn execute_mixed(args: MixedArgs, format: OutputFormat) -> Result<()> {
    let reference_date = get_reference_date(&args.reference_date)?;
    let deposit_dc: DayCountConvention = args.deposit_daycount.into();
    let swap_dc: DayCountConvention = args.swap_daycount.into();
    let swap_freq: Frequency = args.swap_frequency.into();

    let mut instruments = InstrumentSet::new();

    // Add deposits
    if let (Some(tenors_str), Some(rates_str)) = (&args.deposit_tenors, &args.deposit_rates) {
        let tenors = parse_tenors(tenors_str)?;
        let rates = parse_rates(rates_str)?;

        if tenors.len() != rates.len() {
            return Err(anyhow::anyhow!(
                "Deposit tenors ({}) and rates ({}) must have same length",
                tenors.len(),
                rates.len()
            ));
        }

        for (tenor, rate) in tenors.iter().zip(rates.iter()) {
            instruments = instruments.with(Deposit::from_tenor(reference_date, *tenor, *rate, deposit_dc));
        }
    }

    // Add swaps
    if let (Some(tenors_str), Some(rates_str)) = (&args.swap_tenors, &args.swap_rates) {
        let tenors = parse_tenors(tenors_str)?;
        let rates = parse_rates(rates_str)?;

        if tenors.len() != rates.len() {
            return Err(anyhow::anyhow!(
                "Swap tenors ({}) and rates ({}) must have same length",
                tenors.len(),
                rates.len()
            ));
        }

        for (tenor, rate) in tenors.iter().zip(rates.iter()) {
            instruments = instruments.with(Swap::from_tenor(reference_date, *tenor, *rate, swap_freq, swap_dc));
        }
    }

    if instruments.is_empty() {
        return Err(anyhow::anyhow!(
            "No instruments provided. Use --deposit-tenors/--deposit-rates or --swap-tenors/--swap-rates."
        ));
    }

    // Calibrate
    let result = match args.method {
        CalibrationMethod::Global => {
            let fitter = GlobalFitter::with_config(FitterConfig::default());
            fitter.fit(reference_date, &instruments)?
        }
        CalibrationMethod::Sequential => {
            let bootstrapper = SequentialBootstrapper::new();
            bootstrapper.bootstrap(reference_date, &instruments)?
        }
    };

    output_calibration_result(&result, reference_date, args.show_residuals, format)
}

/// Execute OIS bootstrap.
fn execute_ois(args: OisArgs, format: OutputFormat) -> Result<()> {
    let reference_date = get_reference_date(&args.reference_date)?;
    let daycount: DayCountConvention = args.daycount.into();

    let tenors = parse_tenors(&args.tenors)?;
    let rates = parse_rates(&args.rates)?;

    if tenors.len() != rates.len() {
        return Err(anyhow::anyhow!(
            "OIS tenors ({}) and rates ({}) must have same length",
            tenors.len(),
            rates.len()
        ));
    }

    let mut instruments = InstrumentSet::new();
    for (tenor, rate) in tenors.iter().zip(rates.iter()) {
        instruments = instruments.with(Ois::from_tenor(reference_date, *tenor, *rate, daycount));
    }

    let result = match args.method {
        CalibrationMethod::Global => {
            let fitter = GlobalFitter::with_config(FitterConfig::default());
            fitter.fit(reference_date, &instruments)?
        }
        CalibrationMethod::Sequential => {
            let bootstrapper = SequentialBootstrapper::new();
            bootstrapper.bootstrap(reference_date, &instruments)?
        }
    };

    output_calibration_result(&result, reference_date, false, format)
}

/// Execute file-based bootstrap.
fn execute_file(args: FileArgs, format: OutputFormat) -> Result<()> {
    let reference_date = get_reference_date(&args.reference_date)?;

    // Read CSV file
    let content = std::fs::read_to_string(&args.file)
        .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", args.file, e))?;

    let mut instruments = InstrumentSet::new();
    let deposit_dc = DayCountConvention::Act360;
    let swap_dc = DayCountConvention::Thirty360US;
    let swap_freq = Frequency::SemiAnnual;

    for (i, line) in content.lines().enumerate() {
        // Skip header
        if i == 0 && line.to_lowercase().contains("tenor") {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 2 {
            continue;
        }

        let tenor: f64 = parts[0].trim().parse()
            .map_err(|_| anyhow::anyhow!("Invalid tenor on line {}: {}", i + 1, parts[0]))?;
        let rate: f64 = parts[1].trim().parse::<f64>()
            .map_err(|_| anyhow::anyhow!("Invalid rate on line {}: {}", i + 1, parts[1]))?
            / 100.0; // Convert from percentage

        let instrument_type = if parts.len() > 2 {
            parts[2].trim().to_lowercase()
        } else {
            // Default: deposits for short tenors, swaps for longer
            if tenor <= 1.0 { "deposit".to_string() } else { "swap".to_string() }
        };

        match instrument_type.as_str() {
            "deposit" | "depo" => {
                instruments = instruments.with(Deposit::from_tenor(reference_date, tenor, rate, deposit_dc));
            }
            "swap" => {
                instruments = instruments.with(Swap::from_tenor(reference_date, tenor, rate, swap_freq, swap_dc));
            }
            "ois" => {
                instruments = instruments.with(Ois::from_tenor(reference_date, tenor, rate, deposit_dc));
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "Unknown instrument type on line {}: {}",
                    i + 1,
                    instrument_type
                ));
            }
        }
    }

    if instruments.is_empty() {
        return Err(anyhow::anyhow!("No valid instruments found in file"));
    }

    let fitter = GlobalFitter::with_config(FitterConfig::default());
    let result = fitter.fit(reference_date, &instruments)?;

    output_calibration_result(&result, reference_date, false, format)
}

/// Output calibration result.
fn output_calibration_result(
    result: &CalibrationResult,
    reference_date: Date,
    show_residuals: bool,
    format: OutputFormat,
) -> Result<()> {
    #[derive(Tabled, serde::Serialize)]
    struct CurvePoint {
        #[tabled(rename = "Tenor")]
        tenor: String,
        #[tabled(rename = "Zero Rate (%)")]
        zero_rate: String,
        #[tabled(rename = "Discount Factor")]
        discount_factor: String,
    }

    // Generate curve output at standard tenors
    let standard_tenors = [0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0, 15.0, 20.0, 30.0];
    let mut points = Vec::new();

    for &tenor in &standard_tenors {
        let zero = result.curve.value_at(tenor);
        let df = (-zero * tenor).exp();
        points.push(CurvePoint {
            tenor: format!("{:.2}Y", tenor),
            zero_rate: format!("{:.4}", zero * 100.0),
            discount_factor: format!("{:.6}", df),
        });
    }

    match format {
        OutputFormat::Table => {
            print_header("Bootstrapped Curve");
            println!("Reference Date: {}", reference_date);
            println!("Iterations: {}", result.iterations);
            println!("RMS Error: {:.6}", result.rms_error);
            println!();

            use tabled::{settings::Style, Table};
            let table = Table::new(&points).with(Style::rounded()).to_string();
            println!("{}", table);

            if show_residuals {
                print_header("Calibration Residuals");
                for (i, residual) in result.residuals.iter().enumerate() {
                    println!("  Instrument {}: {:.6} bps", i + 1, residual * 10000.0);
                }
            }
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "reference_date": reference_date.to_string(),
                "iterations": result.iterations,
                "rms_error": result.rms_error,
                "curve_points": points,
                "residuals": result.residuals
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Csv => {
            let mut wtr = csv::Writer::from_writer(std::io::stdout());
            for point in &points {
                wtr.serialize(point)?;
            }
            wtr.flush()?;
        }
        OutputFormat::Minimal => {
            // Just output RMS error
            println!("RMS: {:.6}", result.rms_error);
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

/// Parses rates from comma-separated string (converts from percentage).
fn parse_rates(rates_str: &str) -> Result<Vec<f64>> {
    rates_str
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<f64>()
                .map(|r| r / 100.0) // Convert from percentage
                .map_err(|e| anyhow::anyhow!("Invalid rate: {}", e))
        })
        .collect()
}
