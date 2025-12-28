//! Convex CLI - Command-line interface for fixed income analytics.
//!
//! # Usage
//!
//! ```bash
//! # Price a bond
//! convex price --coupon 5.0 --maturity 2030-01-15 --yield 4.5
//!
//! # Build a curve
//! convex curve --type sofr-ois --date 2024-01-15
//!
//! # Analyze a bond
//! convex analyze --coupon 5.0 --maturity 2030-01-15 --price 102.5
//!
//! # Calculate spread
//! convex spread --coupon 5.0 --maturity 2030-01-15 --price 102.5 --curve USD.SOFR
//! ```

use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod error;
mod output;

use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up output format
    let format = cli.format;

    // Execute command
    match cli.command {
        Commands::Price(args) => commands::price::execute(args, format)?,
        Commands::Curve(args) => commands::curve::execute(args, format)?,
        Commands::Analyze(args) => commands::analyze::execute(args, format)?,
        Commands::Spread(args) => commands::spread::execute(args, format)?,
        Commands::Bootstrap(args) => commands::bootstrap::execute(args, format)?,
        Commands::Config(args) => commands::config::execute(args, format)?,
    }

    Ok(())
}
