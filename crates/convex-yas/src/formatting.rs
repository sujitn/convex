//! YAS screen formatting utilities.
//!
//! This module provides formatting functions to display YAS analysis
//! in a format similar to Bloomberg YAS screens.

use crate::yas::YasAnalysis;
use rust_decimal::Decimal;

/// Format a yield value with standard precision.
pub fn format_yield(yield_val: Decimal) -> String {
    format!("{:.6}%", yield_val)
}

/// Format a spread value in basis points.
pub fn format_spread_bps(spread: Decimal) -> String {
    format!("{:.1} bps", spread)
}

/// Format a duration value.
pub fn format_duration(duration: Decimal) -> String {
    format!("{:.3}", duration)
}

/// Format a price value.
pub fn format_price(price: Decimal) -> String {
    format!("{:.6}", price)
}

/// Format a money amount.
pub fn format_money(amount: Decimal) -> String {
    // Add thousands separators
    let s = format!("{:.2}", amount);
    add_thousands_separator(&s)
}

/// Add thousands separators to a number string.
fn add_thousands_separator(s: &str) -> String {
    let parts: Vec<&str> = s.split('.').collect();
    let integer_part = parts[0];
    let decimal_part = parts.get(1).unwrap_or(&"");

    let chars: Vec<char> = integer_part.chars().rev().collect();
    let formatted: String = chars
        .chunks(3)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<String>>()
        .join(",")
        .chars()
        .rev()
        .collect();

    if decimal_part.is_empty() {
        formatted
    } else {
        format!("{}.{}", formatted, decimal_part)
    }
}

/// Format YAS analysis as a Bloomberg-style screen.
pub fn format_yas_screen(analysis: &YasAnalysis) -> String {
    let mut output = String::new();

    output.push_str("┌─────────────────────────────────────────────────────────┐\n");
    output.push_str("│                    YIELD ANALYSIS                        │\n");
    output.push_str("├─────────────────────────────────────────────────────────┤\n");

    output.push_str("│ YIELDS                                                   │\n");
    output.push_str(&format!(
        "│   Street Convention:    {:>12}                    │\n",
        format_yield(analysis.street_convention)
    ));
    output.push_str(&format!(
        "│   True Yield:           {:>12}                    │\n",
        format_yield(analysis.true_yield)
    ));
    output.push_str(&format!(
        "│   Current Yield:        {:>12}                    │\n",
        format_yield(analysis.current_yield)
    ));

    output.push_str("├─────────────────────────────────────────────────────────┤\n");
    output.push_str("│ SPREADS                                                  │\n");
    output.push_str(&format!(
        "│   G-Spread:             {:>12}                    │\n",
        format_spread_bps(analysis.g_spread)
    ));
    output.push_str(&format!(
        "│   I-Spread:             {:>12}                    │\n",
        format_spread_bps(analysis.i_spread)
    ));
    output.push_str(&format!(
        "│   Z-Spread:             {:>12}                    │\n",
        format_spread_bps(analysis.z_spread)
    ));

    output.push_str("├─────────────────────────────────────────────────────────┤\n");
    output.push_str("│ RISK METRICS                                             │\n");
    output.push_str(&format!(
        "│   Modified Duration:    {:>12}                    │\n",
        format_duration(analysis.modified_duration)
    ));
    output.push_str(&format!(
        "│   Convexity:            {:>12}                    │\n",
        format_duration(analysis.convexity)
    ));
    output.push_str(&format!(
        "│   DV01:                 {:>12}                    │\n",
        format!("${:.4}", analysis.dv01)
    ));

    output.push_str("└─────────────────────────────────────────────────────────┘\n");

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_format_yield() {
        assert_eq!(format_yield(dec!(4.905895)), "4.905895%");
    }

    #[test]
    fn test_format_spread_bps() {
        assert_eq!(format_spread_bps(dec!(448.5)), "448.5 bps");
    }

    #[test]
    fn test_format_money() {
        assert_eq!(format_money(dec!(1132016.11)), "1,132,016.11");
    }

    #[test]
    fn test_add_thousands_separator() {
        assert_eq!(add_thousands_separator("1234567.89"), "1,234,567.89");
        assert_eq!(add_thousands_separator("100"), "100");
        assert_eq!(add_thousands_separator("1000"), "1,000");
    }
}
