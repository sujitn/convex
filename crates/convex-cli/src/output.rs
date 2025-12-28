//! Output formatting utilities.

#![allow(dead_code)]

use colored::Colorize;
use rust_decimal::Decimal;
use serde::Serialize;
use tabled::{
    settings::{object::Columns, Alignment, Modify, Style},
    Table, Tabled,
};

use crate::cli::OutputFormat;

/// Formats and prints output based on the specified format.
pub fn print_output<T: Serialize + Tabled>(data: &[T], format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Table => print_table(data),
        OutputFormat::Json => print_json(data),
        OutputFormat::Csv => print_csv(data),
        OutputFormat::Minimal => print_minimal(data),
    }
}

/// Prints a single result.
pub fn print_single<T: Serialize>(data: &T, format: OutputFormat) -> anyhow::Result<()> {
    match format {
        OutputFormat::Table | OutputFormat::Minimal => {
            println!("{}", serde_json::to_string_pretty(data)?);
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(data)?);
        }
        OutputFormat::Csv => {
            let mut wtr = csv::Writer::from_writer(std::io::stdout());
            wtr.serialize(data)?;
            wtr.flush()?;
        }
    }
    Ok(())
}

/// Prints data as a formatted table.
fn print_table<T: Tabled>(data: &[T]) -> anyhow::Result<()> {
    if data.is_empty() {
        println!("No results.");
        return Ok(());
    }

    let table = Table::new(data)
        .with(Style::rounded())
        .with(Modify::new(Columns::first()).with(Alignment::left()))
        .to_string();

    println!("{}", table);
    Ok(())
}

/// Prints data as JSON.
fn print_json<T: Serialize>(data: &[T]) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(data)?);
    Ok(())
}

/// Prints data as CSV.
fn print_csv<T: Serialize>(data: &[T]) -> anyhow::Result<()> {
    let mut wtr = csv::Writer::from_writer(std::io::stdout());
    for item in data {
        wtr.serialize(item)?;
    }
    wtr.flush()?;
    Ok(())
}

/// Prints minimal output (first value only).
fn print_minimal<T: Serialize>(data: &[T]) -> anyhow::Result<()> {
    if let Some(first) = data.first() {
        println!("{}", serde_json::to_string(first)?);
    }
    Ok(())
}

/// Formats a decimal as a percentage string.
pub fn format_percent(value: Decimal) -> String {
    format!("{}%", value * Decimal::from(100))
}

/// Formats a decimal as a basis points string.
pub fn format_bps(value: Decimal) -> String {
    format!("{} bps", value * Decimal::from(10000))
}

/// Formats a price.
pub fn format_price(value: Decimal) -> String {
    format!("{:.6}", value)
}

/// Prints a success message.
pub fn print_success(message: &str) {
    println!("{} {}", "✓".green(), message);
}

/// Prints an error message.
pub fn print_error(message: &str) {
    eprintln!("{} {}", "✗".red(), message);
}

/// Prints a warning message.
pub fn print_warning(message: &str) {
    eprintln!("{} {}", "⚠".yellow(), message);
}

/// Prints an info message.
pub fn print_info(message: &str) {
    println!("{} {}", "ℹ".blue(), message);
}

/// A key-value pair for display.
#[derive(Debug, Clone, Serialize, Tabled)]
pub struct KeyValue {
    #[tabled(rename = "Metric")]
    pub key: String,
    #[tabled(rename = "Value")]
    pub value: String,
}

impl KeyValue {
    /// Creates a new key-value pair.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Creates a key-value pair from a decimal value.
    pub fn from_decimal(key: impl Into<String>, value: Decimal, precision: u32) -> Self {
        Self {
            key: key.into(),
            value: format!("{:.prec$}", value, prec = precision as usize),
        }
    }

    /// Creates a key-value pair formatted as percentage.
    pub fn from_percent(key: impl Into<String>, value: Decimal) -> Self {
        Self {
            key: key.into(),
            value: format!("{:.4}%", value * Decimal::from(100)),
        }
    }

    /// Creates a key-value pair formatted as basis points.
    pub fn from_bps(key: impl Into<String>, value: Decimal) -> Self {
        Self {
            key: key.into(),
            value: format!("{:.2} bps", value * Decimal::from(10000)),
        }
    }
}

/// Prints a header for a section.
pub fn print_header(title: &str) {
    println!("\n{}", title.bold().underline());
}

/// Prints a divider line.
pub fn print_divider() {
    println!("{}", "─".repeat(60).dimmed());
}
