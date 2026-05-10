//! UI dropdown helpers: list available conventions, fetch defaults for a market/instrument pair.

use wasm_bindgen::prelude::*;

use convex_bonds::conventions::{ConventionKey, ConventionRegistry};

use crate::convert::{
    format_compounding, format_yield_convention, parse_instrument_type, parse_market,
};
use crate::dto::{ConventionOption, ConventionOptions, DefaultConventions};

/// Get available convention options for UI dropdowns.
#[wasm_bindgen]
pub fn get_convention_options() -> JsValue {
    let options = ConventionOptions {
        markets: vec![
            ConventionOption {
                value: "US".to_string(),
                label: "United States".to_string(),
            },
            ConventionOption {
                value: "UK".to_string(),
                label: "United Kingdom".to_string(),
            },
            ConventionOption {
                value: "Germany".to_string(),
                label: "Germany".to_string(),
            },
            ConventionOption {
                value: "France".to_string(),
                label: "France".to_string(),
            },
            ConventionOption {
                value: "Italy".to_string(),
                label: "Italy".to_string(),
            },
            ConventionOption {
                value: "Spain".to_string(),
                label: "Spain".to_string(),
            },
            ConventionOption {
                value: "Japan".to_string(),
                label: "Japan".to_string(),
            },
            ConventionOption {
                value: "Switzerland".to_string(),
                label: "Switzerland".to_string(),
            },
            ConventionOption {
                value: "Australia".to_string(),
                label: "Australia".to_string(),
            },
            ConventionOption {
                value: "Canada".to_string(),
                label: "Canada".to_string(),
            },
            ConventionOption {
                value: "Netherlands".to_string(),
                label: "Netherlands".to_string(),
            },
            ConventionOption {
                value: "Belgium".to_string(),
                label: "Belgium".to_string(),
            },
            ConventionOption {
                value: "Austria".to_string(),
                label: "Austria".to_string(),
            },
            ConventionOption {
                value: "Eurozone".to_string(),
                label: "Eurozone".to_string(),
            },
        ],
        instrument_types: vec![
            ConventionOption {
                value: "GovernmentBond".to_string(),
                label: "Government Bond".to_string(),
            },
            ConventionOption {
                value: "TreasuryBill".to_string(),
                label: "Treasury Bill".to_string(),
            },
            ConventionOption {
                value: "CorporateIG".to_string(),
                label: "Corporate IG".to_string(),
            },
            ConventionOption {
                value: "CorporateHY".to_string(),
                label: "Corporate HY".to_string(),
            },
            ConventionOption {
                value: "Municipal".to_string(),
                label: "Municipal".to_string(),
            },
            ConventionOption {
                value: "Agency".to_string(),
                label: "Agency".to_string(),
            },
            ConventionOption {
                value: "InflationLinked".to_string(),
                label: "Inflation Linked".to_string(),
            },
            ConventionOption {
                value: "CorporateFRN".to_string(),
                label: "Corporate FRN".to_string(),
            },
            ConventionOption {
                value: "Supranational".to_string(),
                label: "Supranational".to_string(),
            },
            ConventionOption {
                value: "CoveredBond".to_string(),
                label: "Covered Bond".to_string(),
            },
        ],
        yield_conventions: vec![
            ConventionOption {
                value: "Street".to_string(),
                label: "Street Convention".to_string(),
            },
            ConventionOption {
                value: "True".to_string(),
                label: "True Yield".to_string(),
            },
            ConventionOption {
                value: "ISMA".to_string(),
                label: "ISMA/ICMA".to_string(),
            },
            ConventionOption {
                value: "Simple".to_string(),
                label: "Simple Yield".to_string(),
            },
            ConventionOption {
                value: "Municipal".to_string(),
                label: "Municipal (Tax-Equiv)".to_string(),
            },
            ConventionOption {
                value: "Discount".to_string(),
                label: "Discount Yield".to_string(),
            },
            ConventionOption {
                value: "BondEquivalent".to_string(),
                label: "Bond Equivalent".to_string(),
            },
            ConventionOption {
                value: "Annual".to_string(),
                label: "Annual".to_string(),
            },
            ConventionOption {
                value: "Continuous".to_string(),
                label: "Continuous".to_string(),
            },
        ],
        compounding_methods: vec![
            ConventionOption {
                value: "SemiAnnual".to_string(),
                label: "Semi-Annual".to_string(),
            },
            ConventionOption {
                value: "Annual".to_string(),
                label: "Annual".to_string(),
            },
            ConventionOption {
                value: "Quarterly".to_string(),
                label: "Quarterly".to_string(),
            },
            ConventionOption {
                value: "Monthly".to_string(),
                label: "Monthly".to_string(),
            },
            ConventionOption {
                value: "Continuous".to_string(),
                label: "Continuous".to_string(),
            },
            ConventionOption {
                value: "Simple".to_string(),
                label: "Simple".to_string(),
            },
        ],
    };
    serde_wasm_bindgen::to_value(&options).unwrap_or(JsValue::NULL)
}

/// Get default conventions for a given market and instrument type.
#[wasm_bindgen]
pub fn get_default_conventions(market: String, instrument_type: String) -> JsValue {
    let market_enum = parse_market(&market);
    let inst_enum = parse_instrument_type(&instrument_type);

    let registry = ConventionRegistry::global();
    let key = ConventionKey::new(market_enum, inst_enum);

    let rules = if let Some(specific_rules) = registry.rules(&key) {
        specific_rules.clone()
    } else {
        registry.default_rules_for_market(market_enum)
    };

    let defaults = DefaultConventions {
        day_count: format!("{:?}", rules.accrual_day_count),
        yield_convention: format_yield_convention(rules.convention),
        compounding: format_compounding(rules.compounding),
        settlement_days: rules.settlement_rules.days,
        ex_dividend_days: rules.ex_dividend_rules.as_ref().map(|r| r.days),
        use_business_days: rules.settlement_rules.use_business_days,
    };

    serde_wasm_bindgen::to_value(&defaults).unwrap_or(JsValue::NULL)
}
