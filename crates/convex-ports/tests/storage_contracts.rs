use convex_ports::error::TraitError;
use convex_ports::storage::{BondPricingConfig, Page, Pagination};

#[test]
fn pagination_is_bounded_and_does_not_overflow() {
    assert!(Pagination::new(usize::MAX, 0).validate().is_ok());
    assert!(Pagination::new(0, 1000).validate().is_ok());
    assert!(matches!(
        Pagination::new(0, 1001).validate(),
        Err(TraitError::InvalidInput(_))
    ));
    let page = Page {
        items: vec![1, 2],
        total: 5,
        offset: usize::MAX,
        limit: 2,
    };
    assert!(!page.has_more());
    assert!(Page::<()> {
        items: vec![],
        total: 5,
        offset: 0,
        limit: 0
    }
    .has_more());
}

#[test]
fn configuration_unknown_fields_fail_at_each_nested_boundary() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/pricing_config.json")).unwrap();
    for pointer in [
        "",
        "/applies_to",
        "/pricing_spec/MarketQuote",
        "/bid_ask_spread",
        "/analytics_curves",
    ] {
        let mut value = fixture.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown_config_setting".into(), serde_json::json!(true));
        assert!(
            serde_json::from_value::<BondPricingConfig>(value).is_err(),
            "unknown setting accepted in {pointer}"
        );
    }
}

#[test]
fn an_option_model_is_an_explicit_bounded_assumption_omitted_when_absent() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/pricing_config.json")).unwrap();
    let mut config: BondPricingConfig = serde_json::from_value(fixture).unwrap();
    assert!(serde_json::to_value(&config)
        .unwrap()
        .get("option_model")
        .is_none());
    convex_ports::storage::validate_pricing_config(&config).unwrap();
    let assumption = convex_ports::storage::OptionModelAssumption {
        mean_reversion: 0.03,
        volatility: 0.01,
        reason: "explicit desk assumption".into(),
    };
    config.option_model = Some(assumption.clone());
    convex_ports::storage::validate_pricing_config(&config).unwrap();
    let changes: [fn(&mut convex_ports::storage::OptionModelAssumption); 5] = [
        |m| m.mean_reversion = -0.01,
        |m| m.mean_reversion = 10.5,
        |m| m.volatility = f64::NAN,
        |m| m.volatility = 1.5,
        |m| m.reason = " ".into(),
    ];
    for change in changes {
        let mut model = assumption.clone();
        change(&mut model);
        config.option_model = Some(model);
        assert!(matches!(
            convex_ports::storage::validate_pricing_config(&config),
            Err(TraitError::InvalidInput(_))
        ));
    }
}
