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
