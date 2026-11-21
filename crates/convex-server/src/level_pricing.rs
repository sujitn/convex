//! Pricing a stored bond from its quoted level at request time.
//!
//! Transport only: the engine's level stage prices the level. This module maps a request
//! to the stage's input, and reports what the stage read: the rule and set versions, the
//! benchmark securities' Live prices, the benchmark a spread was quoted to, and the
//! rule's width with each side's level when it applies. A report
//! names each benchmark's `instrument_id`, so the access layer requires the caller's
//! entitlement to it, as to the bond.

use axum::response::Response;
use convex_engine::level_pricing::{
    BenchmarkYield, BidAsk, LevelError, PricingRuleInForce, QuotedInput, RulePricing,
    LEVEL_PRICE_MODEL,
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

/// What the stage read and gave, added to a quote's body as `pricing_rule`: the rule as
/// read, the price the quote gave, and the benchmark and two-way it was quoted with.
pub(crate) fn pricing_rule_json(
    in_force: &PricingRuleInForce,
    input: QuotedInput,
    pricing: &RulePricing,
) -> Value {
    let mut body = rule_read_json(in_force, pricing);
    body["clean_price_mid"] = json!(pricing.clean_price_mid);
    if let QuotedInput::Level(level) = input {
        body["level"] = json!(level);
    }
    if let Some(benchmark) = &pricing.benchmark {
        body["benchmark"] = benchmark_json(benchmark);
    }
    if let BidAsk::RuleWidth(two_way) = &pricing.bid_ask {
        body["two_way"] = json!({
            "unit": two_way.width.unit(),
            "width": two_way.width.value(),
            "bid": two_way.bid_level,
            "ask": two_way.ask_level,
        });
    }
    body
}

/// The rule of a bond whose `model` values its level itself, a callable's yield to worst
/// on its workouts or OAS on HW1F trees, or a floating rate note's discount margin, with
/// no stage and no benchmark.
pub(crate) fn model_rule_json(
    model: &str,
    in_force: &PricingRuleInForce,
    input: QuotedInput,
) -> Value {
    let mut body = rule_head_json(model, in_force);
    if let QuotedInput::Level(level) = input {
        body["level"] = json!(level);
    }
    body
}

/// The model that priced the rule's level, and the rule's version and method.
pub(crate) fn rule_head_json(model: &str, in_force: &PricingRuleInForce) -> Value {
    json!({
        "model": model,
        "version": in_force.rule.version.get(),
        "method": in_force.rule.method,
    })
}

/// The rule as read: the stage's model, the rule's version and method, the set it names
/// with each security's Live price, and why no G-spread is published when it is not.
pub(crate) fn rule_read_json(in_force: &PricingRuleInForce, pricing: &RulePricing) -> Value {
    let mut body = rule_head_json(LEVEL_PRICE_MODEL, in_force);
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
    if let Some(reason) = pricing.g_spread_refused {
        body["g_spread_refused"] = json!(reason);
    }
    body
}

/// The benchmark a spread was quoted to, and the yield it was added to.
pub(crate) fn benchmark_json(benchmark: &BenchmarkYield) -> Value {
    json!({
        "tenor": benchmark.tenor,
        "instrument_id": benchmark.instrument_id.as_str(),
        "ytm": benchmark.ytm,
    })
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
            LevelError::Unsupported(_) => Self::unsupported_product(message),
            LevelError::Analytics(_) => Self::calculation(message),
            LevelError::Reference(error) => error.into(),
        }
    }
}
