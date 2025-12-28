//! CLI argument definitions.

use clap::{Parser, Subcommand, ValueEnum};

use crate::commands::{
    AnalyzeArgs, BootstrapArgs, ConfigArgs, CurveArgs, PriceArgs, SpreadArgs,
};

/// Convex - High-performance fixed income analytics CLI
#[derive(Parser)]
#[command(name = "convex")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Output format
    #[arg(short, long, value_enum, default_value = "table", global = true)]
    pub format: OutputFormat,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Subcommand)]
pub enum Commands {
    /// Price a bond given yield or calculate yield from price
    Price(PriceArgs),

    /// Build and display a yield curve
    Curve(CurveArgs),

    /// Analyze a bond (duration, convexity, DV01, etc.)
    Analyze(AnalyzeArgs),

    /// Calculate spread metrics (Z-spread, I-spread, G-spread, OAS)
    Spread(SpreadArgs),

    /// Bootstrap a curve from market instruments
    Bootstrap(BootstrapArgs),

    /// Manage configurations
    Config(ConfigArgs),
}

/// Output format options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable table format
    #[default]
    Table,
    /// JSON format
    Json,
    /// CSV format
    Csv,
    /// Minimal output (just the value)
    Minimal,
}
