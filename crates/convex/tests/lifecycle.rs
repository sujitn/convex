#[path = "../examples/lifecycle.rs"]
mod example;
#[test]
fn public_native_lifecycle_example_preserves_projected_recorded_and_pending_credit_states(
) -> Result<(), Box<dyn std::error::Error>> {
    let result = example::run()?;
    assert_eq!(result["certified"], false);
    assert_eq!(result["task"], "CVX-062");
    assert_eq!(result["bond_closed"]["status"], "closed");
    assert_eq!(result["credit_closed"]["status"], "closed");
    assert_eq!(
        result["credit_default_pending"]["status"],
        "default_pending"
    );
    assert_eq!(
        result["credit_default_pending"]["recovery"],
        serde_json::Value::Null
    );
    assert_eq!(
        result["credit_auction_cash_pending"]["status"],
        "default_cash_pending"
    );
    for o in result["credit_auction_cash_pending"]["obligations"]
        .as_array()
        .ok_or("missing obligations")?
    {
        assert_eq!(o["settled"], false);
        assert_eq!(o["recorded"]["value"], "0");
    }
    assert_eq!(result["facade_first_step"]["revision"], 1);
    assert_eq!(
        result["facade_first_step"]["obligations"]
            .as_array()
            .ok_or("missing obligations")?
            .len(),
        0
    );
    Ok(())
}
#[test]
fn standalone_lifecycle_entry_point_uses_public_facade() -> Result<(), Box<dyn std::error::Error>> {
    example::main()
}
