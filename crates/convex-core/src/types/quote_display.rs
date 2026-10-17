//! Fixed quote text without Decimal multiplication or precision-buffer growth.

use rust_decimal::Decimal;

pub(super) fn four_places(value: Decimal, percentage: bool) -> String {
    // Default Decimal text fits its own buffer. Only bounded String operations
    // add the two percentage digits and four displayed fractional places.
    let raw = value.to_string();
    let (sign, unsigned) = match raw.strip_prefix('-') {
        Some(value) => ("-", value),
        None => ("", raw.as_str()),
    };
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let mut fraction = fraction.chars();
    let mut integer = integer.to_owned();
    let shift = if percentage { 2 } else { 0 };
    for _ in 0..shift {
        integer.push(fraction.next().unwrap_or('0'));
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let mut output = format!("{sign}{integer}.");
    for _ in 0..4 {
        output.push(fraction.next().unwrap_or('0'));
    }
    output
}
