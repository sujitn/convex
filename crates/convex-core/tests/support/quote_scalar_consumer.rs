//! Standalone consumer of checked scalar APIs, without reference arithmetic.

use convex_core::types::{Compounding, Currency, Price, Spread, SpreadType, Yield};
use convex_core::ConvexResult;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;

fn input(case: &Value, field: &str) -> Decimal {
    Decimal::from_str(case[field].as_str().expect("predeclared Decimal string"))
        .expect("admitted exact Decimal input")
}

fn amount(value: Decimal) -> Value {
    json!({"kind":"decimal", "value":value.to_string(), "typed_rejection":false})
}

fn price(value: Price) -> Value {
    json!({"kind":"decimal", "value":value.as_percentage().to_string(),
        "currency":value.currency().to_string(), "typed_rejection":false})
}

fn spread(value: Spread) -> Value {
    json!({"kind":"decimal", "value":value.as_bps().to_string(),
        "spread_type":format!("{:?}",value.spread_type()), "typed_rejection":false})
}

pub fn consume(case: &Value) -> ConvexResult<Value> {
    let value = input(case, "value");
    let left_price = Price::new(value, Currency::USD);
    let left_spread = Spread::new(value, SpreadType::ZSpread);
    let left_yield = Yield::new(value, Compounding::SemiAnnual);
    Ok(
        match case["operation"]
            .as_str()
            .expect("declared scalar operation")
        {
            "yield_from_percentage" => {
                let value = Yield::from_percentage(value, Compounding::SemiAnnual)?;
                json!({"kind":"decimal", "value":value.value().to_string(),
                "compounding":format!("{:?}",value.compounding()), "typed_rejection":false})
            }
            "yield_as_percentage" => amount(left_yield.as_percentage()?),
            "yield_as_bps" => {
                json!({"kind":"integer", "value":left_yield.as_bps()?.to_string(), "typed_rejection":false})
            }
            "yield_display" => {
                json!({"kind":"text", "value":left_yield.to_string(), "typed_rejection":false})
            }
            "price_from_decimal" => price(Price::from_decimal(value, Currency::USD)?),
            "price_as_decimal" => amount(left_price.as_decimal()?),
            "price_to_dirty" => price(left_price.to_dirty(input(case, "other"))?),
            "price_to_clean" => price(left_price.to_clean(input(case, "other"))?),
            "price_dollar_value" => amount(left_price.dollar_value(input(case, "other"))?),
            "price_discount_or_premium" => amount(left_price.discount_or_premium()?),
            "price_add" => price((left_price + input(case, "other"))?),
            "price_subtract" => price((left_price - input(case, "other"))?),
            "spread_from_decimal" => spread(Spread::from_decimal(value, SpreadType::ZSpread)?),
            "spread_as_decimal" => amount(left_spread.as_decimal()?),
            "spread_as_percentage" => amount(left_spread.as_percentage()?),
            "spread_add" | "spread_subtract" => {
                let right_kind = if case["right_spread_type"] == "GSpread" {
                    SpreadType::GSpread
                } else {
                    SpreadType::ZSpread
                };
                let right = Spread::new(input(case, "other"), right_kind);
                if case["operation"] == "spread_add" {
                    spread((left_spread + right)?)
                } else {
                    spread((left_spread - right)?)
                }
            }
            _ => panic!("unknown predeclared scalar operation"),
        },
    )
}
