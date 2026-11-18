//! Pricing a stored bond from its quoted level at request time.
//!
//! Transport only: the engine's level stage prices the level. This module maps a request
//! to the stage's input, and reports what the stage read: the rule and set versions, the
//! benchmark securities' Live prices, and the benchmark a spread was quoted to. A report
//! names each benchmark's `instrument_id`, so the access layer requires the caller's
//! entitlement to it, as to the bond.

use axum::response::Response;
use convex_engine::level_pricing::{
    LevelError, PricingRuleInForce, QuotedInput, RulePricing, LEVEL_PRICE_MODEL,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};

use crate::error::ApiError;

/// The one input a quote request sends: `market_price`, or `level` for a bond whose rule
/// quotes it by one. Checked before the rule and its benchmarks are read.
pub(crate) fn quoted_input(
    market_price: Option<Decimal>,
    level: Option<Decimal>,
) -> Result<QuotedInput, ApiError> {
    match (market_price, level) {
        (Some(_), Some(_)) => Err(ApiError::invalid("send market_price or level, not both")),
        (Some(price), None) => Ok(QuotedInput::Price(price)),
        (None, Some(level)) => Ok(QuotedInput::Level(level)),
        (None, None) => Err(ApiError::missing_input(
            "market_price or level is required; no model or market value is substituted",
        )),
    }
}

/// The mark a `POST /quote` or batch item sends: `market_price`, `level`, or neither for
/// a quote with no mark.
pub(crate) fn optional_quoted_input(
    market_price: Option<Decimal>,
    level: Option<Decimal>,
) -> Result<Option<QuotedInput>, ApiError> {
    match (market_price, level) {
        (None, None) => Ok(None),
        (market_price, level) => quoted_input(market_price, level).map(Some),
    }
}

/// What the stage read and gave, added to a quote's body as `pricing_rule`.
pub(crate) fn pricing_rule_json(
    in_force: &PricingRuleInForce,
    input: QuotedInput,
    pricing: &RulePricing,
) -> Value {
    let mut body = json!({
        "model": LEVEL_PRICE_MODEL,
        "version": in_force.rule.version.get(),
        "method": in_force.rule.method,
        "clean_price_mid": pricing.clean_price_mid,
    });
    if let QuotedInput::Level(level) = input {
        body["level"] = json!(level);
    }
    if let Some(set) = &in_force.benchmark_set {
        body["benchmark_set"] = json!({
            "benchmark_set_id": set.benchmark_set_id,
            "version": set.version.get(),
            "securities": set.quotes.iter().map(|quote| json!({
                "tenor": quote.security.tenor,
                "instrument_id": quote.security.instrument_id.as_str(),
                "version": quote.security.version.get(),
                "clean_price_mid": quote.clean_price_mid,
                "snapshot_id": quote.snapshot_id,
            })).collect::<Vec<_>>(),
        });
    }
    if let Some(benchmark) = &pricing.benchmark {
        body["benchmark"] = json!({
            "tenor": benchmark.tenor,
            "instrument_id": benchmark.instrument_id.as_str(),
            "ytm": benchmark.ytm,
        });
    }
    if let Some(reason) = pricing.g_spread_refused {
        body["g_spread_refused"] = json!(reason);
    }
    body
}

/// The rule's and set's versions, as the reference version is reported; none for a bond
/// with no rule.
pub(crate) fn with_pricing_rule_headers(
    response: Response,
    in_force: Option<&PricingRuleInForce>,
) -> Result<Response, ApiError> {
    use crate::curves::with_header;
    let Some(in_force) = in_force else {
        return Ok(response);
    };
    let response = with_header(
        response,
        "x-convex-pricing-rule-version",
        &in_force.rule.version.get().to_string(),
    )?;
    match &in_force.benchmark_set {
        Some(set) => with_header(
            response,
            "x-convex-benchmark-set-version",
            &set.version.get().to_string(),
        ),
        None => Ok(response),
    }
}

impl From<LevelError> for ApiError {
    fn from(error: LevelError) -> Self {
        let message = error.to_string();
        match error {
            LevelError::Refused(_) => Self::invalid(message),
            LevelError::BenchmarkUnavailable { .. } => Self::missing_input(message),
            LevelError::Analytics(_) => Self::calculation(message),
            LevelError::Reference(error) => error.into(),
        }
    }
}
